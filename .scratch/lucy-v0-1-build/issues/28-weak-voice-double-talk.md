# 28: A quiet voice can still Barge in

**What to build:** Barge-in by speech works when the user's voice reaches the mic weakly, from a low mic gain or a mic far from the mouth. In wayfinder 23, a voice at about −50 dBFS (normal is about −20) was mostly removed by AEC3's suppressor along with the echo during double talk, and both cancellers missed 2 of 3 talk-overs. The same recording boosted 12 dB caught 3 of 3, and boosted 24 dB caught them at ~290 ms. lucyd boosts a quiet mic ahead of the canceller itself, and `lucy setup`'s mic test tells the user when a mic is too quiet even for that.

Spec: Audio pipeline (AEC, VAD gate); TUI setup (mic test). Raised by [wayfinder 23](../../lucy-v0-1/issues/23-aec-on-real-speakers.md) (Answer, follow-up 2).

**Blocked by:** [27](27-gate-relaxes-without-echo.md) (both change `sonora.rs` and the canceller; build on its level measurement where it fits)

**Seams:** changes `Sonora` internally, `EchoCanceller` unchanged (owned by 09, resolved; extended by 27); extends `test.mic_level`'s `MicLevel` with `quiet` (owned by 20, resolved; this ticket owns the extension)

**Status:** resolved

**Decisions** (settled with the user):

- **Fix:** an automatic pre-gain ahead of AEC3, plus a level check in setup's mic test. AEC3's own capture pre-gain (`RuntimeSetting::CapturePreGain`) is the stage: it runs before the canceller and tells AEC3 the echo path's gain changed. AGC2 runs after the canceller in this pipeline, so it can't help; changing AEC3's `ep_strength.default_gain` or transparent mode made no difference.
- **What the gain follows:** the loud end of what the mic hears over the last ~30 s (a high percentile of frame level, not a single peak), whoever made it: the user or Lucy's echo. The gain brings that up towards a set level below full scale, at most +24 dB. The pre-gain clamps at full scale, and AEC3 can't cancel clipped echo, so following the loudest sound keeps Lucy's echo from clipping. On wayfinder 23's recordings that is about 0 dB on desk speakers (echo peaks at −8 dBFS), 0 dB on headphones once the user has spoken (voice peaks at −10 dBFS), and about +23 dB on the quiet mic (everything peaks at −35 dBFS). Accepted cost: in a quiet room with nobody talking it raises the noise floor.
- **When it changes:** only while Lucy is silent (her reference isn't arriving), and only when the new value differs by more than ~6 dB. Each change leaks more echo until AEC3's filter re-adapts, and resets its ERLE estimate, which puts the gate back on the unconverged bar for a few seconds. So the gain moves rarely and never mid-sentence.
- **Lifetime:** in memory, in the canceller, kept across Sessions for as long as lucyd runs. Each lucyd starts at 0 dB; nothing goes to disk.
- **Only with Lucy's own canceller:** the gain lives in `Sonora`, so with `echo_cancellation` left to PipeWire or off there is no gain. Whisper and the Wake word hear the boosted signal when it is on.
- **Seam:** the gain is internal to `Sonora`, which already sees every frame before AEC3 and knows when the reference plays. `EchoCanceller` and the gate don't change.
- **Constants, not config:** the window, the target level, the +24 dB maximum and the 6 dB step live in code, not `[audio]` or `[barge_in]`.
- **Setup's check:** lucyd judges it, so the constants stay in lucyd. `MicLevel` gains `quiet: bool`: the loudest level since the last ask is too low for even +24 dB to bring it up to the gain's target. If every reading during the meter was quiet, setup says "Lucy can barely hear you on this mic, so talking over her may not work. Move it closer, turn it up in your sound settings, or pick another mic." and "Keep this microphone?" defaults to No. Silence looks the same as a too-quiet mic, hence "barely hear you". The same check and words apply whatever `echo_cancellation` is set to.
- **Out of scope:** the user talking from Lucy's very first word. Wayfinder 23 saw a normal-level voice suppressed there too, because the canceller never converged. That is convergence, not level; a pre-gain doesn't fix it, and the Wake word still interrupts.

**Acceptance:**

- [x] Through the bench with lucyd's `Sonora`: wayfinder 23's quiet-voice recording (`tools/aec-bench/runs/wrong-mic/desk-talk`) catches 3 of 3 talk-overs
- [x] No new false Barge-ins on any of wayfinder 23's recordings, and the gain stays at 0 dB on `desk-*` and `headphones-talk` (so no clipped echo)
- [x] The gain is unit-tested in `sonora.rs`: a quiet input is boosted up to +24 dB, a loud one isn't, nothing changes while the reference plays, and a gap under 6 dB is left alone
- [x] `test.mic_level` reports `quiet` (tested in `crates/lucyd/tests/setup_tests.rs`); the schema is regenerated (`LUCY_UPDATE_SCHEMA=1 cargo test -p lucy-ipc`)
- [x] Setup, through `Home::setup`: a quiet meter shows the message and "Keep this microphone?" defaults to No; a normal one shows neither

The bench in `tools/aec-bench/` (README there; `./bench analyse <run>`, its copy of the gate in `src/gate.rs`) replays those recordings through lucyd's `Sonora`, Silero and gate.

## Comments

Resolved on `main`. `Sonora`'s `PreGain` (`sonora.rs`) sets AEC3's capture pre-gain, which AEC3 sees as an echo path gain change. It keeps every frame's level over the last 30 s, and works out the room's floor from it once a second: the 10th percentile.

- **Only sound counts (decided with the user while implementing):** following every frame raised the gain to +24 dB after 30 s of an idle, quiet room. On a normal mic the next Wake word and request would then have been clipped by up to ~20 dB. Only frames at least 20 dB over the floor enter the gain's window, so idle time doesn't age it. The gain moves only once that window holds 3 s of sound, so a fresh lucyd stays at 0 dB until it has heard some. A quiet room no longer raises the noise floor. Minutes of loud typing near the mic could still count as sound.
- **The gain:** once a second while Lucy is silent (no reference for 100 ms), it brings the 99th percentile of that sound to −22 dBFS (frame RMS), clamped to 0..+24 dB. It changes only when that differs by more than 6 dB from the gain in use. `EchoMeter` divides the gain back out of the cancelled power, so `echo_heard` compares like with like across a change.
- **Setup:** `too_quiet(rms)` holds when even +24 dB leaves the loudest frame under −22 dBFS, so under −46 dBFS. lucyd reports it as `MicLevel.quiet`.

Bench (`./bench analyse`, now 2 passes through the same cancellers as two Sessions in one lucyd, with a `gain` column):

| run | gain | false (pass 1 / 2) | talk-overs, pass 2 |
|---|---|---|---|
| wrong-mic/desk-talk | +23.4 dB | 0d0b / 1d0b | 380, 290, 350 ms (pass 1: 3 missed) |
| wrong-mic/desk-silent | +22.3 dB | 0d0b / 2d0b | |
| desk-talk | 0 | 0d0b / 0d0b | 290, 290, 280 ms |
| desk-silent | 0 | 2d0b / 0d0b | |
| headphones-talk | 0 | 0d0b / 0d0b | 290, 290, 290 ms |
| headphones-silent | 0 | 0d0b / 0d0b | |

The false ducks on wrong-mic pass 2 are the first ~300 ms of Lucy's first word after the gain changed (2.3–2.6 s): echo leaks until AEC3 re-adapts, as expected. The gate's warm-up bar keeps them to ducks. In a boosted pass the bench's ERLE and residual columns compare boosted output with the raw mic, so they read low by the gain.
