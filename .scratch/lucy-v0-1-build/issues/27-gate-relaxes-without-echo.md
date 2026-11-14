# 27: The Barge-in gate relaxes when there is no echo

**What to build:** When Lucy's speaker doesn't reach the mic (headphones), the Barge-in gate uses its normal bar (`threshold` for `sustain_ms`), not the unconverged one, once the warm-up is over. Today it stays at `unconverged_threshold`/`unconverged_sustain_ms` all Session, because AEC3's ERLE never reaches `converged_erle_db` when there is no echo to remove, so talk-overs take ~490 ms instead of ~290 ms.

Spec: Audio pipeline (AEC, VAD gate). Raised by [wayfinder 23](../../lucy-v0-1/issues/23-aec-on-real-speakers.md) (Answer, follow-up 1).

**Blocked by:** None (can start immediately)

**Seams:** extends `EchoCanceller` (owned by 09, resolved) if the canceller has to report "no echo path"; uses the gate in `crates/lucyd/src/audio/gate.rs` and `[barge_in]` (owned by 09, resolved)

**Status:** needs-triage

Open question for triage: which signal means "no echo path". In wayfinder 23's runs, sonora's `echo_return_loss` stayed at its initial −30 dB for a whole headphone run and reached 17–20 dB (its clamp) within 5–7 s on speakers, but that is a sentinel, not a measurement. AEC3's transparent mode (built for headsets) and coupling measured by lucyd (mic level while Lucy speaks and the user doesn't) are the alternatives.

- [ ] With headphones (no echo), a talk-over after the warm-up Barges in at the normal bar (~300 ms)
- [ ] On speakers, an unconverged canceller still gets the stricter bar
- [ ] No false Barge-in on wayfinder 23's recordings (`prototypes/aec-bench/runs/`)
- [ ] Harness tests with `Harness::echo`

## Comments
