# Which local wake-word engine works from Rust for a custom word like "Lucy"?

Type: research
Status: resolved
Blocked by: none

## Question

Survey local, always-on wake-word engines usable from Rust (e.g. openWakeWord via ONNX, rustpotter, Porcupine, Vosk keyword spotting). For each: licence compatibility with an MIT/Apache open-source project, how a custom word like "Lucy" is trained or configured (and whether end users can change it), CPU/memory cost when idle, reported false-accept/false-reject behaviour, and PipeWire/ALSA capture integration. Recommend one default.

## Answer

Default to **LiveKit WakeWord v2** through the official Rust crate `livekit-wakeword`. Its code and its model weights are both Apache-2.0. It runs on a pure-Rust tract backend and costs about 1.6% of one core. It reports 89% recall at 0.5 false alarms/h on LiveKit's own bench, against 46% for openWakeWord.
The project trains and ships `lucy.onnx` once. Training is a one-off that needs a GPU and a free LiveKit Cloud TTS account. Users swap in another `.onnx` file to change the word. Capture uses cpal 0.18 with its native PipeWire host.
Caveat: v2 is merged but not yet released on crates.io (latest is 0.1.3), so pin a git rev for now.
Rejected options:
- openWakeWord: its pre-trained models are CC BY-NC-SA.
- Porcupine: the free tier ended on 2026-06-30 and it needs an AccessKey.
- rustpotter: unmaintained since 2023.
- Vosk: a full ASR, not a keyword spotter.
sherpa-onnx open-vocabulary KWS (type any word, no training) is the runner-up, but short keywords show weak recall.

[findings](../research/wake-word-engine.md)
