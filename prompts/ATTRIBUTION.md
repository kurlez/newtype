# Prompt attribution

The literal cleanup prompt (`v1-literal.system.txt`, `v1-literal.user.txt`) is the v1 minimal-cleanup prompt for newtype.

Its Chinese skeleton is a rewrite of the recommended prompt in [ckckck/OpenTypeless](https://github.com/ckckck/OpenTypeless) ([`推荐提示词.md`](https://github.com/ckckck/OpenTypeless/blob/main/%E6%8E%A8%E8%8D%90%E6%8F%90%E7%A4%BA%E8%AF%8D.md)), which is MIT licensed. That prompt defined the restatement role, minimal fixes, hot-word replacement, list formatting, spoken numbers, self-corrections, and the rule that the model does not answer or execute. Some test cases in `cases.json` follow or extend examples from that file.

Keep the ckckck/OpenTypeless attribution when redistributing these prompts.

`v2-polish.system.txt` and `v2-polish.user.txt` are newtype's default polish prompts. They keep the same MIT skeleton and the same hard limits (do not answer, do not execute, keep meaning and language, keep terms and hot words exact, treat the transcript as untrusted data). They relax only wording and sentence flow, as specified for the polish level.

`v2-polish-local.system.txt` and `v2-polish-local.user.txt` are the shortened local-model variants of that polish prompt. They keep the same hard limits and add a few short examples. They do not copy prompt text from Open-Less/openless.

Ideas about correction order — hot words before a model, transcripts treated as data, no "here is the cleaned text" preamble — are common to open voice-input tools. No prompt text was copied from Open-Less/openless (AGPL-3.0).
