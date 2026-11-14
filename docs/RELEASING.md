# Releasing

End users install from the stable release. This file is the maintainer flow that was previously in the README. The one-time secret checklist is [SETUP.md](../SETUP.md). Optional self-hosted runners are [scripts/runner/README.md](../scripts/runner/README.md). CI does not use those runners, and they should not be registered for a public repository or for pull requests from forks.

Installed apps download from the public repository `https://github.com/kurlez/newtype-releases`. They do not need credentials. Publish jobs upload there. Whether this source repository is public does not change that.

## Version scheme

The comparable version is what the updater orders. The string in the tray and the About section also shows the commit.

| Build | Updater version | Shown in the app |
| --- | --- | --- |
| Local, or CI without `GITHUB_RUN_NUMBER` | `0.2.1-dev.0` | `0.2.1-dev.0+<7-char commit>` |
| Push to `main`, and pull requests | `0.2.1-dev.<github.run_number>` | `0.2.1-dev.<run>+<7-char commit>` |
| Git tag `v0.2.0` | `0.2.0` | `0.2.0+<7-char commit>` |

`0.2.0` in `src-tauri/tauri.conf.json` is the **latest stable** version. Dev builds use the next patch, so they sort above that release. Ordering, which is what `tauri-plugin-updater` uses (`semver::Version`'s `Ord`), is:

```text
0.1.0-dev.89 < 0.2.0 < 0.2.1-dev.1 < 0.2.1-dev.10 < 0.2.1
```

The `+commit` suffix is display-only. It is not part of the package version. SemVer treats `+…` as build metadata, and this updater compares metadata alphabetically, so a commit hash is not a monotonic build number. A `+` in the version also breaks installer file names. The number after `-dev.` is `github.run_number` for `.github/workflows/build.yml`. Do not rename that file; the counter would restart and new builds would look older.

Pull-request builds use the same counter but are not published. The next main build still has a higher number.

A stable tag must be `v` plus plain `MAJOR.MINOR.PATCH`. The tag sets the published version. After you publish `v0.2.1`, bump the plain version in `tauri.conf.json`, `src-tauri/Cargo.toml`, and `package.json` to `0.2.1` before the next main push. Dev builds then become `0.2.2-dev.N`, which is newer than `0.2.1`. If you skip the bump, `0.2.1-dev.N` stays older than `0.2.1` and the updater will not offer it. See [SETUP.md](../SETUP.md).

Do not bump the version by hand except for that post-tag step.

## Release and update flow

```text
push to main ──► build macOS arm64 (.dmg + .app.tar.gz) and Windows x64 (NSIS)
                 sign the updater artifacts with TAURI_SIGNING_PRIVATE_KEY
                 upload them and latest.json to the public dev prerelease

tag v0.2.0  ──► same artifacts, uploaded as a non-prerelease v0.2.0
                 also copied onto the dev feed when that feed is older than 0.2.0
```

| Channel | In-app endpoint | Release |
| --- | --- | --- |
| stable (default for a new install) | `https://github.com/kurlez/newtype-releases/releases/latest/download/latest.json` | Newest non-prerelease `v*` tag |
| dev | `https://github.com/kurlez/newtype-releases/releases/download/dev/latest.json` | Rolling prerelease tag `dev`, replaced on every main push |

A machine with no saved channel uses **stable**. Loading `settings.json` keeps `"dev"` or `"stable"` exactly as stored. Nothing migrates a saved `dev` channel to stable.

`latest.json` lists the signed updater files only: macOS `.app.tar.gz` and Windows `-setup.exe`. The `.dmg` is attached for the first macOS install and is not an updater payload. The app checks the selected channel at startup and about once an hour, downloads in the background, and asks before restarting into the new build. Switching from a newer build to an older channel does not downgrade.

Pull requests run the tests and both installers on GitHub-hosted runners (`windows-latest` and `macos-latest`). A pull request, including one from a fork, does not receive signing or publish secrets. It signs the updater with a throwaway key and ad-hoc signs the Mac app. Publish runs on `ubuntu-latest` only for a push to `main`, a `v*` tag, or a manual run of one of those refs, and only when both `TAURI_SIGNING_PRIVATE_KEY` and `NEWTYPE_RELEASES_TOKEN` are set. Each updater artifact is checked against `plugins.updater.pubkey` before upload. A placeholder key or a mismatched pair stops the upload and leaves the previous release in place.

## Signing

The public key embedded in the app is not an Apple certificate. It only verifies updater payloads.

A local or CI installer needs the updater signing key in the environment. `.env` files are not read. Generate and store it with [SETUP.md](../SETUP.md). Do not commit it.

Pull requests are ad-hoc signed. A push to `main` or a `v*` tag ad-hoc signs unless `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, and `APPLE_SIGNING_IDENTITY` are all set. Those three secrets are a self-signed Code Signing certificate, not an Apple Developer ID, and they are optional: if they are absent the job stays on ad-hoc and stays green. Ad-hoc signing changes the code hash every build. The stable identity is the mitigation for Accessibility and Microphone grants surviving an update. Hardened runtime stays off. `llama-server` and its libraries are signed with that same identity when the private key is in the keychain, and ad-hoc as a set otherwise.

### macOS first open

The `.dmg` is not notarized and is not Developer ID signed. The user steps (right-click → Open, then Open Anyway) are in the README.

### Windows

The NSIS installer is per-user and is not Authenticode signed. SmartScreen: **More info**, then **Run anyway**.

## Setup checklist

Do this once, on your own machine. Do not commit the private key, a `.p12`, or a `.env` file. Commands and the exact secret names are in [SETUP.md](../SETUP.md).

1. Generate the updater keypair (`TAURI_SIGNING_PRIVATE_KEY` and its password).
2. Keep `plugins.updater.pubkey` in `src-tauri/tauri.conf.json` equal to the public half. Do not paste the placeholder key back.
3. Create `NEWTYPE_RELEASES_TOKEN` with contents read/write on `newtype-releases` only.
4. Optional, and only as a set: `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, `APPLE_SIGNING_IDENTITY` for a stable self-signed Mac identity.

After those secrets exist, a push to `main` publishes the dev feed. Tagging `v*` publishes stable. A new install follows stable. Switch the in-app channel to dev when you want every main build.
