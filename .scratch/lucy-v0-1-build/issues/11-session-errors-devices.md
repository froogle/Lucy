# 11: Session errors and device handling

**What to build:** Failures are spoken plainly and never hang a Session.
- STT failure: "Sorry, I didn't catch that"; two in a row ends the Session.
- Thinking process dies: respawn once silently and resend the last user message. If that fails too, she says "I've lost my connection to Claude", shows a notification naming the Provider, and ends.
- Plan or usage limit: she speaks the limit and the reset time if known, then ends, with no switch to the other tier.
- Pinned device missing at Session start: fall back to the default with one notification, keeping the pin.
- Device lost mid-Session: the Session ends with a notification.

Spec: Session state machine (Errors); Audio pipeline (Devices).

**Blocked by:** 04, 05, 06

**Seams:** owns a failure result from `Stt::transcribe` (also used by 19); owns a `ThinkingEvent` for a dead Thinking process and its `FakeThinking` helper (also used by 09, 18); owns `[audio] speaker` and by-name device resolution through `AudioIo` (also used by 20); extends `Speaker` and `SpeakerEvent` (owned by 06, resolved) and `NotificationKind` (owned by 08, resolved)

**Status:** resolved

- [ ] Each error path above is covered by a harness test using the fakes
- [ ] A transient `error` state event is emitted (Indicator red flash)
- [ ] Never falls back silently to another Provider
- [ ] Pinned devices are re-resolved by name at each Session start

## Comments

Resolved on `lucy-v0-1-slice-3` (427912a..1339254). `Stt::transcribe` returns `Result<String, SttError>`; `ThinkingEvent::Died` with `FakeThinking::dies()`; `[audio] speaker` and `AudioIo::has_device`; `Tts::open` takes the speaker name (06's trait, needed for a pinned speaker). Problem notifications are `NotificationKind::Problem`. Respawn-once is per Turn. A death while a question is pending withdraws it silently (a pending Approval too). Live device unplugging, a real `claude` death and a real plan limit still need a person.
