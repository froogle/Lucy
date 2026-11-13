# 05: Mic capture, VAD and Whisper STT

**What to build:** Lucy hears the user. cpal captures the mic (PipeWire → PulseAudio → ALSA) at 16 kHz mono f32, behind the `AudioIo` seam. Callbacks push into lock-free rings, and one DSP thread reframes to 10 ms. Silero VAD (behind a `Vad` trait) finds the end of each utterance. whisper-rs (Vulkan, resident model; `small.en` on GPU, `base.en` on CPU) transcribes it into the Session. `lucy listen` followed by speaking produces a real transcript.

Spec: Audio pipeline; Providers (`whisper`).

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] Capture behind `AudioIo`; mic device is `"default"` or a pinned node name from config
- [ ] Single DSP thread reframes to 10 ms and fans out (AEC slot left empty for 09)
- [ ] Silero v6.2 through `ort` on tract; endpointing at 0.5/0.35 thresholds, 100 ms to start, 500 ms pre-roll, 700 ms silence to end, 30 s cap, all from config
- [ ] Whisper model loaded once at startup; GPU/CPU choice and `audio_ctx` handled
- [ ] STT is a Provider kind `whisper` behind the STT trait the harness fakes
- [ ] Manual check: speak a short command after `lucy listen` and see it reach the Thinking fake or Claude
