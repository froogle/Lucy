# 28: A quiet voice can still Barge in

**What to build:** Barge-in by speech works when the user's voice reaches the mic weakly, from a low mic gain or a mic far from the mouth. In wayfinder 23, a voice at about −50 dBFS (normal is about −20) was mostly removed by AEC3's suppressor along with the echo during double talk, and both cancellers missed 2 of 3 talk-overs. The same recording boosted 12 dB caught 3 of 3, and boosted 24 dB caught them at ~290 ms.

Spec: Audio pipeline (AEC, VAD gate); TUI setup (mic test). Raised by [wayfinder 23](../../lucy-v0-1/issues/23-aec-on-real-speakers.md) (Answer, follow-up 2).

**Blocked by:** None (can start immediately)

**Seams:** uses `EchoCanceller`/`Sonora` (owned by 09, resolved) and `test.mic_level` (owned by 20, resolved)

**Status:** needs-triage

Open question for triage: the fix. Candidates are a capture pre-gain or AEC3's AGC2 ahead of the canceller, or a level check in `lucy setup`'s mic test that tells the user to turn the mic up, or both. Changing AEC3's `ep_strength.default_gain` or transparent mode made no difference.

- [ ] wayfinder 23's quiet-voice recording (`prototypes/aec-bench/runs/wrong-mic/desk-talk`) catches 3 of 3 talk-overs
- [ ] No new false Barge-ins on wayfinder 23's other recordings
- [ ] If setup checks the level: a mic too quiet for Barge-in is reported in plain words, with what to do

## Comments
