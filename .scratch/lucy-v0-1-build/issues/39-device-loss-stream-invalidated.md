# 39: A lost device goes unnoticed on PipeWire

**What to build:** when a pinned mic or speaker disappears, cpal 0.18.2's PipeWire host reports `ErrorKind::StreamInvalidated` ("target not found"), not `DeviceNotAvailable`. `audio::cpal::is_device_lost` only matches `DeviceNotAvailable | HostUnavailable`, so lucyd's lost-device path never runs. With the mic pinned to a USB headset, switching the headset off left lucyd deaf, with no notification, no reopen, and still deaf once the headset was back, until a restart. A pinned speaker that disappears stops pulling samples, so `Played` never arrives: a Voice preview hung, and a Session would stay in Speaking. Count `StreamInvalidated` as lost, for both the mic and the speaker. Measured in wayfinder 25 ([research](../../lucy-v0-1/research/audio-stack-coexistence.md)).

Spec: none; wayfinder 25.

**Blocked by:** none

**Seams:** none (`AudioIo` and `SpeakerEvent::DeviceLost` unchanged).

**Status:** resolved

**Notes:**

- **Where:** `crates/lucyd/src/audio/cpal.rs` `is_device_lost`, used by the mic's error callback there and by `playback.rs`'s `open_output`. A `StreamInvalidated` stream is dead whatever the cause, so ending the Session and reopening is right for any cause.
- **Leave alone:** `DeviceChanged` on a `"default"` stream. The stream follows the new default and keeps playing, which is what we want.
- **Why the tests pass today:** the harness's `mic.lose()` and `lose_device()` signal loss above cpal, so nothing checks which cpal kinds count as lost. A unit test on `is_device_lost` covers that.
- **To reproduce without unplugging:** `pactl set-card-profile <card> off` and back (`pactl list cards short`; note the active profile first). For a speaker, pin `[audio] speaker` to a `pactl load-module module-remap-sink master=<sink> sink_name=probe` and `pactl unload-module` it during `lucy say`. Probes: `.scratch/lucy-v0-1/prototypes/audio-coexistence/`.

**Acceptance:**

- [ ] `is_device_lost` counts `StreamInvalidated`, `DeviceNotAvailable` and `HostUnavailable`, and not `DeviceChanged`, checked by a unit test
- [ ] Redeployed, with the mic pinned: switching its card off outside a Session gives the lost-microphone notification, Lucy hears the Wake word through the default mic within a few seconds, and after switching the card back on, the next Session hears through the pinned mic again
- [ ] Redeployed, with the mic pinned: switching its card off in a Session ends it through `error` with the notification
- [ ] Redeployed, with the speaker pinned to a remap sink: unloading it during `lucy say` ends the preview with "the speaker went away" rather than hanging
- [ ] Once, a physical unplug of the USB headset mid-Session behaves like the card-off run (wayfinder 25 only switched the profile)

## Comments

2026-11-16: built in slice 8 (`lucy-v0-1-slice-8`). Hand checks left: the redeployed card-off runs (outside and in a Session), the remap-sink speaker run, and one physical unplug.

2026-11-16 hand checks (redeployed main): all passed except the outside-Session notification, now ticket 47. Card off outside a Session: lucyd fell back to the camera mic and heard the Wake word. Card off and a physical unplug mid-Session: the Session ended with the notification. The remap-sink speaker: "the speaker went away". After replugging: heard through the pinned mic again.
