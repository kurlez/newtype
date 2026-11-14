# newtype

[English](README.md) · [简体中文](README.zh-CN.md)

AI voice input method for macOS and Windows.

Hold a hotkey to dictate. newtype transcribes your speech, optionally polishes the wording, and pastes the result into the app you were using. The same hotkey path can translate, or act on selected text. Cloud providers and local models are both supported. newtype is not affiliated with or endorsed by Typeless.

## What it does

- **Dictate.** Right Ctrl starts and stops a recording. The default mode is toggle. Hold-to-talk is in Settings. Escape cancels.
- **Polish.** The default cleanup smooths wording and does not answer the request. Literal mode stays closer to the transcript. A personal dictionary replaces terms before the model sees the text.
- **Translate.** A second chord translates into the language you set (English by default).
- **Command.** A third chord uses the selected text when something is selected, and otherwise treats the dictation as a writing request. Translate and command need a cloud model, or a local rewrite model of at least 4B.
- **Auto routing.** The default path uses your cloud provider when it answers, switches to local models when that provider stays unreachable, and returns to cloud after the connection holds. Cloud only never uses local models. Local only stays on this computer. Strict offline also blocks probes, update checks, and model downloads.
- **Local models.** Qwen3-ASR for speech, Qwen3 for rewrite, Silero VAD to trim silence on the local path, sherpa-onnx and llama.cpp as the runtimes. Weights are downloaded when you ask for them. They are not inside the installer.
- **Cloud presets.** Groq is the default for a new install. OpenAI, Volcengine (火山引擎), DashScope (通义 / 百炼), and a custom OpenAI-compatible endpoint are included.
- **History.** Entries stay on this computer. The default keeps 30 days and the newest 300.
- **Windows taskbar strip.** While you record, a strip can sit in the empty space just left of the clock. It floats if that space is too small. macOS uses a floating indicator. Either can be hidden.
- **Updates.** A new install checks the stable channel, downloads in the background, and asks before restarting. You can switch to the dev channel in Settings. A channel you already saved is left as it is.

## Screenshots

Screenshots are not in this repository yet.

<!-- TODO: add product screenshots before wider promotion.
     Suggested: the settings window, the Windows taskbar strip, and History.
     Files under src-tauri/icons and app-icon.png are the app icon, not screenshots.
     Do not add placeholder images. -->

## Download

Install the current stable build from:

**[github.com/kurlez/newtype-releases/releases/latest](https://github.com/kurlez/newtype-releases/releases/latest)**

| Platform | File |
| --- | --- |
| Windows 64-bit | `newtype_<ver>_x64-setup.exe` |
| Mac, Apple Silicon | `newtype_<ver>_aarch64.dmg` |

`<ver>` is the version on that release, for example `newtype_0.2.0_x64-setup.exe`. There is no Intel Mac build and no Linux installer.

The `.dmg` and the Windows setup program are the first-install files. Later updates come from inside the app. The updater uses the signed `.app.tar.gz` (Mac) and the signed `-setup.exe` (Windows), not the `.dmg`.

## Requirements

- **macOS 12 or later**, Apple Silicon.
- **Windows 10 or later**, 64-bit.
- A microphone.
- For cloud dictation: a network connection and an API key for the provider you pick.
- For local models: several gigabytes of free disk. A Mac with 16 GB of RAM or more uses Qwen3-ASR 1.7B and Qwen3 4B. Windows, and Macs below that, use Qwen3-ASR 0.6B and Qwen3 1.7B. Sizes and checksums are in [docs/local-models.md](docs/local-models.md).

## Install

The builds are not notarized and are not signed with an Apple Developer ID or a Windows Authenticode certificate.

### macOS

1. Open the `.dmg` and drag **newtype** to Applications.
2. Do not double-click it the first time. Gatekeeper blocks an app whose developer it cannot verify.
3. In Finder, right-click (or Control-click) **newtype** → **Open**, then click **Open** in the dialog.
4. If a double-click was already blocked: **System Settings → Privacy & Security**, scroll to the block message, and click **Open Anyway**. macOS may ask for an administrator password.
5. Later launches from the same path should open without that prompt, until a new signature identity makes macOS treat the app as a different program. An update may ask for Microphone and Accessibility again.

### Windows

The installer is per-user.

1. Run `newtype_<ver>_x64-setup.exe`.
2. If SmartScreen shows “Windows protected your PC”, click **More info**, then **Run anyway**.

## First run

1. Allow the microphone. On macOS, also allow **Accessibility**. If the hotkey does nothing, allow **Input Monitoring** as well. The settings window has buttons that open the matching system pane.
2. The default provider is **Groq**. Open Settings, paste an API key, and click **Save key**. The key is written to the macOS Keychain or Windows Credential Manager and read back before the window says it was saved. It is not stored in `dictation.json`.
3. Focus another app. Press **Right Ctrl** to start, speak, and press **Right Ctrl** again to stop. newtype pastes the text and then restores the previous clipboard. On macOS that paste is Command+V. On Windows it is Ctrl+V.
4. Optional: add dictionary terms, change the route to Local only or Strict offline, or download local models. Strict offline cannot download, so download first if you want those models.

## API keys

Create a key in the provider’s console, then paste it into Settings. **Load models** calls `GET /v1/models` with the saved key. The model ids below are the ones a new install suggests. You can type another id.

| Provider | Speech | Rewrite | Where to create a key |
| --- | --- | --- | --- |
| Groq (default) | `whisper-large-v3-turbo` | `openai/gpt-oss-20b` | [console.groq.com/keys](https://console.groq.com/keys) |
| OpenAI | `gpt-transcribe` | `gpt-4o-mini` | [platform.openai.com/api-keys](https://platform.openai.com/api-keys) |
| Volcengine · 火山 | openspeech `bigmodel` | `doubao-seed-2-1-lite-260915` (pro: `doubao-seed-2-1-pro-260915`) | Speech: [豆包语音控制台](https://console.volcengine.com/speech/app). Rewrite: [火山方舟 Ark](https://console.volcengine.com/ark). |
| DashScope · 通义 / 百炼 | `qwen-audio-3.1-asr-flash` | `qwen3.8-flash` (also `qwen3.8-max`) | [百炼控制台](https://bailian.console.aliyun.com/). Paste a base URL that ends with `/compatible-mode/v1`. |
| OpenAI-compatible | you type it | you type it | Your own endpoint. `https`, or `http` on localhost. |

**Groq.** One key covers speech and rewrite. Rewrite uses reasoning effort `low`, which is the lowest that `openai/gpt-oss-20b` accepts. A separate “Groq + GPT-4o mini” bundle uses the Groq key for speech and an OpenAI key for rewrite. Those keys are stored separately.

**Volcengine.** Speech uses the openspeech socket (`volc.seedasr.sauc.duration`, model name `bigmodel`). On the new console, save the speech API key. On the old console, save the App ID and the access token. The rewrite key is the Ark key, in the Volcengine API key field. Thinking is turned off. `doubao-seed-asr-1-0` is not used.

**DashScope.** Speech is Qwen-Audio ASR on the Model Studio multimodal generation endpoint, with dictionary terms sent as instant hotwords. Paste a workspace base URL that ends with `/compatible-mode/v1`, for example `https://dashscope.aliyuncs.com/compatible-mode/v1` or `https://{WorkspaceId}.cn-beijing.maas.aliyuncs.com/compatible-mode/v1`. Rewrite uses the chat API on that same base, with thinking requested off. This path is HTTP. It is not the realtime WebSocket message model.

The in-app bundles quote about $1.8 / month for Groq, about $2.5 for Groq plus GPT-4o mini, and about $8 for OpenAI, at 200 dictations a day. Those figures were checked on 2026-11-12 and are estimates, not a bill.

## Local models

Leave the route on **Auto** if you only want a fallback, or switch to **Local only** / **Strict offline** when the files are installed. In Settings, download the recommended speech model and rewrite model. Silero VAD is installed with a speech model and trims silence before local recognition. If that file is missing, the whole recording is recognized on device and nothing is sent out because of it.

A local rewrite model below 4B does not translate and does not run commands. On a Mac at 16 GB or above, the default rewrite model is Qwen3 4B, so those modes can run locally. On Windows the default rewrite model is Qwen3 1.7B, so translate and command stay on a cloud model unless you install a 4B model.

Strict offline blocks the download. Download on Auto or Local only, then switch. Details, checksums, and licenses: [docs/local-models.md](docs/local-models.md).

## Hotkeys

macOS and Windows use the same defaults. Left and right modifiers stay distinct. An empty saved chord means “use the default”.

| Action | Default |
| --- | --- |
| Dictate | Right Ctrl (`ControlRight`, not Left Ctrl) |
| Translate | Right Ctrl + Right Shift |
| Command | Right Ctrl + Enter |
| Cancel a recording | Escape |

Toggle is the default: press to start, press again to stop. Hold-to-talk stops when you release the chord.

Some keyboards have no Right Ctrl key. The defaults stay Right Ctrl. Open **Hotkeys**, click **Capture**, press a chord that exists (Right Alt works), then click **Set**. Set writes the chord and the listener uses it immediately. Cancel or Escape during capture leaves the saved chord in place. A clash with another app’s shortcut, including chords also used by Typeless, is a warning. Set still saves the chord.

## Privacy

- **API keys** are stored in the macOS Keychain or Windows Credential Manager. `dictation.json` does not contain them. A startup check writes and deletes a throwaway credential. If that check fails, Settings says the store is unavailable.
- **No telemetry.** newtype does not phone home with usage stats.
- **Logs** are a rolling `newtype.log` in the app log directory. The settings page and the tray menu show that path. A line records metadata: phase, route, model id, character counts, and error text. It does not record the transcript or the audio. Before a line is written, API-key-shaped text is removed, including `sk-…`, Groq `gsk_…`, Volcengine `AKLT…`, Alibaba `LTAI…`, and other long token-like strings.
- **Cloud.** On a cloud path, the recording is sent to the speech provider you picked, and the transcript plus the prompt (including dictionary terms used as hints) is sent to the rewrite provider. Auto does that when the provider is reachable. Local only does not send speech or transcripts to a cloud provider. Strict offline also blocks connectivity probes, update checks, and model downloads. The local llama.cpp server is reached at `127.0.0.1` and keeps working in strict offline.
- **History** is plaintext JSON on this computer. The default retention is 30 days, and only the newest 300 entries in that window are kept. Settings → History → **Don't keep** stores nothing new. **Clear** deletes what is already there. If a cloud step fails and the recording cannot be finished on this computer, that entry may keep a wav beside the history file. Deleting the entry removes the wav. History audio is not written into the log.
- **Strict offline** is the switch for “do not leave this computer.” It does not by itself delete history you already have.

## FAQ

**The hotkey does nothing.** On macOS, grant Accessibility, then Input Monitoring if it still does not fire. On both systems, confirm the microphone permission and that the saved chord is the key you are pressing. Left Ctrl is not the default.

**This keyboard has no Right Ctrl.** Use Capture, then Set. The warning about another app’s shortcut does not block the save.

**macOS says the developer cannot be verified.** Use the right-click → Open steps above. Do not rely on the first double-click.

**SmartScreen blocks the installer.** More info → Run anyway.

**Updates do not appear.** A new install follows **stable**, which only moves when a version tag is published. Main-branch builds are on the **dev** channel. Switching channel does not downgrade. Strict offline turns update checks off. A channel already saved in `settings.json` is not changed by an upgrade.

**Translate or command did not insert.** Those modes need a cloud rewrite model or a local model of at least 4B. The transcript stays in History when the result is withheld.

**Where is the log?** Settings and the tray menu show the `newtype.log` path. It is metadata, not a transcript.

## Build from source

You need Node 22, Rust stable (see `rust-toolchain.toml`), and the platform libraries [Tauri documents](https://tauri.app/start/prerequisites/) for a desktop build.

```bash
npm install
npm test
cargo test --manifest-path src-tauri/Cargo.toml --lib
npm run app:dev
```

`npm run app:dev` is the supported dev command. The window opens on launch. Closing it minimizes. Quit is on the tray menu. Linux can run the UI this way. CI does not ship a Linux installer.

A local installer also needs the updater signing key in the environment. Do not commit that key. The release steps are in [docs/RELEASING.md](docs/RELEASING.md).

## Contributing

Issues and pull requests are welcome. See [CONTRIBUTING.md](CONTRIBUTING.md) for tests, the prompt harness, and the manual checks that need a real Mac or Windows machine. Version numbers, signing, and the publish flow are in [docs/RELEASING.md](docs/RELEASING.md).

## License

newtype is dual-licensed under the MIT License or the Apache License 2.0, at your option. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE). Copyright 2026 Wenfeng Liu.

Other licenses — Rust crates (including the MPL-2.0 crates cssparser, selectors, option-ext, and dtoa-short), npm packages, downloaded models, FLEURS samples, and the OpenTypeless prompt attribution — are summarized in [THIRD_PARTY_NOTICES.md](THIRD_PARTY_NOTICES.md).

newtype is not affiliated with or endorsed by Typeless.
