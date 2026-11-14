# How well does echo cancellation work on real speakers, and does sonora match the C++ library?

Type: prototype
Status: resolved
Blocked by: none

## Question

Extend the voice-loop prototype with in-process AEC3 (see [How do we get echo cancellation for voice interruption with PipeWire, from Rust?](02-echo-cancellation.md)), with Lucy's own TTS as the reference. Measure echo reduction (ERLE) and residual echo on laptop speakers, desk speakers and headphones. Check whether the pure-Rust `sonora` matches the C++ `webrtc-audio-processing` 2.x, and whether the barge-in gate from [Which voice-activity detector should Lucy use, and how strict is the barge-in gate?](14-vad-barge-in.md) holds without false interrupts on each setup.

## Answer

**Both cancellers work on real speakers and behave alike; keep `sonora`. The `[barge_in]` defaults held on every setup, with no false Barge-in, but two things need follow-up: the gate never relaxes on headphones, and a weak voice at the mic gets suppressed during double talk.** This is a desktop, so the setups were desk speakers (Pebble V3 with the OBSBOT Tiny 2 Lite webcam mic) and headphones (ATH-M50xSTS USB headset, its boom mic). There are no laptop speakers to test.

**Method.** A throwaway bench (`tools/aec-bench/`, README there) plays ~38 s of Piper `cori-medium` news briefing through the default speaker while recording the default mic. It opens both the way lucyd does (mic mono 16 kHz, speaker mono at the voice's rate) and places the 16 kHz reference on the mic's timeline from the streams' timestamps (measured echo delay 44 ms after placement, within AEC3's range). Each recording then runs offline through no canceller, lucyd's own `Sonora`, and the C++ `webrtc-audio-processing` 2.1 (bundled build, AEC3 only, configured like lucyd's), then lucyd's Silero and a copy of lucyd's gate with default `[barge_in]`. "Silent" runs have the user quiet; "talk" runs prompt the user to talk over Lucy three times. Volumes were the user's normal ones (speakers 45%, headphones 44%, mics ~75–95%).

| Setup | Canceller | Echo removed (after 2 s / first 2 s) | Own ERLE median, reaches 10 dB at | Max VAD on residual (longest ≥ 0.7) | False duck / Barge-in | Talk-overs caught (latency) |
|---|---|---|---|---|---|---|
| Desk, silent | none | – | – | 1.00 (5 s) | 117 / 97 | – |
| | sonora | 30.2 / 23.1 dB | 13.8 dB, 3.0 s | 0.98 (160 ms) | 1 / 0 | – |
| | webrtc | 26.0 / 28.6 dB | 14.2 dB, 4.5 s | 0.80 (32 ms) | 0 / 0 | – |
| Desk, talk | none | – | – | – | 56 / 46 | – |
| | sonora | 47.9 / 44.0 dB | 13.2 dB, 4.4 s | 0.54 (0) | 0 / 0 | 3/3 (290, 290, 280 ms) |
| | webrtc | 26.6 / 31.2 dB | 12.8 dB, 5.9 s | 0.79 (32 ms) | 1 / 0 | 3/3 (320, 290, 280 ms) |
| Headphones, silent | none | – | – | 0.03 | 0 / 0 | – |
| | both | ~7 dB (no echo to remove) | 0.2 dB, never | 0.01 | 0 / 0 | – |
| Headphones, talk | none | – | – | 0.02 | 0 / 0 | 3/3 (290 ms each) |
| | both | – | 0.2 dB, never | 0.00 | 0 / 0 | 3/3 (480, 510, 480 ms) |

"Echo removed" is mic power over output power while Lucy speaks and the user doesn't. It counts the suppressor too, so it is more than the linear filter's ERLE. Latency runs from the first VAD ≥ 0.5 in a talk window to the Barge-in.

**sonora vs the C++ library.** They match in kind on every run: same convergence (3–6 s to 10 dB), same zero false Barge-ins, same talk-over latencies, and the same failure on a weak voice (below). The differences are small and go both ways: sonora removed more echo once converged (30 vs 26 dB, 48 vs 27 dB), and the C++ library removed more in the first 2 s of one run (28.6 vs 23.1 dB). There's nothing here to justify the C++ system dependency: keep `sonora`.

**The gate defaults hold.** No false Barge-in on any setup with either canceller. The only residual-echo leak that came near the threshold (sonora, VAD 0.98 for 160 ms) was 1.3 s into Lucy's first sentence, before convergence, and the stricter unconverged bar turned it into a duck only. Once converged, residual echo stayed below VAD 0.66 (sonora) and 0.80 (webrtc). On speakers, talk-over latency (~290 ms) is the gate's `sustain_ms`, not the canceller: the user's voice comes through intact. Tightening isn't needed; loosening `threshold` would eat into a margin of about 0.04–0.16 above the worst converged residual.

**Follow-up 1: the gate never relaxes on headphones.** With no echo, AEC3's ERLE stays at 0.2 dB, so `converged_erle_db` (10) is never met and the gate stays at `unconverged_threshold`/`unconverged_sustain_ms` (0.85 for 500 ms) for the whole Session. Talk-overs take ~490 ms instead of ~290 ms. sonora's `echo_return_loss` tells the cases apart in these runs: it stays at its initial −30 dB for a whole headphone run, and reaches 17–20 dB (its upper clamp) within 5–7 s of Lucy speaking on the speakers. That is a sentinel, not a measurement, so the rule for "no echo path, count as converged" (that ERL, AEC3's transparent mode, or measured coupling) is a design decision for a build ticket.

**Follow-up 2: a weak voice is suppressed during double talk.** On two runs the user's voice was far below normal at the mic (about −50 dBFS, against −20 dBFS normally; the boom mic not in use, recorded by mistake, kept in `runs/wrong-mic/`). AEC3's suppressor then removed most of it along with the echo (VAD ≥ 0.5 in 14 of ~75 speech chunks), and both cancellers missed 2 of 3 talk-overs while the no-canceller path caught them all. Boosting that recording by 12 dB brought it to 3/3; by 24 dB, 3/3 at ~290 ms. Changing AEC3's `ep_strength.default_gain` or transparent mode made no difference. Lucy can't rely on every mic's gain: a capture pre-gain or AGC2 ahead of the gate, or a level check in `lucy setup`'s mic test, would cover it. The same happened when the user talked from Lucy's first word without a pause: the canceller never converged (ERLE 2–3 dB) and the voice was suppressed. The Wake word still interrupts in both cases.

**For [Does Silero run on pure-Rust ort-tract, and does earshot match it?](26-vad-runtime-ab.md):** every run keeps `mic.wav`, `ref.wav` and `residual-sonora.wav` / `residual-webrtc.wav` (16 kHz f32) under `tools/aec-bench/runs/`. The residuals are the echo-cancelled TTS that ticket asks for; the talk runs include the user's speech at known prompts (`meta.txt`).

**For the acceptance run (build 24):** "Barge-in without false interrupts on laptop speakers, desk speakers and headphones" is met for desk speakers and headphones on this machine with defaults unchanged. There are no laptop speakers here to test.
