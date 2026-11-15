# Does Silero run on pure-Rust ort-tract, and does earshot match it?

Type: task
Status: resolved
Blocked by: none

## Question

Check whether Silero VAD v6.2's If-less ONNX export runs correctly on `ort-tract`, so Lucy can avoid a native ONNX Runtime dependency. A/B Silero against earshot on recorded utterances and on echo-cancelled TTS residual: accuracy, latency, CPU. Confirm or replace the Silero default from [Which voice-activity detector should Lucy use, and how strict is the barge-in gate?](14-vad-barge-in.md).

## Answer

**Yes, Silero runs correctly on pure-Rust `ort-tract`; keep it. earshot doesn't replace it. But lucyd runs Silero 45× slower than it needs to, and fixing that is build ticket 33.** Details and tables: [research/vad-runtime-ab.md](../research/vad-runtime-ab.md); prototype in `prototypes/vad-ab/`.

- **Correctness:** the bundled If-less v6.2.3 model on tract matches native ONNX Runtime to within 0.0001, and the standard model to 4 decimals, on labelled speech, echo residual and real talk-overs.
- **CPU:** the If-less export still has an `If` on the runtime `sr` input, which tract runs as a subgraph on every chunk: 7.3 ms per 32 ms chunk, and the running lucyd's DSP thread sits at ~25% of a core while idle. With `sr` folded in as a constant 16000 it is 160 µs per chunk (~0.5% of a core), with the same output. An `ort` optimisation level makes no difference.
- **earshot:** level with Silero on clean speech, echo residual after `Sonora` and the human talk-overs (within 30 ms), and ~8× cheaper again. But in steady noise it calls most silence speech (23% of silent chunks at 10 dB SNR, 62–74% at 0 dB, against Silero's 2% and 0.3–8%), which would stop end-of-utterance from firing in a noisy room. Rejected as the default; no need for it once Silero is fixed.
- **Gate:** the 0.7 × 300 ms Barge-in gate held: no false fire on any residual after `Sonora`; one-word replies are shorter than its window, as designed.
- **Limits:** labelled speech is Piper's voice, not a human; one room, steady noise only, no laptop speakers.
