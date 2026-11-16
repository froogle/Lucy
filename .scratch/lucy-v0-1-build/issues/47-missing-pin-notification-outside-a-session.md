# 47: Notify when the pinned mic goes missing outside a Session

**What to build:** When the pinned mic disappears outside a Session, lucyd falls back to the default mic quietly and only notifies "Lucy is using the default device" at the next Session start. That notification lasts 8 s, while the user is talking to Lucy, so in the 2026-11-16 hand check it went unseen. Diagnosis (see Comments) found no bug: the notification was sent and expired as normal.

User decision (2026-11-16): notify at the moment of the loss instead. When the pinned mic is lost outside a Session, lucyd shows the "Lucy is using the default device" notification straight away, and doesn't repeat it at the next Session start while the pin is still missing. If the pin comes back and goes again, that's a new loss and a new notification. A pin found missing at a Session start without an earlier loss notification (for example, lucyd started with it unplugged) still notifies there, as today. A loss during a Session keeps its current path (the Session ends through `error` with its own notification).

Spec: [v0.1](../../lucy-v0-1/spec.md): Audio pipeline, Devices ("A missing pin falls back to the default with one notification and keeps the pin").

**Blocked by:** none

**Seams:** none expected.

**Status:** resolved

**Acceptance:**

- [ ] Harness: the pinned mic lost outside a Session notifies at once; the next Session start doesn't notify again; a pin missing at start with no earlier loss still notifies once
- [ ] The spec's Devices line says when the notification comes
- [ ] Redeployed, with the mic pinned: switching its card off outside a Session shows the notification straight away, and the next Session doesn't repeat it

## Comments

2026-11-16: diagnosed (no lucyd bug): the Session-start notification was sent at 18:47:43.785 and expired after 8 s (Omarchy notification history). `has_device` and `Hearing::configure` report the missing pin correctly on the real headset. Reworked by user decision: notify on the loss.

2026-11-16: hand-checked on the redeployed build; passed.
