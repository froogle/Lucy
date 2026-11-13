# 19: Superwhisper STT Provider

**What to build:** A Superwhisper user can use it for speech-to-text. An STT Provider of kind `superwhisper`, offered only when it is installed, writes each utterance as 16 kHz mono, calls `superwhisper transcribe FILE` and polls `read`. It never uses `record`/`stop`.

Spec: Providers and configuration (Superwhisper).

**Blocked by:** 05, 12, 11

**Seams:** owns the `[stt]` config section choosing `whisper` or `superwhisper`, and the Superwhisper warnings the TUI reads (also used by 20); uses the failure result from `Stt::transcribe` (owned by 11)

**Status:** ready-for-agent

- [ ] Selectable in config; absent from the options when Superwhisper isn't installed
- [ ] Failures follow the STT error path from 11
- [ ] The latency, cleanup-model and history warnings are available for the TUI (20)
