# 33: Silero VAD at a fraction of the CPU

**What to build:** lucyd's idle `lucyd-dsp` thread uses ~25% of a core, almost all of it Silero. The bundled "If-less" model still has one `If`, on the `sr` input; since `sr` arrives at runtime, tract runs that branch as a subgraph on every 32 ms chunk (7.3 ms each). Bundle the model with `sr` folded in as a constant 16000 and stop passing `sr`: tract then folds the `If` away, at 160 µs per chunk with the same output (max difference 0.0001). Measured in wayfinder 26 ([research](../../lucy-v0-1/research/vad-runtime-ab.md)).

Spec: none; wayfinder 26.

**Blocked by:** none

**Seams:** none (`Vad` trait unchanged).

**Status:** ready-for-agent

**Notes:**

- **The model:** `.scratch/lucy-v0-1/prototypes/vad-ab/sr_const.py` makes it from upstream v6.2.3's `silero_vad_op18_ifless.onnx` (`uv run --with onnx python -I sr_const.py <in> <out>`; leave out the third argument, since fixed input shapes make tract fail to parse the model). Replace `crates/lucyd/models/silero_vad_op18_ifless.onnx` under a name that says what it is, keep `SILERO_VAD_LICENSE`, and say in `audio/silero.rs`'s header how the file was made.
- **The code:** `audio/silero.rs` drops the `sr` input. Setting an `ort` optimisation level makes no difference, so don't add one.

**Acceptance:**

- [ ] `cargo test -p lucyd --test silero -- --ignored` passes
- [ ] Redeployed, the idle `lucyd-dsp` thread uses under 3% of a core (`top -H -p $(pgrep -x lucyd)`); it was ~25%
- [ ] The AEC bench's `analyse` on `runs/*/` gives the same barge-in and false-duck columns as before (the bench copies lucyd's Silero; rebuild it after the change)
