# 47: No notification when the pinned mic goes missing outside a Session

**What to build:** With the mic pinned to the USB headset, switching its card off outside a Session gives no notification. lucyd logs "the microphone has gone away" and falls back to the default mic, which hears the Wake word fine. But the Session that then starts doesn't send the spec's "Lucy is using the default device" notification. The same card-off during a Session ends the Session and does notify. Find out why, then fix it.

What the hand check showed (2026-11-16, after ticket 39 was deployed):

- 18:47:24, card off: `microphone error: target not found`, then `the microphone has gone away`.
- 18:47:43, a Session starts on the Wake word, heard through the OBSBOT camera mic. No notification. A test `notify-send` run from systemd's environment does show, so delivery works.
- `runner/start.rs` notifies `devices_missing` when `Hearing::configure` finds the pin missing. That goes through `resolve`, which calls `AudioIo::has_device` (cpal device enumeration). The leading hypothesis is that `has_device` still listed the removed node. The reopen loop (`REOPEN_MIC_EVERY`) might also have changed `Hearing`'s state first.

Spec: [v0.1](../../lucy-v0-1/spec.md): Audio pipeline, Devices ("A missing pin falls back to the default with one notification and keeps the pin").

**Blocked by:** none

**Seams:** none expected.

**Status:** ready-for-agent

**Acceptance:**

- [ ] The cause is named, and a harness test that fails before the fix reproduces it
- [ ] Redeployed, with the mic pinned: switching its card off outside a Session gives the notification, either on the loss or at the next Session start, once

## Comments
