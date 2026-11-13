# How well does echo cancellation work on real speakers, and does sonora match the C++ library?

Type: prototype
Status: open
Blocked by: none

## Question

Extend the voice-loop prototype with in-process AEC3 (see [How do we get echo cancellation for voice interruption with PipeWire, from Rust?](02-echo-cancellation.md)), with Lucy's own TTS as the reference. Measure echo reduction (ERLE) and residual echo on laptop speakers, desk speakers and headphones. Check whether the pure-Rust `sonora` matches the C++ `webrtc-audio-processing` 2.x, and whether the barge-in gate from [Which voice-activity detector should Lucy use, and how strict is the barge-in gate?](14-vad-barge-in.md) holds without false interrupts on each setup.
