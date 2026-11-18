# 53: Kokoro as a second TTS Provider

**What to build:** Piper's voices sound robotic. Add Kokoro (Kokoro-82M, Apache 2.0, runs locally) as a second TTS Provider that the user can pick. Piper stays the default. Kokoro is never installed or downloaded unless the user picks it in the voice section of `lucy setup`. Once picked, it is used for Sessions and Voice previews just as Piper is now.

Spec: none; raised by the user on 2026-11-18 because cori sounds robotic.

**Blocked by:** none

**Seams:** extends the `Tts` adapter (`crates/lucyd/src/adapters.rs`) with a second implementation; adds a TTS Provider choice to the config (`crates/lucy-config`) and the `Key::Voice` setting.

**Status:** needs-triage

**Notes:**

- lucyd builds Piper directly in `main.rs` (`lucyd::piper::Piper::start`). It also gets the echo canceller's playback reference from Piper (`take_reference`). Kokoro has to supply the same 16 kHz reference, or the echo canceller (ADR 0009) stops working while Kokoro speaks. Selecting between Kokoro and Piper at startup could follow the `by_kind::ByKind` pattern used for the Thinking Providers.
- Licence: Kokoro can fall back to espeak-ng for pronunciation, and espeak-ng is GPL. It has to run out of process, as Piper does (ADR 0003).
- The voice section (`crates/lucy/src/sections/voice.rs`) needs to offer the Provider first, then that Provider's voices. Picking Kokoro asks before installing or downloading anything, the same way the Piper install does now, and says roughly how big the download is.
- Config: `[piper] voice` stays as it is. The Provider choice and Kokoro's voice need a new key or section. Decide which in triage.
- Open questions for triage:
  - How Kokoro runs: a supervised Python server, like Piper (e.g. the `kokoro-onnx` package behind a small HTTP shim), or in-process ONNX through `ort`. Which one we pick drives the install plan, the licence and how restarts work.
  - Which Kokoro voices to list, and which is recommended.
  - Is Kokoro's latency to first sound acceptable on CPU? Measure it against `en_GB-cori-medium` before committing.

**Acceptance:**

- [ ] With no config change, Lucy still speaks in Piper and nothing Kokoro-related is downloaded
- [ ] In the setup voice section the user can pick Kokoro. Lucy asks before installing it, fetches it, says the sample line in it, and saves the choice
- [ ] After a restart, lucyd speaks in Kokoro. If Kokoro fails, replies go out as notification text and the Session carries on, as with Piper (harness test)
- [ ] The echo canceller gets a playback reference while Kokoro speaks (test), and Barge-in works with Kokoro by hand
- [ ] A test shows the Provider choice parses from config and defaults to Piper
- [ ] Switching back to Piper in setup works

## Comments
