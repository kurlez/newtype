# Third-party notices

newtype is dual-licensed under the MIT License or the Apache License, Version 2.0. See [LICENSE-MIT](LICENSE-MIT) and [LICENSE-APACHE](LICENSE-APACHE). Copyright 2026 Wenfeng Liu.

This file summarizes other licenses that apply to dependencies, downloaded models, benchmark samples, and prompts. It is not a substitute for the upstream license texts. Crate licenses were read from `cargo metadata` against `src-tauri/Cargo.lock` on 2026-11-14 (652 dependency packages). npm licenses were counted from `package-lock.json` on the same date.

Where a dependency offers a choice of licenses, newtype uses a permissive option (MIT or Apache-2.0 when that choice exists).

## Rust crates

Most crates are MIT, Apache-2.0, or a dual MIT/Apache-2.0 grant. Counts from the metadata above:

| License expression | Packages |
| --- | ---: |
| MIT OR Apache-2.0 | 301 |
| MIT | 137 |
| Apache-2.0 OR MIT | 57 |
| MIT/Apache-2.0 | 23 |
| Zlib OR Apache-2.0 OR MIT | 18 |
| Unicode-3.0 | 18 |
| Apache-2.0 | 11 |
| Unlicense OR MIT | 10 |
| BSD-3-Clause | 9 |
| MPL-2.0 | 7 |
| other permissive combinations | 61 |

There is no GPL-only or AGPL dependency. `r-efi` is `MIT OR Apache-2.0 OR LGPL-2.1-or-later`; the permissive terms apply.

### MPL-2.0

These crates use the Mozilla Public License 2.0. MPL-2.0 is file-level: the files stay under MPL-2.0, including if you modify them. The rest of newtype stays MIT OR Apache-2.0. The copies in this tree are the unmodified crates.io releases.

| Crate | Versions in the lockfile |
| --- | --- |
| cssparser | 0.37.0, 0.38.0 |
| cssparser-macros | 0.7.1 |
| selectors | 0.38.0, 0.41.0 |
| option-ext | 0.2.0 |
| dtoa-short | 0.3.5 |

### Other licenses worth naming

- **Unicode-3.0.** ICU, tinystr, yoke, zerovec, and related data crates. Permissive, with the Unicode copyright and permission notice.
- **unicode-ident.** `(MIT OR Apache-2.0) AND Unicode-3.0`.
- **CDLA-Permissive-2.0.** `webpki-roots` and `webpki-root-certs`.
- **bzip2-1.0.6.** `libbz2-rs-sys`. The bzip2 license is a permissive BSD-style grant.
- **Apache-2.0 AND ISC.** `ring` 0.17.14. Both apply.
- **BSD-3-Clause.** Includes `zstd` / `zstd-sys`, `bindgen`, `subtle`, and `alloc-stdlib`.
- **BSL-1.0.** `clipboard-win`, `error-code`.
- **ISC.** Includes `rustls-webpki`, `untrusted`, and `libloading`.
- **Zlib.** `zlib-rs`, `foldhash`.

Direct application dependencies include Tauri, tokio, reqwest, cpal, rdev, sherpa-onnx, keyring, and the usual serde/tokio stack. sherpa-onnx 2.0.1 is Apache-2.0 and is linked into the app binary.

## npm packages

`package-lock.json` (lockfileVersion 3):

| License | Packages | Notes |
| --- | ---: | --- |
| MIT | 62 | Includes Vite (dev) |
| Apache-2.0 OR MIT | 13 | Includes `@tauri-apps/api` (runtime) and `@tauri-apps/cli` (dev) |
| Apache-2.0 | 1 | TypeScript (dev) |
| ISC | 1 | picocolors (dev) |
| BSD-3-Clause | 1 | source-map-js (dev) |

The only runtime npm dependency is `@tauri-apps/api`.

## Models and runtimes downloaded after install

Weights are not in the installer. File names, checksums, and download hosts are in [docs/local-models.md](docs/local-models.md).

| Component | License |
| --- | --- |
| Qwen3-ASR weights (1.7B GGUF and 0.6B int8) | Apache-2.0 |
| Qwen3 GGUF rewrite models | Apache-2.0. The 1.7B Q4_K_M file is an Unsloth quant of the official model; the official GGUF repo publishes Q8_0 only. |
| sherpa-onnx | Apache-2.0 |
| Silero VAD | MIT |
| llama.cpp (sidecar build b12292) | MIT |
| SenseVoice-Small int8 (optional, not the default) | FunASR Model License. Attribution to FunAudioLLM / Alibaba is required. The About page names FunASR after the files are installed. |

## Samples and prompts

- The FLEURS clips described in `spikes/qwen3-asr-mac/samples/manifest.json` are CC-BY-4.0 (FLEURS, Google). The manifest has the attribution line. They are benchmark samples, not part of the installed app. Dataset: https://huggingface.co/datasets/google/fleurs
- Cleanup prompts are adapted from [ckckck/OpenTypeless](https://github.com/ckckck/OpenTypeless), which is MIT. Keep that attribution when redistributing the prompts. See [prompts/ATTRIBUTION.md](prompts/ATTRIBUTION.md). No prompt text is taken from Open-Less/openless.

## Names

newtype is not affiliated with or endorsed by Typeless. Names of models, datasets, and cloud services belong to their owners.
