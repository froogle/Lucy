# 33: Silero VAD at a fraction of the CPU

**What to build:** lucyd's idle `lucyd-dsp` thread uses ~25% of a core, almost all of it Silero. The bundled "If-less" model still has one `If`, on the `sr` input; since `sr` arrives at runtime, tract runs that branch as a subgraph on every 32 ms chunk (7.3 ms each). Bundle the model with `sr` folded in as a constant 16000, stop passing `sr`, and set `ort`'s graph optimisation level: tract then folds the `If` away (either change alone doesn't), at 160 µs per chunk with the same output (max difference 0.0001). Measured in wayfinder 26 ([research](../../lucy-v0-1/research/vad-runtime-ab.md)).

Spec: none; wayfinder 26.

**Blocked by:** none

**Seams:** none (`Vad` trait unchanged).

**Status:** resolved

**Notes:**

- **The model:** `.scratch/lucy-v0-1/prototypes/vad-ab/sr_const.py` makes it from upstream v6.2.3's `silero_vad_op18_ifless.onnx` (`uv run --with onnx python -I sr_const.py <in> <out>`; leave out the third argument, since fixed input shapes make tract fail to parse the model). Replace `crates/lucyd/models/silero_vad_op18_ifless.onnx` under a name that says what it is, keep `SILERO_VAD_LICENSE`, and say in `audio/silero.rs`'s header how the file was made.
- **The code:** `audio/silero.rs` drops the `sr` input and builds its session with `GraphOptimizationLevel::Level3`; `ort-tract` only optimises the graph when a level is set.

**Acceptance:**

- [x] `cargo test -p lucyd --test silero -- --ignored` passes
- [x] Redeployed, the idle `lucyd-dsp` thread uses under 3% of a core (`top -H -p $(pgrep -x lucyd)`); it was ~25%
- [x] The AEC bench's `analyse` on `runs/*/` gives the same barge-in and false-duck columns as before (the bench copies lucyd's Silero; rebuild it after the change)

## Comments

2026-11-15: resolved on `silero-sr-folded`. The bundled model is now `models/silero_vad_16k.onnx`; `audio/silero.rs` passes no `sr` and sets `GraphOptimizationLevel::Level3`. It takes both: either alone left Silero at ~6.7 ms per chunk (the first version of this ticket said the fold alone was enough; corrected here and in wayfinder 26).

- Silero in lucyd: 2.1 ms → 52 µs per 10 ms frame (21% → 0.5% of a core), measured with lucyd's own `Silero`, `LiveKit` and `Sonora` on the `headphones-silent` recording. The wake word is now the biggest part of the DSP thread at ~1.6%; `Sonora` is 0.04%.
- Redeployed (all four binaries, News Plugin matches): idle `lucyd-dsp` 2.88% of a core averaged over 60 s, from ~25%.
- AEC bench `analyse` on all runs: output byte-identical to before the change, and the run takes 24 s instead of 10 min 43 s.
- `scripts/check.sh` passes.
