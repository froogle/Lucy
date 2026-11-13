# Which voice-activity detector should Lucy use, and how strict is the barge-in gate?

Type: research
Status: resolved
Blocked by: none

## Question

WebRTC APM v2 no longer reports voice activity, so Lucy needs a separate VAD on the echo-cancelled signal (see [How do we get echo cancellation for voice interruption with PipeWire, from Rust?](02-echo-cancellation.md)). Compare Rust-usable VADs (Silero via ONNX, webrtc-vad, earshot, others): licence, latency, CPU, accuracy on residual echo. Propose concrete barge-in gate parameters while Lucy is speaking (threshold, sustained-speech duration ~200–400 ms, AEC-convergence check) and end-of-utterance detection for normal turns. Recommend defaults.

## Answer

Use **Silero VAD v6.2** (MIT, code and weights). Run it on the echo-cancelled 16 kHz signal in 32 ms chunks through a thin own wrapper over `ort`, behind a `Vad` trait. It has the best published accuracy, especially on noise.
**earshot** (pure Rust, MIT/Apache) gets an A/B test in prototype 07. TEN VAD is rejected because its licence forbids uses that compete with Agora. WebRTC VAD is rejected for poor accuracy.
**Barge-in while Lucy speaks:**
- The VAD probability must stay ≥ 0.7 for 300 ms. When AEC is unconverged (ERLE < 10 dB, or the first 2 s of speech) the bar rises to 0.85 for 500 ms.
- The window covers the TTS plus a 300 ms tail.
- On the first frame above threshold, TTS is ducked. Once the 300 ms is met, TTS is stopped and flushed.
**End of utterance:** the threshold is 0.5 to start speech and 0.35 to end it. Speech starts after 100 ms, with a 500 ms pre-roll. It ends after 700 ms of silence, with a 30 s cap. Smart Turn v3.2 (BSD-2) is the later upgrade. All of these are starting values to tune in 07.
Open: whether Silero's If-less ONNX runs on pure-Rust `ort-tract`; otherwise it needs a native ONNX Runtime.

[findings](../research/vad-barge-in.md)
