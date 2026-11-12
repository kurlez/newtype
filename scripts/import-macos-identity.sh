#!/usr/bin/env bash
# Import the stable self-signed macOS identity, or leave the build on ad-hoc.
#
# Tauri's own APPLE_CERTIFICATE importer only accepts Apple Developer ID name
# prefixes, so this script imports the PKCS#12 and the next step signs with
# the certificate name alone. No Apple Developer ID and no notarization.
#
# Secrets, all three or none:
#   APPLE_CERTIFICATE           base64 of the .p12
#   APPLE_CERTIFICATE_PASSWORD  export password
#   APPLE_SIGNING_IDENTITY      certificate common name, for example newtype-self-signed
set -euo pipefail

identity="${APPLE_SIGNING_IDENTITY:-}"
certificate="${APPLE_CERTIFICATE:-}"
password="${APPLE_CERTIFICATE_PASSWORD:-}"

if [[ -z "$identity" && -z "$certificate" && -z "$password" ]]; then
  echo "No Apple certificate secret. The app will be ad-hoc signed (signingIdentity -)."
  echo "The cdhash changes every build, so Accessibility and Microphone grants may not survive updates."
  exit 0
fi

if [[ -z "$identity" || -z "$certificate" || -z "$password" ]]; then
  echo "Set all of APPLE_CERTIFICATE, APPLE_CERTIFICATE_PASSWORD, and APPLE_SIGNING_IDENTITY, or none of them."
  exit 1
fi

if [[ -z "${GITHUB_ENV:-}" || -z "${RUNNER_TEMP:-}" ]]; then
  echo "This import step is for GitHub Actions (GITHUB_ENV and RUNNER_TEMP)."
  exit 1
fi

echo "::add-mask::${certificate}"
echo "::add-mask::${password}"
echo "Stable macOS signing identity is configured. Hardened runtime stays off. This is not Developer ID and not notarization."

p12="${RUNNER_TEMP}/newtype-signing.p12"
keychain="${RUNNER_TEMP}/newtype-signing.keychain-db"
keychain_password="$(openssl rand -base64 24)"
echo "::add-mask::${keychain_password}"

python3 - "$p12" <<'PY'
import base64, os, pathlib, sys
raw = os.environ["APPLE_CERTIFICATE"]
try:
    data = base64.b64decode(raw, validate=True)
except Exception as exc:
    raise SystemExit(f"APPLE_CERTIFICATE is not valid base64: {exc}")
if not data.startswith(b"\x30"):
    raise SystemExit("APPLE_CERTIFICATE did not decode to a PKCS#12 (expected an ASN.1 SEQUENCE).")
pathlib.Path(sys.argv[1]).write_bytes(data)
print(f"Decoded PKCS#12 ({len(data)} bytes).")
PY

if ! openssl pkcs12 -in "$p12" -nokeys -passin env:APPLE_CERTIFICATE_PASSWORD >/dev/null 2>"${RUNNER_TEMP}/p12.err"; then
  if ! openssl pkcs12 -in "$p12" -nokeys -legacy -passin env:APPLE_CERTIFICATE_PASSWORD >/dev/null 2>>"${RUNNER_TEMP}/p12.err"; then
    echo "Could not read the PKCS#12. Check APPLE_CERTIFICATE_PASSWORD."
    exit 1
  fi
fi

security create-keychain -p "$keychain_password" "$keychain"
security set-keychain-settings -lut 21600 "$keychain"
security unlock-keychain -p "$keychain_password" "$keychain"
security import "$p12" -k "$keychain" -P "$password" -T /usr/bin/codesign -T /usr/bin/security
security set-key-partition-list -S apple-tool:,apple:,codesign: -s -k "$keychain_password" "$keychain" >/dev/null
existing="$(security list-keychains -d user | tr -d '"')"
# shellcheck disable=SC2086
security list-keychains -d user -s "$keychain" $existing
security default-keychain -s "$keychain"

listed="$(security find-identity -v -p codesigning "$keychain")"
echo "$listed"
if ! grep -F "\"${identity}\"" <<<"$listed" >/dev/null; then
  echo "security find-identity did not list \"${identity}\"."
  echo "Create the certificate in Keychain Access as a Code Signing certificate named exactly ${identity}, export the .p12, and set the three secrets again."
  echo "A certificate without the Code Signing extended key usage will not appear in this list. Ad-hoc signing is what you get when the secrets are unset."
  exit 1
fi

rm -f "$p12"
# Do not export the PKCS#12. Tauri would treat it as a Developer ID certificate and reject this self-signed one.
{
  echo "APPLE_SIGNING_IDENTITY<<NEWTYPE_APPLE_ID"
  printf '%s\n' "$identity"
  echo "NEWTYPE_APPLE_ID"
} >>"$GITHUB_ENV"
echo "Imported ${identity}. The bundle step will codesign with that name and will not see APPLE_CERTIFICATE."
