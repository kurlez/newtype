# One-time setup

The app in this repository cannot publish updates until these steps are done. Pull-request builds still compile without them. Publishing runs only on pushes to `main` and on `v*` tags, and it skips itself with a notice until the two required secrets exist.

Do this on your own machine. Do not commit the private key, a `.p12`, or a `.env` file.

## 1. Generate the updater keypair

From the repository root, with Node dependencies installed (`npm install`):

```bash
mkdir -p "$HOME/.tauri"
npm run tauri signer generate -- -w "$HOME/.tauri/newtype.key" -p 'choose-a-long-password'
```

This writes:

- `$HOME/.tauri/newtype.key` — private key. This is the secret.
- `$HOME/.tauri/newtype.key.pub` — public key. This is safe to commit.

The password is whatever you passed to `-p`. You will store it as a second secret. If you omit `-p`, the command prompts for one.

## 2. Put the public key in the app

Replace `plugins.updater.pubkey` in `src-tauri/tauri.conf.json` with the full contents of `$HOME/.tauri/newtype.key.pub` (one base64 line).

Leave `src-tauri/placeholder-updater-pubkey.txt` unchanged. The publish job compares the two. While they match, it refuses to publish, because the private key for that placeholder was never saved.

Commit and push the `tauri.conf.json` change. The first build that contains your public key is the first build that can verify updates you sign. Anything installed earlier, including a build that still has the placeholder key, will reject those updates and has to be installed again by hand.

## 3. Add repository secrets

On the private source repository **kurlez/newtype** (Settings → Secrets and variables → Actions), add:

| Secret | Required | Value |
| --- | --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | yes | Entire contents of `$HOME/.tauri/newtype.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | yes, if the key has a password | The `-p` password from step 1 |
| `NEWTYPE_RELEASES_TOKEN` | yes | Token from step 4 |
| `APPLE_CERTIFICATE` | no | Base64 of a stable macOS signing `.p12`. See below. |
| `APPLE_CERTIFICATE_PASSWORD` | no | Password for that `.p12` |
| `APPLE_SIGNING_IDENTITY` | no | Certificate name, for example `newtype-self-signed` |

The same three Apple secrets must be set together. If any one is missing, the macOS job fails on purpose instead of silently falling back.

With the GitHub CLI, from a shell where `gh` is logged in as you:

```bash
gh secret set TAURI_SIGNING_PRIVATE_KEY --repo kurlez/newtype < "$HOME/.tauri/newtype.key"
gh secret set TAURI_SIGNING_PRIVATE_KEY_PASSWORD --repo kurlez/newtype
# paste the password, then Ctrl-D

gh secret set NEWTYPE_RELEASES_TOKEN --repo kurlez/newtype
# paste the token, then Ctrl-D
```

`TAURI_SIGNING_PRIVATE_KEY` is the key text, not a path. The build machine does not have your home directory.

## 4. Create `NEWTYPE_RELEASES_TOKEN`

Updates are published to the public repository `https://github.com/kurlez/newtype-releases` because this source repository is private and installed apps cannot download from it.

Create a fine-grained personal access token:

1. GitHub → Settings → Developer settings → Personal access tokens → Fine-grained tokens → Generate new token.
2. Resource owner: `kurlez`.
3. Repository access: **Only select repositories** → `newtype-releases`. Do not grant this token access to `newtype`.
4. Permissions: **Contents: Read and write**. No other repository permission.
5. Set an expiry you will remember. When it expires, publishing stops until you replace the secret.

A classic token whose only scope is `public_repo` also works. Prefer the fine-grained token.

Store the token value as `NEWTYPE_RELEASES_TOKEN` on **kurlez/newtype**, not on newtype-releases. The workflow runs in the private repo and uses the token only to create releases in the public one.

`newtype-releases` must stay public. The installed app downloads `latest.json` and the update files with no credentials.

## 5. Optional: stable macOS signing identity

macOS builds are not Developer ID signed and not notarized. By default CI ad-hoc signs them (`signingIdentity` is `-`, hardened runtime is off). An ad-hoc signature is a hash of that specific build, so the code identity changes on every update. macOS may then treat the new build as a different app and drop Accessibility and Microphone grants. This build does not request those permissions yet. Whether a stable identity keeps them must be checked on a real Mac after two successive updates, and it is not a promise until that check passes.

To try a stable identity:

1. On a Mac, open Keychain Access → Certificate Assistant → Create a Certificate.
2. Name: `newtype-self-signed`. Identity type: Self Signed Root. Certificate type: Code Signing.
3. In Keychain Access, confirm `security find-identity -v -p codesigning` lists it. If it does not, `codesign` will not use it. Leave the Apple secrets unset and stay on ad-hoc.
4. Export the certificate and its private key as a `.p12` with a password.
5. `openssl base64 -A -in newtype-self-signed.p12`
6. Set `APPLE_CERTIFICATE` to that base64 string, `APPLE_CERTIFICATE_PASSWORD` to the export password, and `APPLE_SIGNING_IDENTITY` to `newtype-self-signed`.

After two dev updates, look at System Settings → Privacy & Security → Accessibility and Microphone and record whether the grants survived. Delete the `.p12` from any machine that does not need it. Do not commit it.

## 6. Install the first build

After the secrets and the public key are on `main`, the publish job uploads a **dev** prerelease to `https://github.com/kurlez/newtype-releases/releases/tag/dev`.

- macOS: download the `.dmg`.
- Windows: download the NSIS `-setup.exe`.

First-launch steps are in the README. After that install, leave the in-app channel on **dev**. Later pushes to `main` publish a higher `0.1.0-dev.<run>` build; the app checks at startup and about once an hour, downloads in the background, and asks you to restart.

## 7. After the first stable tag

Tagging `v0.1.0` publishes version `0.1.0` on the stable channel. That version is newer than every `0.1.0-dev.N`. Before further work lands on `main`, bump the plain version in both `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` to the next stable number (`0.2.0`). Dev builds then become `0.2.0-dev.<run>`, which is newer than `0.1.0`. If you skip the bump, anyone who installed `0.1.0` will not be offered the older-looking dev builds.

Do not rename `.github/workflows/build.yml`. Its `github.run_number` is the dev build counter, and renaming the file starts that counter over.
