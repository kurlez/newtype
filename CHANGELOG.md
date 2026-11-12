# Changelog

- 2026-11-12: Translate and command hotkeys record. Translate inserts text in the target language (English by default). Command rewrites a selection or inserts generated text. The dictionary imports and exports JSON, and history can be searched, inserted again, cleared, and kept for a chosen period.
- 2026-11-12: Auto routing falls back to local models when the cloud provider is unreachable, retries the same recording if a cloud call fails, and strict offline blocks outbound pipeline traffic.
- 2026-11-12: Local dictation can use Qwen3-ASR and a Qwen3 rewrite model. Weights download on demand and are checksummed before they count as installed.
- 2026-11-12: API keys are stored in the OS credential store, and dictation errors open the indicator.
- 2026-11-11: Test build to verify in-app auto-update (dev.5).
