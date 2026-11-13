# Does Silero run on pure-Rust ort-tract, and does earshot match it?

Type: task
Status: open
Blocked by: none

## Question

Check whether Silero VAD v6.2's If-less ONNX export runs correctly on `ort-tract`, so Lucy can avoid a native ONNX Runtime dependency. A/B Silero against earshot on recorded utterances and on echo-cancelled TTS residual: accuracy, latency, CPU. Confirm or replace the Silero default from [Which voice-activity detector should Lucy use, and how strict is the barge-in gate?](14-vad-barge-in.md).
