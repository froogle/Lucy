# 01: Hand off a request to Claude

**What to build:** "Tell Claude to list my Linear tickets" starts a Hand-off: the user's own `claude`, with their own settings sources and MCP config, in the folder they gave as an absolute or `~` path, or in `~` when they gave none. In this first slice, anything their Claude settings don't already allow is **denied**: the permission tool answers no to every request, which is safe from day one. Spoken permission requests are 02. When Claude's turn ends, the fast tier speaks a one-to-three-sentence summary and the full answer goes to a notification. The transcript is kept, and the notification carries `claude --resume <id>` and the folder.

Spec: [v0.2](../spec.md), Hand-off. ADR 0007. Stories 1–3, 6–8, 12, 16, 17, 20, 21, 26–28.

**Blocked by:** lucy-v0-1-build 24 (v0.1 acceptance)

**Seams:** owns the `hand_to_claude` Built-in tool, the Hand-off Delegate kind, the permission tool (02 makes it ask), the `[handoff]` config section, the login-shell environment capture and the rule that Lucy's transcript deletion skips a Hand-off. Uses the Delegate relay and tokens, and the subscription guard.

**Status:** resolved

**Notes:**

- Settled in a grilling session (2026-11-16) and split into tracer slices by `to-tickets` the same day. Moved from lucy-v0-1-build tickets 38 and 41.
- **How it runs:** in Claude's default permission mode, with `--permission-prompt-tool` pointing at Lucy's permission tool. Lucy's relay is added as an extra MCP server beside the user's own.
- **Not sandboxed:** it is not under Landlock. The subscription guard's environment scrub still applies.
- **Environment (2026-11-17):** at each Hand-off's start, lucyd captures `$SHELL -lic 'env -0'`, removes the billing variables itself and starts `claude` directly with that environment, so a shell startup file can't undo the guard. A capture that fails or takes over 5 s falls back to lucyd's own environment, logged without values. Story 50.
- **While it runs:** the Session stays open, Listening with the silence timer paused. If the Session ends first, the Hand-off is stopped; carrying on past the Session is 05.
- **Folders:** a bare folder name is refused with "say the full path" until 03.

**Acceptance:**

- [ ] With the fake `claude` (`Harness::with_thinking`): the real adapter starts `claude` with the user's settings sources, `--permission-prompt-tool`, the relay as an extra MCP server, the guard environment, and the resolved folder
- [ ] In the harness: every permission request is denied; the summary is spoken and the full answer notified
- [ ] The transcript is left in place; the notification carries the resume command and folder
- [ ] With a fake `$SHELL`: a token its `env -0` sets reaches `claude`; an `ANTHROPIC_API_KEY` it sets doesn't; a hanging shell falls back after 5 s
- [ ] Nothing about the request, folder, tool input or result reaches the public socket
- [ ] Once, by hand with real `claude`: "tell Claude to list my Linear tickets" (or another integration the user has)

## Comments

2026-11-17: built in slice 1 (`lucy-v0-2-slice-1`). The Hand-off always runs `claude` with the subscription guard, ignoring `[tiers]` and the `program` override. There's no `[handoff]` section yet, since nothing in 01 needs a setting. A second request is refused until 11. A dead Hand-off notifies its exit status and last stderr line. Hand check left: "tell Claude to list my Linear tickets" with real `claude`.
