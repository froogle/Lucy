# 26: Developer `say` command

**What to build:** A way to have lucyd speak a fixed sentence through its real TTS path, so an agent or developer can check the voice and playback without a Session or a model call (a real Session spends the user's subscription). A hidden `lucy say "<text>"` subcommand sends a developer-only IPC request; lucyd synthesises the text with the configured TTS Provider and plays it through the Speaker, with the start and closing sounds, and replies when playback ends. It refuses while a Session is open. It never reaches the model, and nothing it speaks is emitted on the public bus beyond the request itself.

Spec: Public IPC contract; Providers (Piper). Raised by the slice 3 retro: the end-of-slice announcement had to call Piper directly.

**Blocked by:** None (can start immediately)

**Seams:** extends the public IPC schema (`crates/lucy-ipc/src/schema.rs`) and `Speaker`

**Status:** needs-triage

- [ ] `lucy say "hello"` is heard through lucyd's Speaker in the configured voice
- [ ] Refused while a Session is open
- [ ] TTS unavailable gives a clear error, not a notification
- [ ] Harness test with the fake TTS
