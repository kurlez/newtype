# newtype

Menu-bar voice input for macOS (Apple Silicon) and Windows. This repository is milestone 0: the tray app, a settings window, and in-app updates. Speech recognition, the language model, hotkeys, and text insertion are module stubs only.

The product notes live outside this repository. The behavior below is the release and update contract for this milestone.

## Version scheme

The comparable version is what the updater orders. The string in the tray and the About section also shows the commit.

| Build | Updater version | Shown in the app |
| --- | --- | --- |
| Local, or CI without `GITHUB_RUN_NUMBER` | `0.1.0-dev.0` | `0.1.0-dev.0+<7-char commit>` |
| Push to `main`, and pull requests | `0.1.0-dev.<github.run_number>` | `0.1.0-dev.<run>+<7-char commit>` |
| Git tag `v1.2.3` | `1.2.3` | `1.2.3+<7-char commit>` |

`0.1.0` in `src-tauri/tauri.conf.json` is the **next stable** version, not a prerelease. Dev builds insert `-dev.<number>` in front of it. Ordering, which is what `tauri-plugin-updater` uses (`semver::Version`'s `Ord`), is:

```text
0.1.0-dev.1 < 0.1.0-dev.2 < 0.1.0-dev.10 < 0.1.0 < 0.2.0-dev.1 < 0.2.0
```

The `+commit` suffix is display-only. It is not part of the package version. SemVer treats `+…` as build metadata, and this updater compares metadata alphabetically, so a commit hash is not a monotonic build number. A `+` in the version also breaks installer file names. The number after `-dev.` is `github.run_number` for `.github/workflows/build.yml`. Do not rename that file; the counter would restart and new builds would look older.

Pull-request builds use the same counter but are not published. The next main build still has a higher number.

A stable tag must be `v` plus plain `MAJOR.MINOR.PATCH`. After you publish `v0.1.0`, bump the plain version in `tauri.conf.json` and `src-tauri/Cargo.toml` to `0.2.0` before the next main push. Otherwise `0.1.0-dev.N` stays older than `0.1.0` and the updater will not offer it. See [SETUP.md](SETUP.md).

## Building locally

Requirements: Node 22, Rust stable (see `rust-toolchain.toml`), and the platform libraries Tauri documents for a desktop build.

```bash
npm install
npm test
npm run app:dev
```

`npm run app:dev` is the supported dev command. It stamps the running app as `0.1.0-dev.0+<commit>`. Closing the window leaves the process in the menu bar (macOS) or the system tray (Windows). Quit is on that menu.

A local installer needs the updater signing key in the environment, because update artifacts are signed at bundle time. `.env` files are not read.

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$HOME/.tauri/newtype.key")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD='the password from SETUP.md'

npm run app:build -- --bundles app,dmg    # macOS Apple Silicon
npm run app:build -- --bundles nsis       # Windows x64
```

Generate that key with the commands in [SETUP.md](SETUP.md). Do not commit it. Linux can run `npm run app:dev` for UI work. CI does not ship a Linux installer.

## Release and update flow

```text
push to main ──► build macOS arm64 (.dmg + .app.tar.gz) and Windows x64 (NSIS)
                 sign the updater artifacts with TAURI_SIGNING_PRIVATE_KEY
                 upload them and latest.json to the public dev prerelease

tag v1.2.3  ──► same artifacts, uploaded as a non-prerelease v1.2.3
```

The source repository is private, so clients cannot download from it. Artifacts go to the public repository `https://github.com/kurlez/newtype-releases`.

| Channel | In-app endpoint | Release |
| --- | --- | --- |
| dev (default) | `https://github.com/kurlez/newtype-releases/releases/download/dev/latest.json` | Rolling prerelease tag `dev`, replaced on every main push |
| stable | `https://github.com/kurlez/newtype-releases/releases/latest/download/latest.json` | Newest non-prerelease `v*` tag |

`latest.json` lists the signed updater files only: macOS `.app.tar.gz` and Windows `-setup.exe`. The `.dmg` is attached for the first macOS install and is not an updater payload. The app checks the selected channel at startup and about once an hour, downloads in the background, and asks before restarting into the new build. Switching from a newer build to an older channel does not downgrade.

Pull requests build both installers and do not publish. If `TAURI_SIGNING_PRIVATE_KEY` is missing, CI signs with a throwaway key so the bundle step still succeeds. The publish job uploads only when both `TAURI_SIGNING_PRIVATE_KEY` and `NEWTYPE_RELEASES_TOKEN` are set, and only after `minisign` verifies each updater artifact against `plugins.updater.pubkey`. A placeholder key or a mismatched pair stops the upload and leaves the previous release in place.

The public key embedded in the app is not an Apple certificate. It only verifies updater payloads.

## First install

Install once from the [dev release](https://github.com/kurlez/newtype-releases/releases/tag/dev). Later main builds arrive through the app.

### macOS

The `.dmg` is not notarized and is not Developer ID signed.

1. Open the `.dmg` and drag **newtype** to Applications.
2. Do not double-click it the first time. Gatekeeper blocks an app whose developer it cannot verify.
3. In Finder, right-click (or Control-click) **newtype** → **Open**, then click **Open** in the dialog.
4. If a double-click was already blocked: **System Settings → Privacy & Security**, scroll to the block message, and click **Open Anyway**. macOS may ask for an administrator password.
5. Later launches from the same path should open without that prompt, until a new signature identity makes macOS treat the app as a different program.

首次打开（未公证）：

1. 打开 `.dmg`，把 newtype 拖到「应用程序」。
2. 第一次不要双击。Gatekeeper 会提示无法验证开发者。
3. 在 Finder 中右键（或按住 Control 点击）该应用 → **打开**，再在对话框里点 **打开**。
4. 如果已经双击失败：打开 **系统设置 → 隐私与安全性**，找到被拦截的提示，点 **仍要打开**。可能需要管理员密码。

CI ad-hoc signs every macOS build unless `APPLE_CERTIFICATE`, `APPLE_CERTIFICATE_PASSWORD`, and `APPLE_SIGNING_IDENTITY` are all set. Ad-hoc signing changes the code hash every build. A stable self-signed identity is the mitigation for Accessibility and Microphone grants surviving an update. That has not been verified on a real Mac. Hardened runtime stays off. This milestone does not request those permissions. Details and the certificate steps are in [SETUP.md](SETUP.md).

### Windows

The NSIS installer is per-user and is not Authenticode signed.

1. Run the `-setup.exe`.
2. If SmartScreen shows “Windows protected your PC”, click **More info**, then **Run anyway**.

## Layout

`src-tauri/src/pipeline/` holds the later pipeline (ASR, VAD, hot words, LLM, router, hotkeys, text insertion, history). None of those register a hotkey, open the microphone, or insert text. The settings window lists them as stubs.

The tray menu shows the full version, the commit, and **Check for updates**. The About section in the window shows the same version and commit.

## Setup checklist

The remaining owner steps — updater keypair, the three required secret names, the releases token, and the optional macOS certificate — are in [SETUP.md](SETUP.md).
