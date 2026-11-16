# Lucy's echo canceller is AEC3 with a pre-gain ahead of it and a meter of whether her echo is heard

For Barge-in by speech, lucyd must hear the user over Lucy's own voice. It cancels her echo with WebRTC's AEC3 through `sonora`, a pure-Rust port, in `audio/sonora.rs`. The render reference is the 16 kHz copy of what playback actually played. AEC3 finds the delay itself, so the stream delay is set to 0.

Pre-gain. A quiet microphone is boosted ahead of AEC3, because otherwise AEC3 removes a weak user voice along with Lucy's echo. The gain follows the loud end (99th percentile) of the sound heard over the last 30 s, user or echo, towards −22 dBFS, at most +24 dB. Following the loudest sound keeps Lucy's echo from clipping. Only frames 20 dB over the noise floor (10th percentile) count as sound, so a quiet room doesn't raise the gain. The gain changes only while Lucy is silent, only after 300 frames of sound, and only by more than 6 dB, because each change leaks echo until AEC3 re-adapts and resets its ERLE.

Echo meter (`echo_heard`). The gate needs to know whether Lucy's voice reaches the mic at all: with headphones it never does, and speech can barge in at once. The meter looks over the last 10 s of her speech, in 5-frame blocks (longer than playback's ~33 ms reference hand-over, so bursts even out), at lags up to 50 frames. Her echo is heard when the part of mic power that follows what she played (a least-squares fit) is at least as loud as the floor, the mic's mean while playback is in its quietest quarter. Blocks where the cancelled mic is 10 dB over its quiet level are the user, or echo not yet learned, and are left out. Mic power is capped at 4× its median while playback is loudest, because AEC3's suppressor hides user blocks where Lucy is loudest. The meter says nothing until the window fills, or while the user talks over most of it. Lucy counts as speaking for 10 frames after the reference last arrived.

The numbers were tuned on recordings with `tools/aec-bench/` (tickets 27 and 28).

## Considered Options

- The C++ `webrtc-audio-processing` the spec names: a native build dependency for the same AEC3.
- PipeWire's echo-cancel module: kept as an option (`[audio] echo_cancellation = "pipewire"`), but the user must set it up, and PulseAudio and ALSA have nothing like it.
- No pre-gain: a quiet USB mic's user voice is cancelled with the echo.
- A fixed pre-gain: clips Lucy's echo on a loud mic.
