# Train and commit the "Hey Lucy" and "Lucy" wake-word models

Type: task
Status: open
Blocked by: 16

## Question

Train the `hey_lucy.onnx` (default) and `lucy.onnx` (alternative) models with LiveKit WakeWord v2 on a rented CUDA GPU, using a free LiveKit Cloud project for the TTS step. Commit them under `models/wake/` with the training config, the exact command, the LiveKit WakeWord version and a reproduction README. This is HITL: the maintainer provides the GPU rental and the LiveKit account. Record where the models live and how long training took. Unblocks the soak test, which needs the real models. See [What is the default wake phrase, and how does a user get a different one?](16-wake-phrase.md).

## Comments

2026-11-13 (user decision): no rented cloud GPU. Train on the maintainer's Apple-silicon Mac ("wawa"), which ticket 16's research lists as supported; Claude sets the Mac up for the LiveKit WakeWord v2 recipe when this ticket is picked up. Once trained, `hey_lucy.onnx` ships as the bundled default, replacing the interim `crates/lucyd/models/wake/hey_livekit.onnx`. Until then, the wake phrase is "Hey LiveKit".
