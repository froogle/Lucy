# 53: Kokoro as a second TTS Provider

**What to build:** Piper's voices sound robotic. Add Kokoro (Kokoro-82M, Apache 2.0, runs locally) as a second TTS Provider that the user can pick. Piper stays the default. Kokoro is never installed or downloaded unless the user picks it in the voice section of `lucy setup`. Once picked, it is used for Sessions and Voice previews just as Piper is now.

Spec: none; raised by the user on 2026-11-18 because cori sounds robotic.

**Blocked by:** none

**Seams:** extends the `Tts` adapter (`crates/lucyd/src/adapters.rs`) with a second implementation; adds a TTS Provider choice to the config (`crates/lucy-config`) and the `Key::Voice` setting.

**Status:** ready-for-human

**Notes:**

- lucyd builds Piper directly in `main.rs` (`lucyd::piper::Piper::start`). It also gets the echo canceller's playback reference from Piper (`take_reference`). Kokoro has to supply the same 16 kHz reference, or the echo canceller (ADR 0009) stops working while Kokoro speaks. Selecting between Kokoro and Piper at startup could follow the `by_kind::ByKind` pattern used for the Thinking Providers.
- Licence: Kokoro can fall back to espeak-ng for pronunciation, and espeak-ng is GPL. It has to run out of process, as Piper does (ADR 0003).
- The voice section (`crates/lucy/src/sections/voice.rs`) needs to offer the Provider first, then that Provider's voices. Picking Kokoro asks before installing or downloading anything, the same way the Piper install does now, and says roughly how big the download is.
- Config: `[piper] voice` stays as it is. The Provider choice and Kokoro's voice need a new key or section. Decide which in triage.
- Open questions for triage:
  - How Kokoro runs: a supervised Python server, like Piper (e.g. the `kokoro-onnx` package behind a small HTTP shim), or in-process ONNX through `ort`. Which one we pick drives the install plan, the licence and how restarts work.
  - Which Kokoro voices to list, and which is recommended.
  - Is Kokoro's latency to first sound acceptable on CPU? Measure it against `en_GB-cori-medium` before committing.
  - All three are answered under **Triage decisions** below.

**Triage decisions (2026-11-18):**

1. **Spike first, in the scratchpad only.** Install `kokoro-onnx` in a throwaway venv with the fp16 model and time first sound for typical one-sentence replies against `en_GB-cori-medium`. If Kokoro takes more than 2× Piper's time to first sound, stop: put the numbers in Comments, set Status to `needs-info`, build nothing. Don't fall back to int8 to get under the gate. If it passes, put the numbers in Comments and carry on.
2. **How it runs: out of process.** Lucy ships a small Python shim, embedded in lucyd with `include_str!` and written to the data dir at start. It imports `kokoro-onnx` and serves `/synthesize` on 127.0.0.1. lucyd supervises it like Piper (free port, restart on exit, `Ready { port, sample_rate }`). Factor out shared supervisor code only where it really repeats. espeak-ng (GPL) stays out of process (ADR 0003). The echo canceller's reference comes from the shared `playback` path.
3. **Config:** `[tts] provider = "piper" | "kokoro"` (default `"piper"`) plus a new `[kokoro] voice`. `[piper] voice` is unchanged, so each Provider keeps its own voice. lucyd picks the Provider at startup (`ByKind` pattern). When Kokoro is picked, Piper isn't started.
4. **Model:** Kokoro v1.0 **fp16** plus `voices-v1.0.bin`, from the pinned `kokoro-onnx` `model-files-v1.0` GitHub release and checked against pinned sha256s. lucyd fetches them on first open, as it does Piper voices.
5. **Install:** `kokoro-onnx` is a library, so the Piper install plan doesn't fit. When Kokoro is picked, setup asks first, gives the download size, then creates Lucy's own venv at `~/.local/share/lucy/kokoro/venv` with `uv venv` + `uv pip install kokoro-onnx`, falling back to `python -m venv` + `pip` without `uv`.
6. **Voices:** English only, British first. `bf_emma` is recommended.
7. **On failure:** notification text only. There's no fallback to Piper.
8. **Switching back to Piper** keeps Kokoro's venv and model on disk.
9. **Sample rate:** Kokoro outputs 24 kHz. Report it as `sample_rate` and let playback resample. If playback can't handle it, fix that inside this ticket.
10. **Unattended run:** work on branch `ticket-53-kokoro` in small commits, test-first for the "(test)" items. Run every check in `docs/agents/working-in-lucy.md`, then `/code-review` against this ticket, and fix the findings. No merge to main, no redeploy, no system-wide installs, no changes to the user's live config. Leave a hand-test checklist under Comments for the user: picking and hearing Kokoro in setup, a lucyd restart speaking in Kokoro, Barge-in, and switching back to Piper.

**Acceptance:**

- [ ] With no config change, Lucy still speaks in Piper and nothing Kokoro-related is downloaded
- [ ] In the setup voice section the user can pick Kokoro. Lucy asks before installing it, fetches it, says the sample line in it, and saves the choice
- [ ] After a restart, lucyd speaks in Kokoro. If Kokoro fails, replies go out as notification text and the Session carries on, as with Piper (harness test)
- [ ] The echo canceller gets a playback reference while Kokoro speaks (test), and Barge-in works with Kokoro by hand
- [ ] A test shows the Provider choice parses from config and defaults to Piper
- [ ] Switching back to Piper in setup works

## Comments

### 2026-11-18: spike failed the latency gate, so nothing was built

Measured on this machine (Ryzen 7 7700X, 16 threads, CPU only), in a throwaway scratchpad venv with `kokoro-onnx` 0.6.1. Each figure is the median of 3 runs of a whole sentence's synthesis, after warm-up, in process for both. That's the time to first sound for a sentence, since both servers synthesise a sentence before any of it plays. Kokoro voice `bf_emma`, `lang="en-gb"`. Piper `en_GB-cori-medium` through its Python API.

| Sentence | Piper | Kokoro fp16 | Kokoro int8 | fp16 ÷ Piper |
|---|---|---|---|---|
| "Okay." | 0.012 s | 0.152 s | 0.157 s | 13× |
| "Sure, I've opened it for you." | 0.026 s | 0.271 s | 0.270 s | 10× |
| "Claude has finished fixing the failing test…" (4.4 s of audio) | 0.053 s | 0.532 s | 0.565 s | 10× |
| "The weather in London today…" (4.1 s) | 0.053 s | 0.638 s | 0.533 s | 12× |
| "I found three files called invoice…" (5.5 s) | 0.069 s | 0.666 s | 0.702 s | 10× |

The gate was ≤ 2× Piper, so this **fails**. int8 is no faster than fp16 here (it was measured for information only, not as a fallback). Model load: fp16 0.36 s, Piper 0.18 s.

In absolute terms, Kokoro adds about 0.15–0.7 s before each sentence. Sentences after the first are synthesised while the one before plays, so mostly only the first sentence of a reply would be delayed by that much.

Release asset sha256s, for whenever this is picked up again: `kokoro-v1.0.fp16.onnx` (177,464,787 bytes) `c1610a859f3bdea01107e73e50100685af38fff88f5cd8e5c56df109ec880204`; `voices-v1.0.bin` (28,214,398 bytes) `bca610b8308e8d99f32e6fe4197e7ec01679264efed0cac9140fe9c29f1fbf7d`.

Also found: the venv's Python only imports `kokoro_onnx` with `PYTHONPATH` cleared (system numpy for 3.14 shadows the venv's), the same trap as Piper. The shim's launch would need `env_remove("PYTHONPATH")`.

**Decision needed from the user:** (a) relax the gate to an absolute limit, e.g. ≤ 0.7 s for a typical first sentence, and build as specced; (b) build it but split the first sentence into shorter pieces to hide the latency (more scope); or (c) wontfix, and try a better Piper voice instead (e.g. `en_GB-cori-high`, already in the voices dir) or another engine.

### 2026-11-19: user accepts the delay; build it

The user is happy with Kokoro's extra 0.15–0.7 s before a sentence, so the latency gate in triage decision 1 is waived and the spike counts as done. Build the rest of the triage decisions as written, with one addition:

- **Swappable engines.** Shape the TTS Provider seam so Piper, Kokoro or a future engine can be swapped in or out easily. The engine-specific part (how to find and start the engine, fetch a voice, synthesise a sentence) sits behind one small interface. Everything shared (supervision of the local server, playback, the echo reference, levels, events) is written once and reused. Adding an engine should mean one new module plus a config value, without touching the Session or playback code.

### 2026-11-19: built on `ticket-53-kokoro`, not merged; hand tests below

**What was built** (design in ADR 0013, operating notes under "Real Kokoro" in `docs/agents/working-in-lucy/providers.md`):

- `tts::Engine` is the swappable seam: start in a voice, turn a sentence into samples. `tts::Engines` holds one per `TtsKind` and owns everything shared once: playback, levels, ducking and the echo reference. `local_server::LocalServer` supervises any engine that runs as a localhost server (find on first use, free loopback port, restart on exit, look again next Session if not found). Piper and Kokoro each supply only a small `local_server::Server`.
- Kokoro: Lucy's own `server.py` (embedded in lucyd) runs in `$XDG_DATA_HOME/lucy/kokoro/venv` with `PYTHONPATH` cleared. The fp16 model and voices are fetched on first use, sha256-pinned.
- Setup: the voice section asks Piper or Kokoro first, then that Provider's voices (Kokoro: 6 British, then 4 American, `bf_emma` recommended). Installing Kokoro asks first, then runs `uv venv --clear --python 3.13` + `uv pip install kokoro-onnx==0.6.1`, or a `python3.10`–`3.13` venv without uv.

**Deviations from the triage decisions, for the user to accept or reverse:**

1. The key is `[tts] kind`, not `[tts] provider`, to match the existing `[stt] kind`.
2. The Provider is picked at each Session, not once at lucyd's start (like STT). That lets setup's Voice preview speak through Kokoro before lucyd restarts, and installing Kokoro or Piper needs no restart. At start lucyd still warms up only the configured engine, so Piper isn't started when Kokoro is picked. A preview through the other Provider leaves both servers running until lucyd restarts.
3. The no-uv fallback uses `python3.10`–`3.13`, not plain `python3`, since kokoro-onnx 0.6.1 needs Python below 3.14 (Arch ships 3.14).
4. Adding a third engine touches more than "one new module plus a config value": a `TtsKind` variant in both lucy-config and lucy-ipc, a `[section]`, an `Engines` field, `main.rs`, and its voices and settings keys in setup. Session and playback code stay untouched.

**Checked:** `scripts/check.sh` passes. Live tests passed on this machine: `live_kokoro_downloads_its_model_speaks_switches_voice_and_survives_a_crash` (real download and hash check, 24 kHz, voice switch, unknown voice refused, restart after a kill) and `live_piper_downloads_the_voice_speaks_and_survives_a_crash` (the Piper refactor). The speaker tests weren't run, since they play out loud.

**Hand tests for the user** (after merging and redeploying; see the redeploy memory):

- [ ] `LUCY_KOKORO_VENV=... cargo test -p lucyd --lib live_kokoro_speaks -- --ignored`, and the Piper twin: you hear two sentences, and they pass, so the echo reference is fed
- [ ] `lucy setup` → Voice → Kokoro: it asks before installing, installs, fetches the model during the preview, says the sample line, and saves `[tts] kind = "kokoro"`
- [ ] Next Session (or after `lucy service restart`): Lucy speaks in Kokoro
- [ ] Barge-in: talk over Kokoro mid-reply and she stops
- [ ] Setup → Voice → Piper → cori: the next Session is back in Piper

