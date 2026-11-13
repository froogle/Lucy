# Does Lucy's audio stack coexist with the desktop?

Type: task
Status: open
Blocked by: none

## Question

With the voice-loop prototype running: does Whisper on Vulkan slow down or fail when Superwhisper's resident Vulkan workers hold the same GPU? Does `pw-top` show Lucy's capture and playback streams under one driver (the drift-correction premise in [How should Lucy own microphone capture and speaker playback from Rust?](13-audio-io.md))? What happens when a pinned input or output device disappears mid-Session, given cpal has no hot-plug events? Record facts the Session error handling and the `AudioIo` seam depend on.
