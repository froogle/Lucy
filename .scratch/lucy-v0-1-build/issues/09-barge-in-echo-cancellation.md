# 09: Barge-in with echo cancellation

**What to build:** The user can cut Lucy off by talking. In-process WebRTC AEC3, behind an `EchoCanceller` seam, cancels Lucy's own voice using the playback reference. While she speaks, sustained speech that passes the VAD gate ducks TTS on the first frame and, once the gate is met, stops and flushes it. If the model is still generating, Lucy sends the stream-json control interrupt (feature-detected, with SIGINT plus respawn as the fallback). The next utterance goes to the model prefixed with what the user heard. The Wake word during Speaking interrupts instantly.

Spec: Session state machine (Barge-in); Audio pipeline (AEC, VAD gate).

**Blocked by:** 04, 05, 06, 07, 11, 14, 18

**Seams:** uses `WakeWord`, its harness fake and `session::Input::WakeWord` (owned by 07); uses the dead-process `ThinkingEvent` and its `FakeThinking` helper (owned by 11); uses the Confirming step in `session::Session` (owned by 14); uses `SessionModel::interrupt` (owned by 18), adding Claude's implementation; extends `Speaker` and `SpeakerEvent` (owned by 06, resolved)

**Status:** ready-for-agent

- [ ] Gate: p ≥ 0.7 for 300 ms, or p ≥ 0.85 for 500 ms while AEC is unconverged (ERLE < 10 dB or first 2 s); window covers TTS plus a 300 ms tail
- [ ] `barge_in = "speech" | "wake_word"` (default `speech`); on PulseAudio or ALSA, Barge-in is off unless forced, with a warning
- [ ] Next message is prefixed `[You were interrupted; the user heard up to: "…"]`
- [ ] Empty transcript after a Barge-in returns silently to Listening
- [ ] Speech during Thinking is a Barge-in
- [ ] Unconfirmed in-flight tool calls get an MCP cancellation; confirmed ones run to completion and are reported in the next Turn
- [ ] Interrupt feature detection falls back to SIGINT plus respawn (harness test with the fake)
- [ ] PipeWire echo-cancel module is supported as an opt-in (in-process AEC disabled)
