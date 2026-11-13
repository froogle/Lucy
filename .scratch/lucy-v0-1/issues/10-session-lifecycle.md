# How does a Session move between listening, thinking and speaking, including interruptions?

Type: grilling
Status: resolved
Blocked by: 01, 02, 14, 16

## Question

Define the Session state machine: wake → listening → thinking → speaking → listening, barge-in transitions, end conditions (Indicator click, closing phrase, silence timeout), what the Indicator shows in each state, and what happens on errors (Provider down, no speech recognised). Also decide whether any sustained speech interrupts Lucy, or whether hearing the wake word during TTS is the (or an instant) interrupt.

## Comments

- From [Which tools does Lucy provide in v0.1, and which need a spoken "yes"?](09-tool-catalogue.md) (2026-11-12): the state machine needs a **confirming** state. A tool call waits while Lucy speaks the Confirmation prompt, then she listens for one utterance and matches it locally. Saying "yes" while she is still speaking the prompt counts, and 8 s of silence means no. The closing phrase can also arrive as the model calling the `end_session` Built-in tool.
- From [How does Lucy write a new Plugin when nothing can handle a request?](20-plugin-authoring-loop.md) (2026-11-13): amendment. While an Authoring build runs, the Session sits in Listening with the silence timer **paused** (the build budget, 20 min, caps it), and the Indicator adds a "building" mark. Ending the Session does not cancel the build.

## Answer

Grilled with the user on 2026-11-12. Glossary: added **Turn**, **Barge-in**, **Closing phrase**. No ADR (nothing here is hard to reverse).

**States.** Idle (no Session), Listening, Thinking, Speaking, Confirming. Thinking and Speaking follow *what the user hears*, not the model's stream:
- Speaking: TTS audio is queued or playing.
- Thinking: a Turn is in progress and nothing is playing, tool calls included.

So one Turn can go Thinking → Speaking → Thinking → Speaking (e.g. "Let me check…", then a tool call, then the answer).

**Session start.**
- On the Wake word: a short start sound, the Indicator appears in Listening, and the model process is pre-spawned (03).
- Audio straight after the Wake word is kept, so "Hey Lucy what's the time" in one breath works.
- If no speech at all follows the Wake word, the Session ends after **8 s**, so false triggers clear quickly.

**Wake word inside a Session.**
- While Listening it is ordinary speech, and a leading wake phrase is stripped from the transcript.
- There is only ever one Session.

**Barge-in.**
- While Speaking, any sustained speech (14's gate: p ≥ 0.7 for 300 ms, stricter while AEC is unconverged) is a Barge-in. Hearing the Wake word during Speaking interrupts **instantly**, without waiting the 300 ms.
- Config `barge_in = "speech" | "wake_word"` (default `speech`) lets users with poor echo cancellation limit Barge-in to the Wake word (pending 23).
- On Barge-in Lucy:
  1. stops and flushes TTS;
  2. sends the stream-json control interrupt if the model is still generating (SIGINT + respawn fallback, 07);
  3. moves to Listening and sends the new utterance as the next user message, prefixed with `[You were interrupted; the user heard up to: "…last spoken sentence…"]`.
- If the Barge-in produces no transcript (a cough, empty speech-to-text), Lucy goes back to Listening silently. Resuming the cut-off reply stays in the fog.
- Speech while Thinking is also a Barge-in: the Turn is interrupted and the new utterance sent. The model still has the earlier message, so it answers both.
- Tool calls in flight: unconfirmed calls get an MCP cancellation. **Confirmed calls run to completion**, and their result is reported in the next Turn.

**Confirming** (from 09).
- The tool call waits while Lucy speaks the prompt. She then listens for one utterance and matches it locally.
- A "yes" spoken during the prompt counts. 8 s of silence means no.
- An Indicator click counts as declined and ends the Session.

**Ending a Session.**
- **Indicator click:** from any state, immediately. TTS stops, the model is interrupted and unconfirmed tools are cancelled.
- **Closing phrase:** only through the model calling `end_session`; there is no local phrase list. Lucy finishes speaking the model's text ("Bye!"), then ends.
- **Silence timeout (~20 s, configurable):** the timer runs **only in Listening** and pauses while Thinking, Speaking or Confirming. When it fires, Lucy ends quietly: a soft closing sound, the Indicator fades, nothing spoken.

**Indicator** (Omarchy theme colours, 06):

| State | Shows |
|---|---|
| Idle | hidden |
| Listening | slow pulse, accent colour |
| Thinking | spinner / orbiting dot |
| Speaking | level meter driven by TTS output |
| Confirming | warning colour, pulsing; full arguments also in a notification (09) |
| Error | brief red flash, then the next state |

The GNOME notification fallback shows only start, Confirming and end.

**Errors.**
- **No speech recognised:** back to Listening silently. The silence timer is not reset.
- **STT Provider fails:** "Sorry, I didn't catch that", then keep listening. After two failures in a row, Lucy says it's broken and ends the Session.
- **Thinking process dies:** respawn once silently and resend the last user message. If that fails, Lucy says "I've lost my connection to Claude", shows a notification naming the Provider, and ends.
- **Plan or usage limit hit:** Lucy speaks the limit and the reset time if known, then ends. There is no automatic switch to the other Model tier.
- **TTS Provider fails:** replies go to a notification as text (17's text-only fallback), and the Session carries on.

**Long work.**
- No runtime fillers (07). The Indicator shows Thinking, and the persona prompt asks the model to say a short "Let me check" before slow tools.
- Whether a Plugin-authoring Delegate keeps the Session open is left to 20 (noted there).
