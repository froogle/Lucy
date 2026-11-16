# 37: Barge-in isn't working

**What to build:** Talking over Lucy doesn't cut her off. Tickets 09, 27 and 28 built and tuned Barge-in, so this is a regression or a setup they didn't cover. Diagnose it with the `diagnosing-bugs` skill, then fix it.

**What the user sees (grilling session, 2026-11-16):**

- It happens every time.
- Her voice doesn't dip at all. The duck happens on the first frame above threshold, so the gate never fires.
- Saying "Hey Lucy" while she speaks doesn't cut her off either. **Both Barge-in paths are dead**, so the cause is probably something they share, not the gate's tuning.
- Setup: the ATH-M50xSTS USB headset is both the mic (`alsa_input.usb-Generic_ATH-M50xSTS-USB-00.mono-fallback`) and the speaker. That's the headphones path, where no echo reaches the mic.
- Speech-to-text is Superwhisper (`[stt] kind = "superwhisper"`). The Whisper model `base.en` is missing, as lucyd logs at every start.
- The running lucyd was built 2026-11-15 17:45, after 27, 28 and 33, so it isn't an old binary.

Spec: [v0.1](../../lucy-v0-1/spec.md): Session state machine (Barge-in), Audio pipeline, Further Notes (Barge-in regression).

**Blocked by:** none

**Seams:** none expected; say so in the report if the fix needs one.

**Status:** ready-for-agent

**Steps, in order:**

1. Download `base.en`, then repeat the talk-over test (speech and "Hey Lucy") with Superwhisper and with Whisper. 27 and 28 were probably tuned on Whisper. There's no Superwhisper branch in the Barge-in code, but check what differs for the mic stream and the DSP thread when Superwhisper hears.
2. Check the shared path: does lucyd know Lucy is speaking (the gate's speaking flag), and does the DSP thread still get mic frames while she speaks?
3. Add a debug log of the gate's inputs (VAD probability, ERLE, whether it counts as settled, the pre-gain, the speaking flag) behind `RUST_LOG`, and keep it. The user then does one recorded run of talking over her, which is replayed through `tools/aec-bench`.
4. Check the Silero change from 33 (sr folded, Level3 optimisation), the newest change to what the gate hears.

A harness test that reproduces the cause comes before the fix.

**Acceptance:**

- [ ] The cause is found and named, with a failing harness test that the fix makes pass
- [ ] Redeployed (stop, reinstall all four binaries, restart): on the headset, talking over her ducks and stops her, and "Hey Lucy" over her cuts in, with both Superwhisper and Whisper

## Comments

2026-11-16: grilled with the user; the details are in the body above.
