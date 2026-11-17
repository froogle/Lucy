# 12: Continue the last Hand-off

**What to build:** Claude often ends with an offer ("want me to apply the fix?"). "Tell Claude yes, go ahead" carries on that conversation instead of starting fresh. `hand_to_claude` takes `continue` (bool); the fast tier sets it when the user is replying to Claude's last result. lucyd keeps the most recent finished Hand-off's conversation id and folder in its state dir, so it survives a restart, and resumes it with `--resume <id>` in that folder, with a fresh budget. With nothing to continue, Lucy says "there's nothing to continue" and nothing starts. While a Hand-off runs, `continue` gets the queue offer like any second request.

Spec: [v0.2](../spec.md), Hand-off (Continuing). Story 49.

**Blocked by:** 05, 11

**Seams:** owns the `continue` field on `hand_to_claude` and the last-Hand-off record in the state dir. Uses the Hand-off queue (owned by 11).

**Status:** ready-for-agent

**Notes:**

- Settled in a grilling session on 2026-11-17 and split out of 05 the same day.
- The record holds an id and a folder, both local; neither goes on the public socket.

**Acceptance:**

- [ ] With the fake `claude`: `continue` starts it with `--resume <id>` in the recorded folder
- [ ] It works at a later Session and after a lucyd restart
- [ ] With no earlier Hand-off, Lucy says so and nothing starts
- [ ] While one runs, `continue` gets the queue offer
- [ ] Nothing about the id or folder reaches the bus (`assert_nothing_private_on_bus`)

## Comments
