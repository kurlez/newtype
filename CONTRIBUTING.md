# Contributing

Thanks for looking at newtype. The end-user guide is [README.md](README.md) and [README.zh-CN.md](README.zh-CN.md). This file is for people changing the code.

newtype is not affiliated with or endorsed by Typeless. Hotkey warnings may still name Typeless when a chord matches a shortcut that app also uses. That is a conflict notice, not a product claim.

## Build

Requirements: Node 22, Rust stable (`rust-toolchain.toml`), and the libraries in the [Tauri prerequisites](https://tauri.app/start/prerequisites/).

```bash
npm install
npm test
cargo test --manifest-path src-tauri/Cargo.toml --lib
npm run app:dev
```

`npm run app:dev` stamps the running app as a local dev build. The window opens on launch and stays in the taskbar and Alt+Tab. Closing it minimizes. Quit is on the tray menu. Linux can run the UI. CI does not ship a Linux installer.

Do not commit API keys, updater private keys, `.p12` files, or `.env` files. A local installer needs `TAURI_SIGNING_PRIVATE_KEY` in the environment because update artifacts are signed at bundle time. `.env` files are not read. Generate the key with the commands in [SETUP.md](SETUP.md).

```bash
export TAURI_SIGNING_PRIVATE_KEY="$(cat "$HOME/.tauri/newtype.key")"
export TAURI_SIGNING_PRIVATE_KEY_PASSWORD='the password from SETUP.md'

npm run app:build -- --bundles app,dmg    # macOS Apple Silicon
npm run app:build -- --bundles nsis       # Windows x64
```

## Tests

`npm test` covers the version, release, and UI helper scripts.

```bash
cargo test --manifest-path src-tauri/Cargo.toml --lib
```

That covers hot-word replacement, left/right hotkeys, routing, the rewrite fallback, the update-channel default, and log scrubbing. It does not call a provider.

To run the cleanup cases against a configured provider:

```bash
export NEWTYPE_PROMPT_API_KEY='your key'
# optional
export NEWTYPE_PROMPT_BASE='https://api.openai.com/v1'
export NEWTYPE_PROMPT_MODEL='gpt-4o-mini'
export NEWTYPE_PROMPT_LEVEL='both'   # or literal, or polish
export NEWTYPE_PROMPT_CASES='10,13,14,15'

cargo test --manifest-path src-tauri/Cargo.toml --lib prompt_harness -- --nocapture
```

CI does not set `NEWTYPE_PROMPT_API_KEY`, so `prompt_harness` returns without a network call. A missing key is a skip, not a failure.

`prompts/v2-polish.system.txt` is the default polish prompt. `prompts/v1-literal.system.txt` is the literal level. Both keep the MIT attribution to ckckck/OpenTypeless in `prompts/ATTRIBUTION.md`. The transcript is untrusted data inside `<transcript>`. No prompt text is copied from Open-Less/openless.

## Layout

`src-tauri/src/pipeline/` is the dictation path: cloud speech, hot words, prompts, the router, hotkeys, paste, history, and Silero VAD. `prompts/` holds the versioned prompts. Volcengine speech is the openspeech adapter in `pipeline/volcengine.rs`. DashScope speech uses the Model Studio multimodal generation endpoint from the workspace base URL.

The tray menu starts with **History** and **Show last result**, then the path, **Start or stop dictation**, the full version, the commit, and **Check for updates**. The window opens on History.

## What still needs a real Mac or Windows machine

This repository’s CI cannot grant Accessibility, open a microphone, or paste into another app.

- Right Ctrl on both Mac and Windows starts recording when that is still the saved chord. Left Ctrl does not. Hold-to-talk releases cleanly.
- A keyboard with no Right Ctrl can keep a saved Right Alt chord (`AltRight`, `AltRight+ShiftRight`, `AltRight+Enter`). Changing a chord is Capture, then Set. The settings page warns about a shortcut clash and still saves. `newtype.log` records the applied chord. The press used to capture does not start dictation.
- The indicator shows recording, processing, and error, and names Cloud or the local path. On Windows the default place is the taskbar strip.
- The first launch asks for the microphone. macOS also needs Accessibility, and some versions need Input Monitoring before the hotkey tap runs.
- Dictation pastes into the focused app and the previous clipboard comes back.
- A rewrite timeout still inserts the hot-word text.
- After an update, check whether Microphone and Accessibility survived. Ad-hoc signing can drop them.

## Releases

Version scheme, signing, channels, and the publish job are in [docs/RELEASING.md](docs/RELEASING.md). The one-time secret checklist is in [SETUP.md](SETUP.md). Optional self-hosted runner notes are in [scripts/runner/README.md](scripts/runner/README.md). CI does not use those runners.
