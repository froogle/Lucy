# Echo cancellation for barge-in: PipeWire module vs in-process AEC from Rust

Ticket: [02-echo-cancellation](../issues/02-echo-cancellation.md). Researched 2026-11-12.

## Recommendation

Run AEC **in Lucy's own process**, using WebRTC AEC3 through the `webrtc-audio-processing` crate (tonarino, 2.x). Lucy plays TTS herself and feeds the same PCM to the canceller as the render reference. Wake word, VAD and STT all read the cleaned capture signal. Put it behind a small `EchoCanceller` seam so that `sonora` (a pure-Rust AEC3 port) can be swapped in later. Do **not** depend on PipeWire's `module-echo-cancel` by default. Offer it as an opt-out instead: if the user configured the module themselves, they point Lucy's mic at "Echo Cancellation Source" and turn the in-process AEC off.

Why: end users set up nothing, and Lucy's audio graph stays the same. Both options run the same algorithm (WebRTC AEC3), so quality is the same. Latency is about the same, one 10 ms block. Lucy also knows exactly when she is speaking, which barge-in logic needs anyway (see below).

## The two options

### A. PipeWire `libpipewire-module-echo-cancel` (system level)

- **What it is.** The module creates 4 streams: mic capture, a virtual **sink** that apps play into, a playback stream that forwards the sink to the speakers, and a virtual **source** that carries the cancelled mic signal. Instead of creating a sink, `monitor.mode` captures the default sink's monitor ports, so *all* system audio becomes the reference. ([module source header](https://github.com/PipeWire/pipewire/blob/master/src/modules/module-echo-cancel.c), mirrored at [docs.pipewire.org/page_module_echo_cancel](https://docs.pipewire.org/page_module_echo_cancel.html))
- **Library.** Only `aec/libspa-aec-webrtc` is supported, and it is the default (same source). On this machine, `libspa-aec-webrtc.so` links `libwebrtc-audio-processing-1.so.3` (Arch `webrtc-audio-processing-1` 1.3, required by `pipewire-audio`). So it is **AEC3** via the `HAVE_WEBRTC1` code path, which sets `config.echo_canceller.enabled = true` ([aec-webrtc.cpp](https://github.com/PipeWire/pipewire/blob/master/spa/plugins/aec/aec-webrtc.cpp), lines ~188–206).
- **Tunables (`aec.args`).** `webrtc.high_pass_filter` (default true), `webrtc.noise_suppression` (true), `webrtc.transient_suppression` (true), `webrtc.voice_detection` (true), `webrtc.gain_control` (false), `webrtc.mobile_mode` (false). Confirmed in the source and in `strings` of the installed `.so`. Module args also include `buffer.max_size` (default 100 ms) and `buffer.play_delay` (default 0) ([module-echo-cancel.c](https://github.com/PipeWire/pipewire/blob/master/src/modules/module-echo-cancel.c) `#define MAX_BUFSIZE_MS 100`, `DELAY_MS 0`).
- **Latency.** The webrtc plugin declares `aec.latency = "480/48000"` (10 ms blocks), and the module sets `node.latency` from it (aec-webrtc.cpp:454, module-echo-cancel.c ~1602–1630).
- **Clock drift.** The plugin turns off WebRTC's own drift compensation "since PipeWire will already do drift compensation on all sinks and sources linked to this echo-canceler" (aec-webrtc.cpp:304–306). All streams share a `node.group` (module-echo-cancel.c:1377).
- **How it gets enabled.** Either a drop-in at `~/.config/pipewire/pipewire.conf.d/*.conf` followed by a PipeWire restart (example in the doc page), or at runtime through pipewire-pulse: `pactl load-module module-echo-cancel aec_method=webrtc source_master=… sink_master=… aec_args=…` ([pulse-module-echo-cancel.c](https://github.com/PipeWire/pipewire/blob/master/src/modules/module-protocol-pulse/modules/pulse-module-echo-cancel.c)). A module loaded this way lasts until restart and is not persisted. It could also run in a separate process (`pipewire -c some.conf`). The safe `pipewire` crate (0.10.1) has no `Context::load_module`, so loading it inside Lucy would mean unsafe `pipewire-sys` calls (checked the crate source).
- **Setup burden and side effects.** The user (or Lucy, on their behalf) has to change the audio graph. The new devices show up in every mixer. Unless Lucy targets the nodes explicitly, the user has to make the AEC sink and source the defaults, or Lucy's TTS has to be routed to the AEC sink with `target.object`. Everything played through that sink, and every app reading that source, also goes through AEC, noise suppression and a 10 ms block. Many users would not want that for music or calls. The module does exist on every PipeWire install, though: it is in this machine's stock `pipewire` 1.6.9 package (`/usr/lib/pipewire-0.3/libpipewire-module-echo-cancel.so`). None is loaded right now, and there is no `~/.config/pipewire`.

### B. In-process AEC from Rust

| Crate | Engine | Deps | License | Status |
|---|---|---|---|---|
| [`webrtc-audio-processing`](https://github.com/tonarino/webrtc-audio-processing) 2.1.0 | WebRTC APM (PulseAudio repackaging) v2, AEC3 | system `webrtc-audio-processing-2` via pkg-config, or the `bundled` feature (meson + ninja + C++ toolchain, git submodule) | BSD-3-Clause | ~193k downloads, updated 2026-05 |
| [`sonora`](https://github.com/dignifiedquire/sonora) 0.3.0 | Pure-Rust port of WebRTC APM M145 (AEC3, NS, AGC2, HPF) | none (pure Rust) | BSD-3-Clause | Young, AI-assisted port. The README says the C++ APM test suite (2400+ tests) passes against it via FFI. The neural residual echo estimator is not ported. |
| [`aec3`](https://github.com/RubyBit/aec3-rs) 0.4.0 | Pure-Rust AEC3 port plus graph runtime | none | MIT OR BSD-3 | README: "work in progress, API expected to evolve" |
| [`speexdsp`](https://github.com/rust-av/speexdsp-rs) 0.1.2 / [`aec-rs`](https://github.com/thewh1teagle/aec-rs) 1.0.0 | Speex MDF echo canceller | pure Rust (c2rust) or system speexdsp | MIT | speexdsp-rs last updated 2022. Older algorithm with no delay estimator, so the caller has to align render and capture. |

Facts about `webrtc-audio-processing` 2.x, from its source ([src/lib.rs](https://github.com/tonarino/webrtc-audio-processing/blob/main/src/lib.rs), [config](https://github.com/tonarino/webrtc-audio-processing/blob/main/webrtc-audio-processing-config/src/lib.rs), [sys build.rs](https://github.com/tonarino/webrtc-audio-processing/blob/main/webrtc-audio-processing-sys/build.rs)):

- `Processor::new(sample_rate_hz)`, then `process_render_frame` / `analyze_render_frame` (speaker reference) and `process_capture_frame` (mic, modified in place). Frames are non-interleaved `f32` and exactly 10 ms. The crate panics on a size mismatch.
- `Processor` is `Send + Sync` with `&self` methods, so one `Arc<Processor>` can be shared by the TTS playback thread and the mic thread.
- The default echo canceller is `EchoCanceller::Full { stream_delay_ms: None }`, which is AEC3 estimating the render-to-capture delay itself. `Mobile` (AECM) needs an explicit delay.
- The same pipeline offers noise suppression, a high-pass filter and AGC (`Config` fields). `get_stats()` exposes `echo_return_loss_enhancement` and `delay_ms`, which are useful for diagnostics and a "is AEC converged?" check.
- It links `webrtc-audio-processing-2`. Arch has this as `extra/webrtc-audio-processing` 2.1-9, which is **not** installed here: PipeWire uses `-1`. So the AUR package needs one extra repo dependency, or the crate has to be built `bundled`.
- **No VAD from APM.** The crate's sys tests assert that `stats.voice_detected` is "declared in upstream but never populated in v2.1". VAD has to come from somewhere else.

## Comparison against the ticket's criteria

**Setup burden for end users.**
- A (module): a config drop-in plus PipeWire restart, or a runtime `pactl load-module` that Lucy would issue and later unload. Either way it changes system audio for every app, and a crash can leave stray virtual devices behind. That breaks "nothing personal hard-coded / works on a stock Omarchy box".
- B (in-process): nothing to set up beyond a package dependency (or none with `sonora` or `bundled`).

**Latency.**
- Both run AEC3 on 10 ms blocks.
- A adds one PipeWire node with `node.latency` 480/48000, and it applies to *all* audio routed through it.
- B adds no graph latency for other apps. Lucy's own path gains at most one 10 ms frame of buffering.
- Neither matters next to STT or LLM latency.

**Quality with laptop speakers vs headphones.**
- Same engine (AEC3) both ways, so quality is roughly equal.
- **Laptop speakers** are the hard case. Speaker and mic are millimetres apart in one chassis, the echo is loud, and small drivers distort it nonlinearly. AEC3's linear filter plus residual suppressor will leave some residual echo while Lucy talks, especially at high volume, so barge-in needs a higher bar while she is speaking (below). *Not measured here; validate in prototype 07.*
- **Desk speakers with a separate mic.** This machine has USB Pebble speakers, a USB ATH-M50xSTS headset mic and an OBSBOT webcam mic. The speakers and mic are separate USB devices with separate clocks. With A, PipeWire resamples followers to the driver (aec-webrtc.cpp comment). With B, Lucy should open both streams through PipeWire, natively or via cpal's ALSA→pipewire path, so they land in one graph and get the same rate-matching. AEC3's delay estimator absorbs the constant offset.
- **Headphones.** There is barely any acoustic echo and AEC3 converges to near-passthrough. Keeping it on costs nothing. Bluetooth headsets add large, variable output latency, but that only matters if sound leaks from the cups. *Unverified edge case.*

**How barge-in detection fits.**
- The pipeline is mic → AEC (render ref = TTS PCM) → [optional NS] → {wake-word detector, VAD, STT}.
- VAD must be a separate component, because the APM in v2 does not populate `voice_detected`. Silero VAD or similar is the likely pick, and probably the same VAD the STT/endpointing work chooses.
- B's advantage: Lucy knows when TTS is playing. During playback, barge-in can demand more than "VAD says speech": a higher VAD probability, sustained speech of ~200–400 ms, and/or ERLE from `get_stats()` above a threshold so we know AEC has converged. Outside playback, normal VAD thresholds apply. With A, Lucy would only see a cleaned source and would have to infer playback state separately.
- On barge-in, stop TTS playback immediately (flush the playback buffer) and keep feeding the AEC the actual tail that went to the speaker, so residual echo of the last few hundred ms is still cancelled.
- Reference scope: B cancels only Lucy's own voice. Music or video playing on the machine is *not* cancelled and may trigger VAD or the wake word. A in `monitor.mode` would cancel everything. That is a v0.1 limitation to accept. A later option is capturing the default sink monitor as an extra render reference in-process.

## Sketch (in-process)

```text
TTS PCM ──► playback stream (PipeWire) ──► speaker
   └──────► aec.analyze_render_frame(10 ms chunks)
mic stream (PipeWire) ──► aec.process_capture_frame ──► wake word / VAD / STT
                                    ▲
               barge-in gate uses: is_speaking, VAD prob, duration, ERLE
```

Both streams should run at one rate (e.g. 48 kHz, or 16 kHz to match STT; APM accepts 8/16/32/48 kHz). Reframe to exactly 10 ms. Only Lucy writes to the playback stream, so the reference matches what is played.

## Open / unverified

- Real ERLE and residual-echo numbers on laptop speakers and on this desk setup: measure in the voice-loop prototype (07).
- Whether `sonora` holds up in practice (it is young). It would remove the C++ system dependency.
- Whether Piper is played by Lucy itself (needed for B) or by an external player. Lucy has to own the TTS PCM.
