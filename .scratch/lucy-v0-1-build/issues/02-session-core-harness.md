# 02: Session core via the harness

**What to build:** A whole Session runs end to end with fake adapters. `lucy listen` (IPC `session.start`) opens a Session in Listening. A scripted utterance becomes a transcript and goes to a fake Thinking Provider. The streamed reply goes to a fake TTS, and the Session moves Listening → Thinking → Speaking → Listening, with each change emitted as a `state` event. `lucy end` (IPC `session.end`) or the silence timeout ends the Session. The Session logic is a pure, I/O-free state machine inside lucyd. It is exercised only through the harness.

Spec: Session state machine; Testing Decisions.

**Blocked by:** 01

**Status:** ready-for-agent

- [ ] Fake adapters exist for capture/VAD speech segments, STT, Thinking (scripted stream with deltas, sentence ends and turn end) and TTS (records sentences)
- [ ] Thinking means a Turn is in progress with nothing playing; Speaking means TTS audio is queued or playing; one Turn can alternate between them
- [ ] Silence timer (~20 s, configurable) runs only in Listening and pauses in every other state; when it fires the Session ends quietly
- [ ] `session.start`/`session.end` work and `lucy listen`/`lucy end` wrap them; only one Session at a time
- [ ] "No speech recognised" returns to Listening without resetting the timer
- [ ] No transcript or spoken text ever appears in an IPC message (harness assertion reused by later tickets)
- [ ] Harness tests cover each transition above
