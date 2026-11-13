# 07: Wake word opens a Session

**What to build:** Saying "Hey Lucy" opens a Session hands-free. `livekit-wakeword` v2 (pinned git rev) runs continuously on the DSP output. The model path comes from config. Until the real models land (wayfinder 27), an interim model is used. On detection Lucy plays a start sound and enters Listening, and the audio straight after the Wake word is kept. If no speech follows, the Session ends after 8 s. Inside a Session, a leading wake phrase is stripped from the transcript. A silence-timeout end plays a soft closing sound.

Spec: Session state machine (Start, Wake word inside a Session); Audio pipeline (Wake word).

**Blocked by:** 05

**Seams:** owns the `WakeWord` adapter trait and its `Adapters` field, the harness's scripted-detection fake, `session::Input::WakeWord` and the `[wake_word]` config section (also used by 09, 20); extends `Speaker` (owned by 06, resolved)

**Status:** ready-for-agent

- [ ] Wake-word engine is fixed (not a Provider); the `.onnx` path can be swapped in config
- [ ] "Hey Lucy what's the time" in one breath produces the transcript "what's the time"
- [ ] No speech after the Wake word ends the Session after 8 s (harness test with scripted detection)
- [ ] Start and closing sounds play
- [ ] Wake word while Listening is treated as ordinary speech; the `Speaking` instant-interrupt hook is left for 09
