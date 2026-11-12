#!/usr/bin/env bash
# Install or remove the newtype macOS arm64 GitHub Actions runner.
# The registration token is an argument or a hidden prompt. It is not written to disk.
set -euo pipefail

repo_url="https://github.com/kurlez/newtype"
runner_name="newtype-mac"
runner_root="${HOME}/actions-runners/newtype-mac"

usage() {
  cat <<'EOF'
Usage: setup-mac.sh [--token TOKEN] [--uninstall]

Registers a self-hosted runner named newtype-mac with the label newtype.
Omit --token to type the token at a hidden prompt. A token passed on the
command line can remain in shell history, so the prompt is safer.
--uninstall uses a remove token from the same GitHub API.
EOF
}

uninstall=0
token=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --uninstall)
      uninstall=1
      shift
      ;;
    --token)
      if [[ $# -lt 2 ]]; then
        echo "Missing value for --token." >&2
        exit 2
      fi
      token="$2"
      shift 2
      ;;
    -h|--help)
      usage
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      usage >&2
      exit 2
      ;;
  esac
done

if [[ "${EUID}" -eq 0 ]]; then
  echo "Run this as the normal login user, not root. svc.sh then installs a LaunchAgent for that user." >&2
  exit 1
fi

clear_token() {
  token=""
}
trap clear_token EXIT

require_token() {
  local label="$1"
  if [[ -n "$token" ]]; then
    return 0
  fi
  local value=""
  IFS= read -r -s -p "${label}: " value
  printf '\n'
  token="$value"
  if [[ -z "$token" ]]; then
    echo "A token is required." >&2
    exit 1
  fi
}

script_dir="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
repo_root="$(cd "${script_dir}/../.." && pwd)"

# Compare dotted versions. Do not pipe rustc into grep -q: under pipefail,
# grep -q closes the pipe early and rustc dies with SIGPIPE.
version_at_least() {
  local actual="$1"
  local required="$2"
  local IFS=.
  local -a actual_parts=()
  local -a required_parts=()
  read -r -a actual_parts <<< "$actual"
  read -r -a required_parts <<< "$required"
  local i actual_n required_n
  for ((i = 0; i < 3; i++)); do
    actual_n="${actual_parts[$i]:-0}"
    required_n="${required_parts[$i]:-0}"
    if ((10#$actual_n > 10#$required_n)); then
      return 0
    fi
    if ((10#$actual_n < 10#$required_n)); then
      return 1
    fi
  done
  return 0
}

service_path() {
  local cargo_bin="${HOME}/.cargo/bin"
  local result="$cargo_bin"
  local dir
  for dir in /opt/homebrew/bin /opt/homebrew/sbin /usr/local/bin /usr/bin /bin /usr/sbin /sbin; do
    if [[ -d "$dir" && ":${result}:" != *":${dir}:"* ]]; then
      result="${result}:${dir}"
    fi
  done
  local part
  local IFS=':'
  local -a current_parts=()
  read -r -a current_parts <<< "${PATH}"
  for part in "${current_parts[@]}"; do
    if [[ -n "$part" && ":${result}:" != *":${part}:"* ]]; then
      result="${result}:${part}"
    fi
  done
  printf '%s' "$result"
}

write_runner_path() {
  local path_value
  path_value="$(service_path)"
  printf '%s\n' "$path_value" > "${runner_root}/.path"
  local tmp
  tmp="$(mktemp)"
  if [[ -f "${runner_root}/.env" ]]; then
    awk -v path_value="$path_value" '
      BEGIN { found = 0 }
      /^PATH=/ { print "PATH=" path_value; found = 1; next }
      { print }
      END { if (!found) print "PATH=" path_value }
    ' "${runner_root}/.env" > "$tmp"
  else
    printf 'PATH=%s\n' "$path_value" > "$tmp"
  fi
  mv "$tmp" "${runner_root}/.env"
  echo "Runner PATH starts with ${HOME}/.cargo/bin."
}

ensure_build_tools() {
  if ! xcode-select -p >/dev/null 2>&1; then
    echo "Xcode Command Line Tools are missing. Run: xcode-select --install" >&2
    exit 1
  fi
  if ! command -v brew >/dev/null 2>&1; then
    echo "Homebrew is required and was not found on PATH." >&2
    exit 1
  fi
  if ! command -v node >/dev/null 2>&1 || ! command -v npm >/dev/null 2>&1; then
    brew install node
  fi
  export PATH="${HOME}/.cargo/bin:${PATH}"
  if [[ ! -x "${HOME}/.cargo/bin/rustup" ]]; then
    brew install rustup
    if command -v rustup-init >/dev/null 2>&1; then
      rustup-init -y --no-modify-path --default-toolchain stable --profile minimal
    else
      curl --proto '=https' --tlsv1.2 -fsSL https://sh.rustup.rs | sh -s -- -y --no-modify-path --default-toolchain stable --profile minimal
    fi
  fi
  export PATH="${HOME}/.cargo/bin:${PATH}"
  hash -r
  if [[ ! -x "${HOME}/.cargo/bin/rustup" ]]; then
    echo "rustup was not installed at ${HOME}/.cargo/bin/rustup." >&2
    exit 1
  fi
  rustup toolchain install stable --profile minimal
  rustup default stable
  rustup target add aarch64-apple-darwin
  local rustc_version release msrv
  rustc_version="$(rustc -vV)"
  printf '%s\n' "$rustc_version"
  release="$(printf '%s\n' "$rustc_version" | sed -n 's/^release: //p')"
  msrv="$(sed -n 's/^rust-version = "\([^"]*\)"/\1/p' "${repo_root}/src-tauri/Cargo.toml")"
  if [[ "$rustc_version" != *"host: aarch64-apple-darwin"* ]]; then
    echo "rustc host is not aarch64-apple-darwin." >&2
    exit 1
  fi
  if [[ -z "$msrv" || -z "$release" ]] || ! version_at_least "$release" "$msrv"; then
    echo "rustc ${release:-unknown} is older than rust-version ${msrv:-missing} in src-tauri/Cargo.toml." >&2
    exit 1
  fi
  node -v
  npm -v
}

download_runner() {
  local meta name url tarball expected actual
  meta="$(mktemp)"
  tarball="$(mktemp)"
  curl -fsSL -H "User-Agent: newtype-runner-setup" -H "Accept: application/vnd.github+json" \
    https://api.github.com/repos/actions/runner/releases/latest -o "$meta"
  name="$(node -e '
    const fs = require("fs");
    const release = JSON.parse(fs.readFileSync(process.argv[1], "utf8"));
    const asset = (release.assets || []).find((item) => /^actions-runner-osx-arm64-.+\.tar\.gz$/.test(item.name));
    const checksum = String(release.body || "").match(/<!-- BEGIN SHA osx-arm64 -->([a-fA-F0-9]{64})<!-- END SHA osx-arm64 -->/);
    if (!asset || !checksum) process.exit(1);
    process.stdout.write(asset.name + "\n" + asset.browser_download_url + "\n" + checksum[1].toLowerCase());
  ' "$meta")" || {
    rm -f "$meta" "$tarball"
    echo "The latest actions/runner release has no osx-arm64 archive or checksum." >&2
    exit 1
  }
  expected="${name##*$'\n'}"
  url="${name#*$'\n'}"
  url="${url%%$'\n'*}"
  name="${name%%$'\n'*}"
  curl -fsSL -H "User-Agent: newtype-runner-setup" -o "$tarball" "$url"
  actual="$(shasum -a 256 "$tarball" | awk '{ print tolower($1) }')"
  if [[ -z "$expected" || "$expected" != "$actual" ]]; then
    rm -f "$meta" "$tarball"
    echo "Runner archive checksum did not match ${name}." >&2
    exit 1
  fi
  mkdir -p "$runner_root"
  tar -xzf "$tarball" -C "$runner_root"
  rm -f "$meta" "$tarball"
  echo "Installed runner files for ${name}."
}

start_service() {
  # svc.sh status exits 0 even when the agent is not installed, so match its text.
  local status_text
  status_text="$(./svc.sh status || true)"
  printf '%s\n' "$status_text"
  if [[ "$status_text" == *"not installed"* ]]; then
    ./svc.sh install
  elif [[ "$status_text" == *"Started:"* ]]; then
    ./svc.sh stop
  fi
  ./svc.sh start
}

if [[ "$uninstall" -eq 1 ]]; then
  require_token "Remove token"
  if [[ -d "$runner_root" ]]; then
    (
      cd "$runner_root"
      ./svc.sh stop || true
      ./svc.sh uninstall || true
      ./config.sh remove --token "$token"
    )
    rm -rf "$runner_root"
  fi
  clear_token
  echo "Removed ${runner_name}."
  exit 0
fi

ensure_build_tools
mkdir -p "$runner_root"
if [[ ! -x "${runner_root}/config.sh" ]]; then
  download_runner
fi
cd "$runner_root"
if [[ ! -f .runner ]]; then
  require_token "Registration token"
  ./config.sh --unattended \
    --url "$repo_url" \
    --token "$token" \
    --name "$runner_name" \
    --labels newtype \
    --replace
  clear_token
else
  echo "Runner ${runner_name} is already configured."
fi
write_runner_path
start_service
echo "Runner ${runner_name} is installed for ${USER}. It listens only while this user is logged in."
