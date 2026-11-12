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

## 2. Public key in the app

`plugins.updater.pubkey` in `src-tauri/tauri.conf.json` is the public half of the key stored in `TAURI_SIGNING_PRIVATE_KEY`. Leave it in place.

`src-tauri/placeholder-updater-pubkey.txt` is the original placeholder, whose private key was never saved. The publish job refuses to upload while `plugins.updater.pubkey` still equals that file. Do not edit the placeholder file, and do not paste it back into `tauri.conf.json`.

Anything installed from a build that still contained the placeholder key cannot verify updates signed with the real key. Install a build from this public key onward.

## 3. Repository secrets

`TAURI_SIGNING_PRIVATE_KEY` and `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` are already set on **kurlez/newtype**. The remaining required secret is `NEWTYPE_RELEASES_TOKEN` (step 4). Publishing stays skipped until that token exists.

On the private source repository **kurlez/newtype** (Settings → Secrets and variables → Actions), the full set is:

| Secret | Required | Value |
| --- | --- | --- |
| `TAURI_SIGNING_PRIVATE_KEY` | yes | Entire contents of `$HOME/.tauri/newtype.key` |
| `TAURI_SIGNING_PRIVATE_KEY_PASSWORD` | yes, if the key has a password | The `-p` password from step 1 |
| `NEWTYPE_RELEASES_TOKEN` | yes | Token from step 4 |
| `APPLE_CERTIFICATE` | no | Base64 of a stable self-signed macOS `.p12`. See section 5. Not an Apple Developer ID. |
| `APPLE_CERTIFICATE_PASSWORD` | no | Password for that `.p12` |
| `APPLE_SIGNING_IDENTITY` | no | Certificate common name, exactly `newtype-self-signed` unless you chose another name |

The three Apple secrets are optional and must be set together. If all three are absent, macOS CI ad-hoc signs and stays green. If only some of them are set, the macOS job fails instead of guessing.

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

## 5. Optional: stable self-signed macOS identity

Do this once, on a Mac, if you want updates to keep the same code identity. It does **not** use an Apple Developer ID and it does **not** notarize the app. Gatekeeper still needs the right-click → Open step on first launch. Hardened runtime stays off.

Ad-hoc signing (`signingIdentity` `-`) is a hash of that specific build. macOS can treat the next update as a different app and drop Microphone and Accessibility. A stable self-signed certificate is the mitigation. It is not a promise that those grants survive until you check two successive updates on a real Mac.

Tauri's built-in `APPLE_CERTIFICATE` importer only accepts Apple certificate names such as `Developer ID Application:`. CI therefore imports the `.p12` itself (`scripts/import-macos-identity.sh`) and then signs with the certificate name only. If the three secrets below are absent, that script does nothing and the build stays ad-hoc.

### Create the certificate

1. On a Mac, open Keychain Access → Certificate Assistant → Create a Certificate.
2. Name: `newtype-self-signed`. Identity type: **Self Signed Root**. Certificate type: **Code Signing**. Leave the defaults unless you know you need a longer lifetime.
3. Run `security find-identity -v -p codesigning`. The output must contain `"newtype-self-signed"`. If it does not, `codesign` will not use the certificate. Leave the Apple secrets unset and stay on ad-hoc. Do not switch the certificate type to something else to force it to appear.
4. In Keychain Access, export the certificate **and its private key** as a `.p12`. Pick a long password.
5. Base64, one line: `openssl base64 -A -in newtype-self-signed.p12`
6. On **kurlez/newtype** (Settings → Secrets and variables → Actions), set exactly these three secrets:

| Secret | Value |
| --- | --- |
| `APPLE_CERTIFICATE` | The base64 string from step 5 |
| `APPLE_CERTIFICATE_PASSWORD` | The `.p12` export password |
| `APPLE_SIGNING_IDENTITY` | `newtype-self-signed` |

```bash
gh secret set APPLE_CERTIFICATE --repo kurlez/newtype < newtype-self-signed.b64
gh secret set APPLE_CERTIFICATE_PASSWORD --repo kurlez/newtype
# paste the .p12 password, then Ctrl-D
gh secret set APPLE_SIGNING_IDENTITY --repo kurlez/newtype <<< 'newtype-self-signed'
```

7. Delete the `.p12` and the base64 file from any machine that does not need them. Do not commit them.

The next macOS CI run imports that keychain, signs the `.app` and every nested Mach-O with `newtype-self-signed`, and checks that they match. A bad password or a certificate that is not a Code Signing identity fails that job. Unset all three secrets to go back to ad-hoc.

`llama-server` and its `.dylib` files are downloaded after install, so they are not inside the `.app`. On a Mac whose keychain has this identity, the app signs that whole runtime tree with the same name. On a machine that only has the installed app, the private key is absent and the app ad-hoc signs the tree instead (still one identity for every file in it, libraries first, `--timestamp=none`). The private key is never shipped. Microphone and Accessibility follow the **app** identity, not the helper identity.

After two dev updates, look at System Settings → Privacy & Security → Accessibility and Microphone and record whether the grants survived. Until that check, the app still tells you that an update may ask for them again.

## 6. Install the first build

After the secrets and the public key are on `main`, the publish job uploads a **dev** prerelease to `https://github.com/kurlez/newtype-releases/releases/tag/dev`.

- macOS: download the `.dmg`.
- Windows: download the NSIS `-setup.exe`.

First-launch steps are in the README. After that install, leave the in-app channel on **dev**. Later pushes to `main` publish a higher `0.1.0-dev.<run>` build; the app checks at startup and about once an hour, downloads in the background, and asks you to restart.

## 7. After the first stable tag

Tagging `v0.1.0` publishes version `0.1.0` on the stable channel. That version is newer than every `0.1.0-dev.N`. Before further work lands on `main`, bump the plain version in both `src-tauri/tauri.conf.json` and `src-tauri/Cargo.toml` to the next stable number (`0.2.0`). Dev builds then become `0.2.0-dev.<run>`, which is newer than `0.1.0`. If you skip the bump, anyone who installed `0.1.0` will not be offered the older-looking dev builds.

Do not rename `.github/workflows/build.yml`. Its `github.run_number` is the dev build counter, and renaming the file starts that counter over.
