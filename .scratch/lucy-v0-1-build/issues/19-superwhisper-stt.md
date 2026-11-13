# 19: Superwhisper STT Provider

**What to build:** A Superwhisper user can use it for speech-to-text. An STT Provider of kind `superwhisper`, offered only when it is installed, writes each utterance as 16 kHz mono, calls `superwhisper transcribe FILE` and polls `read`. It never uses `record`/`stop`.

Spec: Providers and configuration (Superwhisper).

**Blocked by:** 05, 12, 11

**Seams:** owns the `[stt]` config section choosing `whisper` or `superwhisper`, and the Superwhisper warnings the TUI reads (also used by 20); uses the failure result from `Stt::transcribe` (owned by 11)

**Status:** resolved

- [ ] Selectable in config; absent from the options when Superwhisper isn't installed
- [ ] Failures follow the STT error path from 11
- [ ] The latency, cleanup-model and history warnings are available for the TUI (20)

## Comments

Resolved on `lucy-v0-1-slice-4` (80a2620..5e4e5a8). `[stt] kind` in lucy-config (`SttKind::options`, `SttKind::warnings` for 20); adapter in `crates/lucyd/src/superwhisper.rs`. Uses `raw` before `text`. How a failed transcription appears in `read` is assumed (an `error` field, plus a 10 s timeout); not checked against the real service. Utterance WAVs go in `$XDG_RUNTIME_DIR/lucy`. Switching back to Whisper after a start without its model needs a lucyd restart. Combination test with Codex: `superwhisper_hears_a_session_on_codex_and_its_reply_is_spoken`.
