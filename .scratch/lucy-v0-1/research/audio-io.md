# Audio I/O: owning mic capture and speaker playback from Rust

Ticket: [13-audio-io](../issues/13-audio-io.md). Researched 2026-11-12. Builds on [echo-cancellation.md](echo-cancellation.md).

## Recommendation

- **Use `cpal` 0.18 with its `pipewire` and `pulseaudio` features on.** Use it for both streams: mic capture and TTS playback.
- **Let PipeWire do drift correction and device-rate conversion.**
  - Lucy asks for the capture stream as **16 kHz mono f32**.
  - She asks for the playback stream at **the TTS voice's native rate**: 22050 Hz mono for Piper "medium" voices.
- **Lucy's internal rate is 16 kHz mono f32.** Every consumer needs 16 kHz: AEC, VAD, wake word and STT.
- **Lucy runs exactly one resampler herself.** `rubato` takes a copy of the TTS PCM down to 16 kHz to use as the AEC render reference. The speaker still gets the full-band 22050 Hz audio.
- **Default device.** Lucy follows the system default unless the user pins a device by PipeWire `node.name`.
- **Hot-plug.** It is handled by re-enumerating devices and rebuilding streams when cpal reports an error. cpal has no hot-plug events.
- **Seam.** Put all of this behind a small `AudioIo` seam. If cpal's young PipeWire host misbehaves, it can be swapped for direct `pipewire`-crate code.

## Options compared

| | `cpal` 0.18.2 (`pipewire` feature) | `pipewire` crate 0.10.1 directly | `cpal` default (ALSA host only) | `libpulse-*-binding` / `pulseaudio` crate |
|---|---|---|---|---|
| Path to PipeWire | native libpipewire (it uses the `pipewire` crate internally) | native libpipewire | ALSA `pipewire`/`pulse` plugin device | pipewire-pulse protocol |
| Who resamples and corrects drift between USB devices | PipeWire graph | PipeWire graph | PipeWire, but only if you open the `pipewire` ALSA device; `default` is busy | PipeWire, via pulse |
| Works without PipeWire | yes: falls back to PulseAudio, then ALSA at runtime | no | yes | PulseAudio only |
| Code Lucy writes | `build_input_stream` / `build_output_stream` callbacks | mainloop thread, SPA format pods, registry listener, RT promotion | same as cpal | moderate |
| Licence | Apache-2.0 | MIT | Apache-2.0 | MIT/Apache, MIT |
| Maturity | cpal: ~28.5M downloads. Its PipeWire host is **new in 0.18.0 (2026-06-06)**, and 0.18.2 fixed many PipeWire bugs | ~3.5M downloads, maintained by freedesktop | most mature path | mature |

Sources: crates.io API (versions, downloads, licences); [cpal CHANGELOG](https://github.com/RustAudio/cpal/blob/master/CHANGELOG.md) 0.18.0 "PipeWire: New host for Linux and BSD via the native PipeWire API" and 0.18.2 fixes; cpal 0.18.2 `Cargo.toml` (`pipewire = { version = "0.10", features = ["v0_3_53"], optional }`, `pulseaudio = "0.3"`, a pure-Rust client: [colinmarc/pulseaudio-rs](https://github.com/colinmarc/pulseaudio-rs)).

### What cpal's PipeWire host actually does (read from the 0.18.2 source, `src/host/pipewire/`)

- **Host order.** `default_host()` on Linux tries PipeWire, then PulseAudio, then ALSA, picking the first one available (`src/platform/mod.rs` ~834–846). PipeWire counts as available when its socket exists (`mod.rs:63`). Both features are **off by default**, so Lucy must turn them on.
- **Both streams share one driver.** Every stream gets `node.group = "cpal-<pid>"`. The comment reads: "Group input and output nodes so PipeWire schedules them in the same quantum, preventing phase drift between simultaneous input/output streams" (`device.rs:173–175`). The libpipewire header defines `node.group` as: "Nodes in the same group are always scheduled with the same driver" (`/usr/include/pipewire-0.3/pipewire/keys.h:153`).
- **Rate requests.**
  - The requested rate goes into the stream's `EnumFormat` (`stream.rs:770–772`), so PipeWire's stream adapter converts graph rate ↔ stream rate.
  - If the negotiated format doesn't match the request, cpal raises `UnsupportedConfig` rather than passing through unexpected audio (`stream.rs:655–672`).
  - `validate_stream_config` only rejects 0 Hz and 0 channels (`lib.rs:875`).
  - Caveat: `supported_input_configs()` only lists the graph's `clock.allowed-rates` (here `[48000]`). Lucy has to request 16 kHz directly rather than pick it from that list. *Not exercised on this machine; confirm in prototype 07.*
- **Device pinning.**
  - A specific device is targeted with `target.object = <object.serial>` plus `DONT_RECONNECT` (`device.rs:169–171`, `stream.rs:794–797`). If that device goes away, the stream gets `ErrorKind::DeviceNotAvailable` ("Device disconnected", `stream.rs:266–275`).
  - Default-device streams instead use `AUTOCONNECT`, so WirePlumber moves them when the default changes. cpal watches the `default` metadata and reports `ErrorKind::DeviceChanged` for information only (`stream.rs:438–520`, `705–720`).
- **Device IDs.** `DeviceId` is `(PipeWire, node.name)` (`device.rs:214–216`). On this machine that gives stable names such as `alsa_input.usb-Generic_ATH-M50xSTS-USB-00.mono-fallback`, which are good for config files.
- **Enumeration is a snapshot.** `PipeWireHost::new()` runs `init_devices` once and `devices()` returns a clone (`mod.rs:32–68`, `device.rs:743`). New devices only show up after building a new `Host`.
- **Cosmetic.** Streams are named `cpal-capture-<pid>` / `cpal-playback-<pid>` (`stream.rs:624–627`, `885–888`), and cpal exposes no way to set `media.role` or `node.description`. In `pavucontrol` Lucy will show up under those generic names. Fixing that needs an upstream PR or the direct `pipewire` crate.
- **Other features.** RT thread promotion is built in (`rt_promote.rs`; the `realtime-dbus` feature uses rtkit). Xruns are reported as `ErrorKind::Xrun` (0.18.2).

### Why not the `pipewire` crate directly

- **Same capabilities as cpal.** It offers `target.object`, `node.group` and arbitrary rates in the format pod: see [`examples/audio-capture.rs`](https://gitlab.freedesktop.org/pipewire/pipewire-rs/-/blob/main/pipewire/examples/audio-capture.rs), which leaves rate empty "to accept the native graph rate" but could set it.
- **It would only add live hot-plug events** through a registry listener.
- **More code for Lucy.** Lucy would own the mainloop thread, format pod serialisation and RT promotion. cpal already does all of that, as the source above shows.
- **No portability.** It is Linux/PipeWire only.
- **Same build cost.** It needs libpipewire headers and clang for `bindgen` (pipewire-sys build deps: `bindgen ^0.72`, `system-deps`). cpal's `pipewire` feature costs the same, since it depends on this crate.
- **Keep it as the fallback** behind the seam.

## How PipeWire handles drift between separate USB devices

From [pipewire-props(7)](https://docs.pipewire.org/page_man_pipewire-props_7.html):

- **One driver per graph.** "The driver is selected from all linked devices by selecting the device with the highest priority" (`priority.driver`). Followers run in lockstep with it ([scheduling](https://docs.pipewire.org/page_scheduling.html)).
- **Resampling happens automatically** in these cases (quoted):
  - "The hardware of a device node does not support the graph samplerate."
  - "The hardware clock of a device does not run at the same speed as the graph clock and adaptive resampling is required to match the clocks."
  - "A stream does not have the same samplerate as the graph and needs to be resampled."
- **Shared clocks skip resampling.** "Devices with the same clock name will not use a resampler to align the clocks" (`clock.name`). The ALSA DLL is tuned with `api.alsa.dll-bandwidth-max`.

On this machine (PipeWire 1.6.9, WirePlumber 0.5.18; `pw-metadata -n settings`: `clock.rate 48000`, `clock.allowed-rates [48000]`, `clock.quantum 1024`):

- **Default sink:** Pebble V3 USB speakers. `priority.driver` 1109, `s16le` 48 kHz.
- **Default source:** ATH-M50xSTS USB mic. `priority.driver` 2100, `s24le` mono 48 kHz.
- **OBSBOT webcam mic:** runs natively at **32 kHz** (`pactl list short sources`). PipeWire already resamples it to the 48 kHz graph, which shows a device-rate conversion happening today with no user setup.
- **Conclusion.** The mic node has the higher priority, so it drives. The speakers become a follower and PipeWire's adaptive resampler matches the Pebble's clock to the mic's. Lucy's two streams see one clock, so the AEC reference and capture never drift apart. AEC3's delay estimator only has to absorb a constant offset (see echo-cancellation.md).

## Shared internal rate: 16 kHz

Every downstream consumer wants 16 kHz:

| Consumer | Rate | Source |
|---|---|---|
| Whisper (STT default) | 16000 only | [whisper.h](https://github.com/ggml-org/whisper.cpp/blob/master/include/whisper.h) `#define WHISPER_SAMPLE_RATE 16000` |
| Silero VAD | 8000 or 16000 | [silero-vad README](https://github.com/snakers4/silero-vad) |
| openWakeWord (representative wake-word engine) | 16 kHz 16-bit PCM | [openWakeWord README](https://github.com/dscripka/openWakeWord) |
| WebRTC APM (`webrtc-audio-processing`) | 8/16/32/48 kHz. A single `Processor` rate applies to both render and capture. | echo-cancellation.md |
| Piper TTS output | 22050 for `medium` voices (e.g. `en_US-lessac-medium.onnx.json` `"sample_rate": 22050`); voice-dependent | [piper-voices](https://huggingface.co/rhasspy/piper-voices) |

**Option 1 (chosen): run AEC at 16 kHz.**
- Capture arrives from PipeWire already at 16 kHz mono. PipeWire's resampler does 48→16 with anti-aliasing (`resample.quality` default 4).
- AEC, VAD, wake word and STT all consume it with no conversion inside Lucy.
- Lucy only converts the TTS reference copy (22050→16000). That ratio is fixed and synchronous, so use [`rubato`](https://github.com/HEnquist/rubato) 5.x `Fft` (synchronous FFT resampler, "for fixed and locked rates"; MIT OR Apache-2.0; real-time safe, no allocation while processing).
- If the configured voice is already 16 kHz (Piper `low`/`x_low`), Lucy does no resampling at all.

**Option 2 (rejected): run AEC at 48 kHz.**
- The AEC would cancel the full band, but Lucy would then need a 48→16 downsampler for every consumer, plus 22050→48000 for the reference.
- The extra band (8–24 kHz) helps no consumer, because every one of them throws it away.
- Costs 3× the AEC CPU and adds a second resampler.

**Why playback stays at the voice's native rate rather than 16 kHz.**
- Playing the 22050 Hz PCM unchanged keeps Piper's 8–11 kHz sibilance for the listener; PipeWire converts it to the 48 kHz graph.
- Echo above 8 kHz doesn't matter, because the 16 kHz capture stream has already been band-limited by PipeWire.
- The 16 kHz reference therefore covers everything the AEC can see.
- Resample the reference copy at the moment the PCM is handed to the playback callback, so reference timing follows real playback.

## Framing and threading

- **Callback sizes don't match the 10 ms frame.**
  - The graph quantum here is 1024 frames at 48 kHz (~21.3 ms), so a 16 kHz stream gets about 341 frames per callback.
  - WebRTC APM needs exactly 10 ms frames: 160 samples at 16 kHz. The crate panics on a size mismatch (echo-cancellation.md).
- **Proposed design.**
  - Each cpal callback only copies samples into a lock-free SPSC ring ([`rtrb`](https://crates.io/crates/rtrb) 0.4, "realtime-safe single-producer single-consumer ring buffer").
  - The mic callback pushes capture samples.
  - The playback callback pulls TTS PCM from a ring and pushes the 16 kHz reference copy to a second ring.
  - One DSP thread takes 160-sample frames: `analyze_render_frame` for each pending reference frame, then `process_capture_frame`, then fan-out to wake word / VAD / STT.
  - Barge-in flush: clear the playback ring but keep feeding the AEC whatever actually played (echo-cancellation.md).
- **Optional.** `BufferSize::Fixed(n)` sets `node.latency = n/rate` (`device.rs:177–182`). Smaller quanta cut barge-in reaction time, but the user's whole graph inherits them. Leave it at `Default` unless prototype 07 shows a need.

## Device selection and hot-plug

- **Config.** `audio.input` / `audio.output` are each either `"default"` or a PipeWire `node.name`. The TUI lists `host.devices()` with `DeviceDescription` names and stores the `node.name`. cpal 0.18.2 gives multi-port devices distinct names.
- **Default device.**
  - Streams on the default device follow the system default automatically (WirePlumber moves them).
  - cpal's `DeviceChanged` notice is the point where Lucy resets the AEC, because the echo path changed. AEC3 re-converges on its own, but a reset makes it faster.
- **Pinned device unplugged.** The stream emits `DeviceNotAvailable`. Lucy drops the stream, falls back to `default`, and says so once.
- **Pinned device plugged back in.** There are no hot-plug events, so while running on a fallback Lucy rebuilds a `Host` every few seconds and checks `devices()` for the pinned `node.name`. That is one PipeWire round-trip, which is cheap.
- **Busy device.** Open fails with `DeviceBusy` or `PermissionDenied`. Surface it through the Indicator or a notification.

## Portability to non-PipeWire systems

- **PulseAudio systems.** The same code runs on cpal's `pulseaudio` host. PulseAudio resamples streams to the device rate. The PipeWire host's same-driver guarantee doesn't apply, but separate input and output devices under PulseAudio are each rate-matched by the server, which is acceptable for AEC3. *Not verified; low priority.*
- **Plain ALSA (no sound server).**
  - The ALSA host opens hardware directly. Two separate USB devices then drift with nothing correcting them. AEC3 has no drift compensation (its old AEC had one), so cancellation slowly degrades.
  - Requesting 16 kHz on ALSA `hw:` fails unless the card supports it. You need `plughw:` or a `rubato` `Async` resampler in Lucy.
  - v0.1 stance: plain ALSA is **best effort**. Support it when mic and speaker are the same card or with headphones. Document that barge-in on separate devices needs PipeWire or PulseAudio.
- **Build.** With the `pipewire` feature, the build needs libpipewire headers and clang for bindgen (cpal README: `libpipewire-0.3-dev` / `pipewire-devel`; the Arch `pipewire` package ships the headers). ALSA headers are always needed on Linux (cpal README: "ALSA is needed even when using JACK, PipeWire, or PulseAudio").

## Open / unverified

- **Run in prototype 07:**
  - Open a 16 kHz mono f32 capture stream and a 22050 Hz playback stream through cpal's PipeWire host.
  - Confirm negotiation succeeds without an `UnsupportedConfig` mismatch.
  - Confirm `pw-top` shows both Lucy nodes under one driver.
- **Upstream gap.** cpal's PipeWire host has no API for stream or node properties (`node.description`, `media.role`), so Lucy shows up as `cpal-capture-<pid>`. Either send an upstream PR or accept it.
- **Maturity risk.** cpal's PipeWire host is five months old. The 0.18.2 changelog fixed "streams starting audio before `play()`", "RT promotion blocking the callback", and "24-bit format mapping". Pin the version and keep the `AudioIo` seam.
