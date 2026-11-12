#!/usr/bin/env bash
# After the macOS bundle step: the .app and every nested Mach-O share one signature.
# APPLE_SIGNING_IDENTITY set -> that certificate. Unset -> ad-hoc.
set -euo pipefail

root="$(cd "$(dirname "$0")/.." && pwd)"
app="$(find "${root}/src-tauri/target/release/bundle" -name '*.app' -type d | head -n 1 || true)"
if [[ -z "$app" ]]; then
  echo "No .app bundle was produced."
  exit 1
fi

info="$(codesign -dv --verbose=4 "$app" 2>&1 || true)"
echo "$info"
identity="${APPLE_SIGNING_IDENTITY:-}"
if [[ -n "$identity" ]]; then
  if grep -q '^Signature=adhoc' <<<"$info"; then
    echo "APPLE_SIGNING_IDENTITY is set, but ${app} is ad-hoc signed."
    exit 1
  fi
  if ! grep -F "Authority=${identity}" <<<"$info" >/dev/null; then
    echo "Expected Authority=${identity}."
    exit 1
  fi
else
  if ! grep -q '^Signature=adhoc' <<<"$info"; then
    echo "No Apple secret was set, so the bundle must stay ad-hoc."
    exit 1
  fi
fi

app_kind="$(grep -m1 '^Signature=' <<<"$info" || true)"
mismatches=0
while IFS= read -r bin; do
  [[ -n "$bin" ]] || continue
  bin_info="$(codesign -dv --verbose=4 "$bin" 2>&1 || true)"
  bin_kind="$(grep -m1 '^Signature=' <<<"$bin_info" || echo "unsigned")"
  if [[ "$bin_kind" != "$app_kind" ]]; then
    echo "Signature mismatch: ${bin}"
    echo "  app: ${app_kind}"
    echo "  file: ${bin_kind}"
    mismatches=1
  elif [[ -n "$identity" ]] && ! grep -F "Authority=${identity}" <<<"$bin_info" >/dev/null; then
    echo "Nested Mach-O is not signed by ${identity}: ${bin}"
    mismatches=1
  fi
done < <(find "$app" -type f -exec file {} + | awk -F: '/Mach-O/ {print $1}')

if [[ "$mismatches" -ne 0 ]]; then
  exit 1
fi
echo "macOS signature is consistent (${app_kind})."
