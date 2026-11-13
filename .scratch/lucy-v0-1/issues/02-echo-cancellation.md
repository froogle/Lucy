# How do we get echo cancellation for voice interruption with PipeWire, from Rust?

Type: research
Status: resolved
Blocked by: none

## Question

Barge-in is in v0.1: the user can interrupt Lucy while she speaks, so the mic must not hear Lucy's own TTS as speech. Compare PipeWire's built-in echo-cancel module (module-echo-cancel / WebRTC AEC) configured at the system level vs in-process AEC from Rust (e.g. webrtc-audio-processing crate, speexdsp). Cover setup burden for end users, latency, quality with laptop speakers vs headphones, and how barge-in detection (VAD on the cleaned signal) fits. Recommend one approach.

## Answer

Run AEC in-process: WebRTC AEC3 via the `webrtc-audio-processing` crate (2.x, BSD-3). Lucy plays TTS herself and feeds that PCM as the render reference. Behind an `EchoCanceller` seam, the pure-Rust `sonora` can be swapped in later.
PipeWire's `module-echo-cancel` runs the same AEC3 at the same 10 ms latency, but it requires changing system audio config and affects every app. Support it only as an opt-out (point the mic at its source, disable in-process AEC).
Wake word, VAD and STT all read the cleaned signal. VAD has to be separate, because APM v2 no longer reports voice activity. While Lucy speaks, barge-in needs a stricter gate: higher VAD threshold, ~200–400 ms of sustained speech, and AEC converged (ERLE).
Laptop speakers will leave some residual echo; headphones need no real cancelling. Measure this in prototype 07.

[findings](../research/echo-cancellation.md)
