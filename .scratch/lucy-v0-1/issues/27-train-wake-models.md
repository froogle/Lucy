# Train and commit the "Hey Lucy" and "Lucy" wake-word models

Type: task
Status: open
Blocked by: 16

## Question

Train the `hey_lucy.onnx` (default) and `lucy.onnx` (alternative) models with LiveKit WakeWord v2 on a rented CUDA GPU, using a free LiveKit Cloud project for the TTS step. Commit them under `models/wake/` with the training config, the exact command, the LiveKit WakeWord version and a reproduction README. This is HITL: the maintainer provides the GPU rental and the LiveKit account. Record where the models live and how long training took. Unblocks the soak test, which needs the real models. See [What is the default wake phrase, and how does a user get a different one?](16-wake-phrase.md).
