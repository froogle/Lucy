# Train and commit the "Hey Lucy" and "Lucy" wake-word models

Type: task
Status: open
Blocked by: 16

## Question

Train the `hey_lucy.onnx` (default) and `lucy.onnx` (alternative) models with LiveKit WakeWord v2 on a rented CUDA GPU, using a free LiveKit Cloud project for the TTS step. Commit them under `models/wake/` with the training config, the exact command, the LiveKit WakeWord version and a reproduction README. This is HITL: the maintainer provides the GPU rental and the LiveKit account. Record where the models live and how long training took. Unblocks the soak test, which needs the real models. See [What is the default wake phrase, and how does a user get a different one?](16-wake-phrase.md).

## Comments

2026-11-13 (user decision): no rented cloud GPU. Train on the maintainer's Apple-silicon Mac ("wawa"), which ticket 16's research lists as supported; Claude sets the Mac up for the LiveKit WakeWord v2 recipe when this ticket is picked up. Once trained, `hey_lucy.onnx` ships as the bundled default, replacing the interim `crates/lucyd/models/wake/hey_livekit.onnx`. Until then, the wake phrase is "Hey LiveKit".

2026-11-14: "Hey Lucy" trained on wawa (M5 Pro) and bundled as `crates/lucyd/models/wake/hey_lucy.onnx`, replacing `hey_livekit.onnx`; how to reproduce it, the versions and the scores are in the README beside it (score 0.822; at threshold 0.5, recall 0.874 at 0.99 false alarms/h, 15.2% of sound-alikes accepted). The models live next to lucyd's other bundled models, not in a top-level `models/wake/`. `positive.wav` is now the maintainer saying "Hey Lucy" (score 0.79 in lucyd's engine). The pinned `livekit-wakeword` rev loads models from the 2.1.2 trainer.

Still open: plain "Lucy". Its speech run used up the LiveKit free plan's $2.50 of Inference credit for November: 956 of 1,150 training clips were written before every request got HTTP 429. Rerun `setup --wakeword "lucy" --eval`, `train` and `eval` in `~/lucy-wake` on wawa on or after 2026-12-01; the clips already made are cached in `.cache/lkwb` and are reused. Then commit `lucy.onnx` beside `hey_lucy.onnx` and add it to the README.

2026-11-15: live check on the dev machine (ATH-M50xSTS headset mic), with lucyd redeployed: "Hey Lucy" opened a Session first time and it ran three exchanges. "Hey Lucky" didn't open a second Session (one Session start in `lucy events`), so whether it would wake Lucy from idle is still unknown; the soak test (wayfinder 24) should try sound-alikes from idle.
