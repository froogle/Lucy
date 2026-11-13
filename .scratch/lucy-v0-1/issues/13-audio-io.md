# How should Lucy own microphone capture and speaker playback from Rust?

Type: research
Status: resolved
Blocked by: none

## Question

Lucy must own both the capture and playback streams (her TTS is the echo-cancellation reference; see [How do we get echo cancellation for voice interruption with PipeWire, from Rust?](02-echo-cancellation.md)). Compare the `pipewire` crate vs `cpal` (and anything else credible) for opening both streams so PipeWire handles clock drift between separate USB devices; choose a shared internal sample rate (16 vs 48 kHz) and the resampling path feeding AEC, VAD, wake word and STT; cover device selection/hot-plug and portability to non-PipeWire systems. Recommend one approach.

## Answer

- Use `cpal` 0.18 with the `pipewire` and `pulseaudio` features. Its PipeWire host puts both of Lucy's streams in one `node.group`, so PipeWire's adaptive resampler absorbs drift between separate USB devices. Its host order is PipeWire, then PulseAudio, then ALSA, which covers portability. The `pipewire` crate directly is the fallback behind an `AudioIo` seam.
- Internal rate is 16 kHz mono f32. Capture is requested at 16 kHz, so PipeWire resamples. TTS plays at the voice's native rate (22050 for Piper medium). A `rubato` `Fft` copy downsampled to 16 kHz is the AEC reference: the only resampler Lucy runs herself.
- cpal callbacks only push into `rtrb` rings. One DSP thread reframes to 10 ms (160 samples) and runs AEC, then fans out to wake word, VAD and STT.
- Devices are `"default"` (follows WirePlumber, AEC reset on `DeviceChanged`) or a pinned `node.name`. If a pinned device is lost, fall back to default and re-enumerate periodically; cpal has no hot-plug events. Plain ALSA is best effort, with no drift correction.

[findings](../research/audio-io.md)
