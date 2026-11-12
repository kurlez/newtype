# Local models (M2)

Auto is the default. It uses cloud speech and rewrite while the configured endpoint answers, and local Qwen speech and rewrite when the probe has marked that endpoint unreachable. A dictation that already started does not change path if the probe flips during the recording. If the cloud call itself fails, that same recording or transcript is retried locally when the models are installed. Cloud only never makes that retry. Strict offline does not probe, download, or check for updates.

Audio for a local session is not given to the cloud client. llama.cpp is reached at `127.0.0.1` with proxies disabled.

## What runs where

| Machine | Speech | Rewrite |
| --- | --- | --- |
| Mac, 16 GB class and up (physical RAM at least 15 GiB) | Qwen3-ASR 1.7B Q8_0 through `llama-server` | Qwen3 4B Q4_K_M |
| Mac below that | Qwen3-ASR 0.6B int8, sherpa-onnx, in process | Qwen3 1.7B Q4_K_M |
| Windows x64 | Qwen3-ASR 0.6B int8, sherpa-onnx, in process | 4B at 16 GB class, otherwise 1.7B |

16 GB class means at least 15 GiB reported by the OS, so a 16 GB machine that reports a little under 16×1024³ still gets the larger models. The 0.6B speech model remains the Mac fallback when 1.7B is missing or the server fails to transcribe. Windows does not offer 1.7B speech.

The owner's M4 Pro (24 GB) measured about 0.3 s for an 18 s clip on 1.7B Q8_0 and about 2.8 GB peak. The same family on a virtual M2 Pro runner was much slower; that run is in `docs/spikes/qwen3-asr-mac.md` and is not the sizing rule used here.

Thinking is off for local rewrite (`--reasoning off`, plus `/no_think` and `enable_thinking: false` when the server accepts it). The local polish prompt is the shortened file `prompts/v2-polish-local.system.txt`. If the rewrite fails, times out, or looks like an answer, the ruled transcript is inserted.

Silero VAD trims silence before local speech recognition. A missing or unloadable VAD file does not send audio anywhere; the whole recording is recognized and the note says so.

One speech engine is resident at a time. The rewrite model is a second `llama-server`. Both unload after 5 minutes idle. Opening Settings reads the model directory. It does not load weights and it does not contact the network.

## Files

Weights and the llama.cpp runtime download into the Tauri app data directory, under `models/<id>/`. That is `~/Library/Application Support/app.newtype.desktop/models/` on macOS and `%APPDATA%\app.newtype.desktop\models\` on Windows. They are not in the `.dmg` or the NSIS installer. sherpa-onnx 2.0.1 is linked into the app binary, so a machine that already has the 0.6B files can recognize without downloading sherpa again.

| Id | Files | SHA-256 | Source |
| --- | --- | --- | --- |
| `qwen3-asr-1.7b-q8` | `Qwen3-ASR-1.7B-Q8_0.gguf` (2,165,034,944 bytes), `mmproj-Qwen3-ASR-1.7B-Q8_0.gguf` (355,709,344) | `58e22d0532d4eacaf034cfac17a6fed159f37c41390c710186783be439d1fc57`, `46c1d533af3f354ceb37ce855dbceff7da7fa7cf1e6a523df3b13440bd164c0d` | Hugging Face `ggml-org/Qwen3-ASR-1.7B-GGUF`, hf-mirror, ModelScope. Mac only. |
| `qwen3-asr-0.6b-int8` | `sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2` (878,702,423), then the unpacked ONNX trio and `tokenizer/` | `393f8a14e2f5fb96746aaab342997a40641001fbd5bf9592a080a8329178ee96` | GitHub `k2-fsa/sherpa-onnx` release `asr-models` only. |
| `qwen3-4b-q4` | `Qwen3-4B-Q4_K_M.gguf` (2,497,280,256) | `7485fe6f11af29433bc51cab58009521f205840f5b4ae3a32fa7f92e8534fdf5` | Official `Qwen/Qwen3-4B-GGUF`, plus hf-mirror and ModelScope. |
| `qwen3-1.7b-q4` | `Qwen3-1.7B-Q4_K_M.gguf` (1,107,409,472) | `b139949c5bd74937ad8ed8c8cf3d9ffb1e99c866c823204dc42c0d91fa181897` | `unsloth/Qwen3-1.7B-GGUF`. The official Qwen GGUF repo publishes Q8_0 only. |
| `silero-vad` | `silero_vad.onnx` (643,854) | `9e2449e1087496d8d4caba907f23e0bd3f78d91fa552479bb9c23ac09cbb1fd6` | Same sherpa `asr-models` release. Installed with a speech model. |
| `sensevoice-small-int8` | `model.int8.onnx` (239,233,841), `tokens.txt` (315,894) | `c71f0ce00bec95b07744e116345e33d8cbbe08cef896382cf907bf4b51a2cd51`, `f449eb28dc567533d7fa59be34e2abca8784f771850c78a47fb731a31429a1dc` | Optional. Hugging Face `csukuangfj/sherpa-onnx-sense-voice-zh-en-ja-ko-yue-2024-07-17` and hf-mirror. Not the local default. |
| `llama-cpp-b12292` | macOS `llama-b12292-bin-macos-arm64.tar.gz` (16,870,580) or Windows `llama-b12292-bin-win-cpu-x64.zip` (25,057,882) | `94b860a529a64aecfe20c4b9ccb9ca6fb4cf9126b019f3380ee02d7951ca11ef`, `12017843d67f560ce1bacf771dccdb189ff651a2dde684cbb49634c29df5b751` | GitHub `ggml-org/llama.cpp` tag `b12292`. |

A model is installed only after every download matches its size and SHA-256 and the expected names are on disk. A mismatch deletes the partial file and leaves an error file. Downloads resume from `<name>.partial`. The install marker is `installed.json` in that model directory. Settings trusts the marker plus file sizes; it does not re-hash multi-gigabyte files on every open.

The Windows llama.cpp build is the CPU zip, so a machine without Vulkan or CUDA can still rewrite. macOS uses the Metal build (`-ngl 99`). Windows passes `-ngl 0`. After a Mac download, the app signs `llama-server` and every sibling Mach-O with one identity: the stable self-signed name when that private key is in the keychain, otherwise ad-hoc. Libraries are signed before the executable, with `--timestamp=none`. The private key is not in the app. That signing step has not been exercised on a physical Mac in this change.

## Licenses

Qwen3-ASR and Qwen3 weights are Apache-2.0. sherpa-onnx is Apache-2.0. llama.cpp is MIT. Silero VAD is MIT. The polish prompts keep the ckckck/OpenTypeless MIT attribution in `prompts/ATTRIBUTION.md`.

SenseVoice-Small int8 is optional. It is not selected for a new install. The weights are the sherpa-onnx export of FunAudioLLM SenseVoice-Small (`model.int8.onnx` plus `tokens.txt`) and use the FunASR Model License. The About page names FunASR only after the files are installed and checksummed. Inverse text normalization stays on. ModelScope does not publish this export (checked 2026-11-12); Auto uses Hugging Face, then hf-mirror. The same bytes are inside the k2-fsa release tarball `sherpa-onnx-sense-voice-zh-en-ja-ko-yue-int8-2024-07-17.tar.bz2`.
