# 27: The Barge-in gate relaxes when there is no echo

**What to build:** When Lucy's speaker doesn't reach the mic (headphones), the Barge-in gate uses its normal bar (`threshold` for `sustain_ms`) once the warm-up is over, not the unconverged one. Today it stays at `unconverged_threshold`/`unconverged_sustain_ms` all Session, because AEC3's ERLE never reaches `converged_erle_db` when there is no echo to remove, so talk-overs take ~490 ms instead of ~290 ms. lucyd measures whether Lucy's voice reaches the mic itself, and keeps measuring, so it reacts when the user changes their setup.

Spec: Audio pipeline (AEC, VAD gate). Raised by [wayfinder 23](../../lucy-v0-1/issues/23-aec-on-real-speakers.md) (Answer, follow-up 1).

**Blocked by:** None (can start immediately)

**Seams:** extends `EchoCanceller` with `echo_heard` (owned by 09, resolved; this ticket owns the extension); uses the gate in `crates/lucyd/src/audio/gate.rs` and `[barge_in]` (owned by 09, resolved)

**Status:** ready-for-agent

**Decisions** (settled with the user):

- **Signal:** the `sonora` adapter measures how much of Lucy's played voice reaches the mic itself. Not AEC3's `echo_return_loss` sentinel, and not the residual echo detector.
- **Seam:** `EchoCanceller::echo_heard() -> Option<bool>`: `Some(true)` echo reaches the mic, `Some(false)` it doesn't, `None` not sure yet. `Sonora` implements the measurement; the harness's fake canceller gets a settable answer.
- **Window:** rolling, over the last ~10 s of Lucy speaking (only time when her reference is playing counts), kept across Sessions since the canceller lives as long as lucyd. A switch between headphones and speakers shows within one window of her speech.
- **Unsure means echo:** until the window has filled, or while the user talks over most of it, the answer is `None` and the gate keeps today's strict bar.
- **Threshold:** no echo only when the estimated echo at the mic is below the noise floor. Faint echo counts as echo: an echo 1 dB above the floor still gave 93 false Barge-ins with no canceller.
- **Gate rule:** the canceller counts as converged once `warm_up_ms` is over and either its ERLE ≥ `converged_erle_db` or `echo_heard() == Some(false)`. The warm-up stays.
- **Constants, not config:** the window length and threshold live in code, not `[barge_in]`.
- **Accepted risk:** switching from headphones to speakers can leave the gate at the normal bar for up to about one window of Lucy's speech. The worst residual leak in wayfinder 23 (VAD 0.98 for 160 ms) was under that bar.
- **Out of scope:** nothing user-visible (no "headphones detected" in setup).

**Acceptance:**

- [ ] On wayfinder 23's recordings (`.scratch/lucy-v0-1/prototypes/aec-bench/runs/`), the measurement says: `headphones-*` no echo; `desk-*` (webcam mic, echo ~16 dB above the floor) echo; `wrong-mic/desk-*` (headset mic, ~1 dB above the floor) echo
- [ ] With no echo, a talk-over after the warm-up Barges in at the normal bar (~300 ms)
- [ ] On speakers, an unconverged canceller still gets the stricter bar; `None` behaves as today
- [ ] No false Barge-in on wayfinder 23's recordings
- [ ] Gate tests through the harness with the fake canceller's settable `echo_heard`; the measurement unit-tested in `sonora.rs`

The bench in `prototypes/aec-bench/` (README there; `./bench analyse <run>`, its copy of the gate in `src/gate.rs`) replays those recordings through lucyd's `Sonora`, Silero and gate. Copy the measurement in, or call it through `lucyd::audio`, to check the first and fourth boxes.

## Comments
