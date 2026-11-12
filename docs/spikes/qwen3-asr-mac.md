# M-S1: Qwen3-ASR-1.7B on Apple Silicon

- Date: 2026-11-12
- Status: spike recommendation. The 16 GB Mac gate is not closed by the GitHub Actions run.
- Product context: PRD v0.2 milestone M-S1. Mac tries Qwen3-ASR-1.7B; if that fails the gate, that Mac uses Qwen3-ASR-0.6B int8 via sherpa-onnx. Windows v1 stays on 0.6B int8.

## Recommendation

**GO for Qwen3-ASR-1.7B on Mac, shipped as a llama.cpp Metal sidecar. Do not ship it as a bundled Python/MLX runtime.**

**NO-GO for the MLX plan as the way the app runs 1.7B.** MLX is fast enough in a published measurement, and it is the wrong runtime to embed. It needs a second, pinned Python stack. The PRD already treats that as a fragile dependency. The OpenLess TextStream package cited in the PRD is also not 1.7B: its manifest downloads `Qwen/Qwen3-ASR-0.6B`.

**0.6B int8 via sherpa-onnx stays the fallback and the Windows v1 engine.** That path is real on both macOS arm64 and Windows x64. There is still no official sherpa-onnx 1.7B release.

This is not a finished measurement on Wenfeng's 16 GB Mac. The latency and memory evidence that clears the gate is a third-party run on an M5 with 32 GB, quoted below and labeled as such. The workflow in this repo runs on a GitHub-hosted M1 with 7 GB of RAM. Those two machines are not interchangeable. If the 16 GB machine later takes more than about 4 seconds for a normal utterance, or starts swapping, that machine drops to 0.6B int8. Nothing in this spike changes Windows.

## Gate

From PRD M-S1, applied to a typical utterance of up to about 20 seconds, model already loaded:

| Result | Condition |
|---|---|
| GO | Usually under about 2 seconds, and memory is comfortable on a 16 GB Mac (no obvious swap). Packable into the app. |
| NO-GO | Over about 4 seconds, swapping, a crash on load, or a runtime that is too fragile to ship. Fall back to 0.6B int8. |

v1 still transcribes after the user stops talking. Streaming is not part of this gate.

## What the survey found

The PRD assumed the Mac 1.7B path was MLX, and assumed sherpa-onnx had no 1.7B package. Both need an update.

- llama.cpp merged Qwen3-ASR support on 2026-04-12 (`ggml-org/llama.cpp` PR 19441) and publishes GGUF pairs at `ggml-org/Qwen3-ASR-1.7B-GGUF` and `ggml-org/Qwen3-ASR-0.6B-GGUF`. The 1.7B card's own run command is `llama-server -hf ggml-org/Qwen3-ASR-1.7B-GGUF`. Prebuilt macOS arm64 binaries are in the `b*` releases (Metal). This spike pins `b12292` (2026-11-11).
- sherpa-onnx 2.0.1 (2026-10-26) runs Qwen3-ASR through the C API and the Rust crate, on macOS arm64 and Windows x64. The only official packaged weights are `sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25`. The docs point at a 1.7B tree on ModelScope. The maintainer declined a GitHub release of 1.7B because GitHub's file limit is 2 GB (`k2-fsa/sherpa-onnx` issue 3535, 2026-04-22). A later comment on that issue describes a mel-frontend mismatch that hits 1.7B harder than 0.6B. Do not treat a community 1.7B ONNX file as the Mac engine.
- OpenLess TextStream is the MLX design the PRD asked us to verify. `experimental-textstream-manifest.json` on `Open-Less/openless` lists `Qwen/Qwen3-ASR-0.6B` at revision `5eb144179a02acc5e5ba31e748d22b0cf3e303b0`, `model.safetensors` = 1,876,091,704 bytes (1.75 GB). The runtime pin in their docs is Python 3.12.15, MLX 0.32.3, `mlx-qwen3-asr` 0.4.4. The "about 1.9 GB" figure is this 0.6B checkpoint, not 1.7B. Their docs also say the worker does not open the microphone. That packaging lesson is worth keeping. The weights are the wrong size for M-S1.
- Community MLX weights of the actual 1.7B exist (`mlx-community/Qwen3-ASR-1.7B-8bit` and the 4-bit, 6-bit, and bf16 repos). `mlx-qwen3-asr` and `mlx-audio` can load them. That is a working prototype path, not the one to bundle.

## Runtime survey

Sizes below were read from the Hugging Face or GitHub API on 2026-11-12, unless the cell says the figure comes from the project's own card.

| Runtime | Maturity for 1.7B on macOS arm64 | License | What you ship besides weights | Python? | Streaming | How it would sit in Tauri |
|---|---|---|---|---|---|---|
| **llama.cpp Metal, official GGUF** | Upstream since 2026-04-12. Audio is still logged as experimental (`llama.cpp` discussion 13759, quoted from server logs in discussion 21906). A shipping Mac app (asist) has used `llama-server` for this model. | Code MIT. Weights Apache-2.0, inherited from `Qwen/Qwen3-ASR-1.7B`. | Pinned `llama-*-bin-macos-arm64.tar.gz`. `b12292` is 16,870,580 bytes. Unpack and keep `llama-server` plus the dylibs beside it. | No | No incremental ASR API. Partials mean re-sending a window of audio. v1 does not need this. | **Sidecar.** Loopback HTTP, `POST /v1/audio/transcriptions`. Crash stays in the child. Same binary family as the planned GGUF polish model, in a second process, not the same one. |
| **mlx-qwen3-asr** (what OpenLess pins) | Active Apache-2.0 port. Supports 0.6B and 1.7B, long audio, and a windowed streaming recipe. OpenLess pins 0.4.4 against the **0.6B** weights. | Code Apache-2.0. Weights Apache-2.0. | A locked CPython + MLX venv. OpenLess uses uv and hash-locked wheels, and keeps them out of system Python. | Yes | Windowed re-decode, closer to the official streaming recipe than llama.cpp is. Still P2. | Sidecar. This is the least bad MLX packaging, and it is still a second runtime to version, sign, and debug. |
| **mlx-audio** (Python) | Community conversion. `mlx-community/Qwen3-ASR-1.7B-8bit` was converted with mlx-audio 0.3.1. 8-bit file is 2,463,307,541 bytes. 4-bit is 1,603,081,617. bf16 is 4,076,186,653. | Code MIT. Weights Apache-2.0. | `pip install mlx-audio` plus MLX. Apple Silicon only. | Yes | Token streaming of a buffer in the library. Not the v1 path. | Sidecar. Same Python objection. |
| **mlx-audio-swift** | Same MLX models, no Python. Token streaming and an incremental session exist. The package is MIT and tracks `main` more than a stable version. | MIT, plus mlx-swift and swift-transformers. | A Swift dylib or helper built with Xcode, then a C ABI in front of it. The MLX stack is large. | No | Incremental session is documented. P2. | FFI into a Swift helper. Possible, and a worse build than llama.cpp for an app that already wants llama.cpp for the polish model. |
| **sherpa-onnx 1.7B** | The C API can load the architecture. An official 1.7B weight file is not in the `asr-models` release. Community exports exist (ModelScope `zengshuishui/Qwen3-ASR-onnx` `model_1.7B`, Hugging Face `thieunv-asilla/sherpa-onnx-qwen3-asr-1.7B-int8`, a 1.97 GB zip on BiBi-Keyboard). Issue 3535 records a frontend bug that 1.7B is sensitive to. CPU ONNX for this decoder is the slow path: CapsWriter's different ONNX+GGUF stack took 4.0 s for 20 s of audio on a U9-285H, which is already the no-go side of the gate. | Runtime Apache-2.0. Weights Apache-2.0 if the export says so. `Wasser1462/Qwen3-ASR-onnx` and `HaujetZhao/Qwen3-ASR-GGUF` have no GitHub license file. | Same sherpa library as 0.6B, plus a ~2 GB ONNX decoder. | No, once the ONNX exists. | Offline model. sherpa's own Qwen3 page shows VAD plus offline decode, which is simulated streaming. | In-process, same as 0.6B. Do not implement this until an official file exists and a Mac timing beats 2 s. |
| **Core ML** | Community only. `weiren119/Qwen3-ASR-1.7B-CoreML` is a split encoder/decoder: the card says encoder ~304 MB, decoder ~2.8 GB, embeddings ~594 MB, decoder forced to FP32 on the GPU so it does not run on the Neural Engine, macOS 14+. `ax0t/Qwen3-ASR-1.7B-CoreML-INT8` is the encoder only; the decoder stays MLX. You still write the mel frontend, chunking, and KV cache. | Weights Apache-2.0. Conversion code is third-party. | Core ML runtime is in the OS. The glue is ours. | Not at runtime. | Not provided. | Too much unowned inference code for v1. |
| **Official `qwen-asr` (transformers / vLLM)** | The reference. Streaming is documented as vLLM-only. vLLM wants CUDA. | Apache-2.0. | A CUDA server, not a Mac app. | Yes | Yes, on vLLM. | Not a local Mac runtime. |
| **CapsWriter ONNX encoder + GGUF decoder** | Windows desktop stack. Vulkan and DirectML. Their own table: 20 s of audio in 4.0 s on a U9-285H CPU and 1.0 s on an RTX 5050. | Repo license file was not present on GitHub at survey time. Weights Apache-2.0. | Python, ONNX Runtime, and a llama.cpp DLL. Release zips: q4_k 1,345 MB, q5_k 1,861 MB. | Yes | They describe streaming file transcription. | Sidecar, and a Windows-shaped one. Not the Mac default. Useful later if Windows ever leaves 0.6B. |

The official GGUF pair for 1.7B Q8_0 is:

- `Qwen3-ASR-1.7B-Q8_0.gguf` — 2,165,034,944 bytes
- `mmproj-Qwen3-ASR-1.7B-Q8_0.gguf` — 355,709,344 bytes

Together 2.35 GiB. The bf16 pair is about twice that and is the wrong download for a 16 GB machine. A decoder without its mmproj starts and then silently ignores audio, so the app has to treat the pair as one artifact.

## Published timings (not this repo)

These are other people's measurements. They are not Chinese-dictation numbers from a 16 GB Mac, and they are not ours.

[asist issue 79](https://github.com/nyosegawa/asist/issues/79), measured 2026-09-29 by the maintainer, llama.cpp `b11246`, `ggml-org` Q8_0, `llama-server`, language forced by prefilling `language Japanese<asr_text>`. The clips were 22 synthetic macOS-voice utterances plus 13 recordings of the maintainer. Hardware: **Mac M5, 32 GB**.

| Setup | 17 s utterance | 3 s utterance | Memory |
|---|---|---|---|
| 1.7B MLX 8-bit | 1.36 s | 0.25–0.42 s | 3.2–3.8 GB |
| 1.7B llama.cpp Metal | 1.40 s | 0.22–0.38 s | 3.4 GB |
| 0.6B MLX bf16 | 0.89 s | 0.15 s | 2.2–2.7 GB |
| 0.6B llama.cpp Metal | 0.58 s | 0.10 s | 1.8 GB |

On that machine both 1.7B runtimes are inside the 2 second gate, and 3.4 GB is a comfortable ASR footprint on 16 GB **by itself**. llama.cpp matched MLX closely enough that the extra Python stack does not buy speed. The same issue records an RTX 2080 at 0.60–0.68 s for the 17 s clip with llama.cpp CUDA/Vulkan. That is evidence for a later Windows 1.7B experiment. It is not a reason to change the v1 Windows default.

CapsWriter's 4.0 s CPU figure (20 s audio, U9-285H, their ONNX+GGUF engine) is the warning against running the 1.7B decoder on CPU.

## sherpa-onnx 0.6B int8

Confirmed on 2026-11-12 against sherpa-onnx **2.0.1** (published 2026-10-26) and the `asr-models` release.

Weights, one file for both desktops:

`https://github.com/k2-fsa/sherpa-onnx/releases/download/asr-models/sherpa-onnx-qwen3-asr-0.6B-int8-2026-03-25.tar.bz2`

878,702,423 bytes. The sherpa docs list the unpacked pieces as about 42 MB `conv_frontend.onnx`, 174 MB `encoder.int8.onnx`, 721 MB `decoder.int8.onnx`, plus a `tokenizer/` directory. fp32 0.6B and a 1.7B tree are documented on ModelScope (`zengshuishui/Qwen3-ASR-onnx`), not as GitHub release assets.

Prebuilt tools that contain the offline binary:

- macOS arm64: `https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/sherpa-onnx-v2.0.1-osx-arm64-shared.tar.bz2` (20,348,241 bytes). Archive listing includes `bin/sherpa-onnx-offline`.
- Windows x64: `https://github.com/k2-fsa/sherpa-onnx/releases/download/v2.0.1/sherpa-onnx-v2.0.1-win-x64-shared-MD-Release.tar.bz2` (21,921,136 bytes). Archive listing includes `bin/sherpa-onnx-offline.exe`.

The app should not shell out to those CLIs. Link the library.

Rust: the `sherpa-onnx` crate **2.0.1** downloads the matching static library at build time (macOS arm64 `sherpa-onnx-v2.0.1-osx-arm64-static-lib.tar.bz2`, Windows x64 `sherpa-onnx-v2.0.1-win-x64-static-MT-Release-lib.tar.bz2`). The example is `rust-api-examples/examples/qwen3_asr.rs`. The config struct is `OfflineQwen3ASRModelConfig` (`conv_frontend`, `encoder`, `decoder`, `tokenizer`, plus `max_total_len`, `max_new_tokens`, `temperature`, `top_p`, `seed`, `hotwords`).

The C struct in `sherpa-onnx/c-api/c-api.h` at tag `v2.0.1` is `SherpaOnnxOfflineQwen3ASRModelConfig`, nested on `SherpaOnnxOfflineModelConfig` as `qwen3_asr`. Same fields. A hotword string is comma-separated UTF-8. That is model-side context, not a substitute for the product's phoneme/regex hotwords, which still run after ASR.

CLI shape, from the sherpa Qwen3 docs:

```text
sherpa-onnx-offline \
  --qwen3-asr-conv-frontend=.../conv_frontend.onnx \
  --qwen3-asr-encoder=.../encoder.int8.onnx \
  --qwen3-asr-decoder=.../decoder.int8.onnx \
  --qwen3-asr-tokenizer=.../tokenizer \
  --num-threads=4 \
  utterance.wav
```

License: sherpa-onnx is Apache-2.0. The Qwen3-ASR weights are Apache-2.0. Name both on the about page.

## This repo's benchmark

`spikes/qwen3-asr-mac/bench.py` is not linked into the Tauri binary. `.github/workflows/build.yml` is unchanged. `.github/workflows/qwen3-asr-mac-bench.yml` is `workflow_dispatch` only, plus one push of the bench script on the spike branch so this run can happen before the file exists on `main`. GitHub shows the Run workflow button after the file is on the default branch. It runs on `macos-15`.

`macos-14` was retired on 2026-11-02. As of the GitHub-hosted runner table read on 2026-11-12, `macos-15` / `macos-26` / `macos-latest` for arm64 are an **M1, 3 vCPU, 7 GB RAM, 14 GB SSD**. That is a different computer from a 16 GB Mac, and 7 GB is tight for a 2.35 GiB model plus the OS. An out-of-memory result there is a fact about the runner. It is not, by itself, a no-go for 16 GB.

The script pins llama.cpp `b12292` and sherpa-onnx `2.0.1`. For 1.7B it serves `llama-server` with `-c 2048`, `-ngl 99`, `--cache-ram 0` (the default prompt cache is 8192 MiB and would not fit), `--no-warmup`, and `--reasoning off`. It times two warm HTTP transcriptions after one discarded warmup. For 0.6B it prefers the `sherpa-onnx` 2.0.1 Python wheel so the model stays loaded; the wheel is a benchmark harness, not a product dependency. If the wheel does not install, it times `sherpa-onnx-offline`, and those times include the load.

Clips are four FLEURS `cmn_hans_cn` validation utterances, CC-BY-4.0, converted to 16 kHz mono PCM16. Attribution is in `spikes/qwen3-asr-mac/samples/manifest.json`. FLEURS is read speech, not a meeting. The two "zh-en" clips are Mandarin sentences that contain an English token (`WiFi`, `Gordon Johndroe`), not spontaneous code-switching.

| File | Duration | Reference |
|---|---|---|
| `zh-short.wav` | 3.66 s | 我觉得这毫无道理；显然不公平。 |
| `zh-en-short.wav` | 3.30 s | 他称，他制作了一个 WiFi 门铃。 |
| `zh-long.wav` | 18.38 s | 报告警告称，没有人能保证目前在伊拉克采取的任何行动能够阻止宗派战争、不断增长的暴力或走向混乱。 |
| `zh-en-long.wav` | 18.04 s | 布什政府发言人戈登·约翰德罗 (Gordon Johndroe) 称朝鲜的承诺是朝着实现朝鲜半岛可核查的无核化目标迈出的重要一步。 |

`python3 spikes/qwen3-asr-mac/bench.py --self-test` checks those durations on any OS. The timed run refuses to start unless the machine is macOS arm64.

### Numbers from the Actions run

Filled from the workflow artifact when that run finishes. Until that paragraph exists, this document has **no** first-party latency, RTF, RSS, or transcript numbers. Do not copy the M5 table above into a hardware recommendation as if we had measured it.

## Integration

Keep the provider split the PRD already sketched. Retarget the Mac 1.7B variant. The stub in `src-tauri/src/pipeline/asr.rs` still says `LocalQwen3Mlx`; leave that rename to the implementation change so this spike does not touch the release binary.

```text
macOS, local path:
  1.7B weights present and this machine passed the 16 GB check
      -> llama-server sidecar
  otherwise
      -> sherpa-onnx 0.6B int8, in process

Windows, local path:
  sherpa-onnx 0.6B int8, in process
```

Cloud ASR is untouched. Local engines are used for manual local, offline fallback, and strict offline, which is when the PRD already says to load weights.

### Sidecar, not an in-process 1.7B

llama.cpp's stable app surface for audio is `llama-server`, not a small C function we should bind ourselves in v1. A sidecar matches the crash isolation the PRD asked of the MLX worker: if the model process dies, the UI, hotkey, and the recording survive. Bind `127.0.0.1` and an ephemeral port. Do not listen on all interfaces.

Start the process on the first local transcription, not at app launch and not when Settings opens. The PRD's idle timeout (default 5 minutes) kills it. The polish model, when it is also llama.cpp, is a **second** process with its own port and its own GGUF. One server cannot hold the ASR pair and the chat model at once. If starting the second process would push the machine into swap, skip polish and insert the rule-and-hotword text. That fallback already exists in the PRD.

Request: `POST /v1/audio/transcriptions` with the wav and `temperature=0`. llama.cpp sometimes returns a prefix `language Chinese<asr_text>` before the words. Strip that prefix before the text hits the hotword layer. The bench script's `split_asr_text` is the behavior to keep.

Flags that matter, from `llama-server --help` on `b12292`:

- `-m` and `--mmproj` together
- `-c 2048` so the KV cache stays small for a single utterance (the model's own context field is 65536)
- `-ngl 99` so Metal takes the layers
- `--cache-ram 0` so the prompt cache does not reserve 8 GB
- `--reasoning off`
- `--no-warmup` if an empty warmup run is unhappy on an audio model

Pin the llama.cpp tag. Record the SHA-256 of the archive and of both GGUF files. A newer nightly is not automatically better; audio support is still marked experimental.

### Where files live

Weights are not in the `.dmg` or the NSIS installer.

| Piece | Where | When it is fetched |
|---|---|---|
| `llama-server` and its dylibs | Inside the Mac app bundle, next to the executable | Build time, from the pinned release. About 17 MB compressed. Ad-hoc sign it with the rest of the app so Gatekeeper sees one tree. |
| 1.7B Q8_0 decoder + mmproj | `~/Library/Application Support/newtype/models/qwen3-asr-1.7b-q8_0/` | User download. Both files, one revision, SHA-256 checked before the directory is marked installed. |
| sherpa 0.6B int8 | `.../models/qwen3-asr-0.6b-int8/` on both desktops | User download. The sherpa **code** is linked into the binary, so a machine with weights and no network can still run 0.6B. |
| Python / MLX | Nowhere | Not a product dependency. |

Mirrors for the sherpa archive: the GitHub release above, plus a copy on Hugging Face or ModelScope if we publish one. The GGUF pair lives on Hugging Face (`ggml-org/Qwen3-ASR-1.7B-GGUF`). hf-mirror is the right second URL for that pair. Do not invent a ModelScope URL for the GGUF until one is verified. Download failure deletes the partial file and does not mark the model installed. Opening Settings reads the directory and does not load weights and does not start `llama-server`.

The model card should show: 1.7B Q8_0, Apache-2.0, Mac only, llama.cpp Metal, about 2.4 GB on disk, about 3.5 GB resident once loaded (the M5 figure, until we measure 16 GB), not streaming. The 0.6B card says sherpa-onnx, both desktops, about 0.9 GB on disk. On a Mac that has not passed the 16 GB check, the 0.6B card is the recommended fallback and the 1.7B card is optional.

### Provider seam

`AsrProvider::transcribe` stays the boundary in `src-tauri/src/pipeline/asr.rs`. The router does not know about ports or ONNX filenames.

- `LocalQwen3Llama` (name to use when the stub is renamed): writes the utterance to a temp wav, posts it to the sidecar, strips the language prefix, returns `Transcript`. If the process is not up, it starts it and the Voice bar already has a "loading the local model" state for that wait. If the process fails and 0.6B weights are installed, the same audio goes to sherpa for that utterance and the bar says so.
- `LocalQwen3Sherpa`: builds `OfflineRecognizer` once, keeps it until the idle timeout, decodes 16 kHz mono f32. Used on Windows always, and on Mac when 1.7B is absent, failed, or was marked no-go for that machine.
- Hotwords in the sherpa config are optional extra context. The product hotword pass still runs on the transcript afterward, on every provider, including cloud.

Only one local ASR is loaded. The sidecar and the sherpa recognizer are not both resident. Switching models drops the other.

### What we are not doing in this spike

- No change to `build.yml`, the updater, or the installer contents.
- No Python in the app.
- No sherpa 1.7B integration.
- No claim that a 7 GB CI runner is the go/no-go machine.
- No CER on a public test set. Four FLEURS lines are a smoke test. The PRD's "CER not worse than 0.6B on the same machine" still wants a longer set on the 16 GB Mac. The bench stores the reference text next to the hypothesis so that comparison can be computed when the run exists.

## Sources

- Qwen3-ASR model card and Apache-2.0 weights: https://huggingface.co/Qwen/Qwen3-ASR-1.7B and https://github.com/QwenLM/Qwen3-ASR
- llama.cpp Qwen3-ASR support: https://github.com/ggml-org/llama.cpp/pull/19441 (merged 2026-04-12)
- GGUF repos: https://huggingface.co/ggml-org/Qwen3-ASR-1.7B-GGUF and https://huggingface.co/ggml-org/Qwen3-ASR-0.6B-GGUF
- llama.cpp audio still experimental: https://github.com/ggml-org/llama.cpp/discussions/13759 and the server log quoted in https://github.com/ggml-org/llama.cpp/discussions/21906
- Third-party M5 / RTX timings: https://github.com/nyosegawa/asist/issues/79
- sherpa-onnx Qwen3 docs: https://k2-fsa.github.io/sherpa/onnx/qwen3-asr/index.html and https://k2-fsa.github.io/sherpa/onnx/qwen3-asr/pretrained.html
- sherpa 2.0.1 release and Rust crate: https://github.com/k2-fsa/sherpa-onnx/releases/tag/v2.0.1 and https://docs.rs/sherpa-onnx/2.0.1
- Rust example: https://github.com/k2-fsa/sherpa-onnx/blob/v2.0.1/rust-api-examples/examples/qwen3_asr.rs
- 1.7B sherpa packaging refusal and frontend note: https://github.com/k2-fsa/sherpa-onnx/issues/3535
- MLX Python: https://github.com/Blaizzy/mlx-audio and https://huggingface.co/mlx-community/Qwen3-ASR-1.7B-8bit
- MLX Swift: https://github.com/Blaizzy/mlx-audio-swift
- mlx-qwen3-asr: https://github.com/moona3k/mlx-qwen3-asr
- OpenLess TextStream manifest (0.6B weights, not copied): `openless-all/app/crates/openless-core/src/experimental-textstream-manifest.json` in https://github.com/Open-Less/openless
- OpenLess runtime pin: https://github.com/Open-Less/openless/blob/master/openless-all/app/docs/experimental-local-asr.md
- Core ML community cards: https://huggingface.co/weiren119/Qwen3-ASR-1.7B-CoreML and https://huggingface.co/ax0t/Qwen3-ASR-1.7B-CoreML-INT8
- CapsWriter timings and zips: https://github.com/HaujetZhao/CapsWriter-Offline/blob/master/docs/%E6%A8%A1%E5%9E%8B%E4%B8%8B%E8%BD%BD%E7%9A%84%E8%8B%A5%E5%B9%B2%E9%97%AE%E9%A2%98.md and https://github.com/HaujetZhao/Qwen3-ASR-GGUF
- Runner shape: https://docs.github.com/en/actions/reference/runners/github-hosted-runners and the macOS 14 retirement note https://github.com/actions/runner-images/issues/13518
- Clips: FLEURS, CC-BY-4.0, https://huggingface.co/datasets/google/fleurs
