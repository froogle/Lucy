# Does Lucy's audio stack coexist with the desktop?

Type: task
Status: resolved
Blocked by: none

## Question

With the voice-loop prototype running: does Whisper on Vulkan slow down or fail when Superwhisper's resident Vulkan workers hold the same GPU? Does `pw-top` show Lucy's capture and playback streams under one driver (the drift-correction premise in [How should Lucy own microphone capture and speaker playback from Rust?](13-audio-io.md))? What happens when a pinned input or output device disappears mid-Session, given cpal has no hot-plug events? Record facts the Session error handling and the `AudioIo` seam depend on.

## Answer

Measured against the deployed lucyd (the voice-loop prototype is superseded). Full write-up: [research/audio-stack-coexistence.md](../research/audio-stack-coexistence.md); probes and raw numbers in `prototypes/audio-coexistence/`.

- **GPU: no problem.** Superwhisper's resident Vulkan workers (~3 GiB VRAM) don't slow Whisper `small.en` (p50 103 ms resident vs 102 ms unloaded, 5.6 s utterance). With both transcribing at once, Whisper goes to p50 110 / p95 135 ms and Superwhisper from ~196 to ~232 ms. Nothing failed, and every transcript was correct. No change.
- **One driver: yes.** Both lucyd streams carry `node.group = cpal-<pid>`. `pw-top` shows capture, playback (opened only while speaking) and the Pebble sink all following the pinned ATH-M50x mic's driver, across two USB devices. The drift-correction premise in 13 holds.
- **Device loss: a bug.** cpal 0.18.2 on PipeWire reports a pinned node's removal as `StreamInvalidated` ("target not found"), which lucyd's `is_device_lost` ignores. With the mic's card switched off, lucyd went deaf with no notification or reopen, and stayed deaf after the device returned. Mid-Session, the Session ended on its timer rather than through `error`. A lost pinned speaker hung a Voice preview, since `Played` never arrives. A `"default"` stream instead gets `DeviceChanged` and follows the new default, which is fine. Build ticket [39](../../lucy-v0-1-build/issues/39-device-loss-stream-invalidated.md). The spec's device rules stand; only the classification is wrong.
- **For the seam:** treat `StreamInvalidated | DeviceNotAvailable | HostUnavailable` as lost. There is no "came back" event, so reopening polls, as lucyd's 1 s reopen loop already does once triggered. Anything waiting on playback progress needs the lost path to release it.
- Not done: a physical unplug (the card profile was switched off instead). Ticket 39's acceptance includes one.
