# 01: Hand a request to Claude

**What to build:** "Tell Claude to …" starts a Hand-off. That is a Delegate running the user's own `claude`, with their config, MCP servers and permission rules, in the folder they name (or `~`). Lucy speaks Claude's permission requests as Confirmations, raises its questions aloud or in the Input dialog, and speaks a short summary when it's done, with the full answer in a notification. It keeps working after the Session ends, and the transcript is kept for `claude --resume`.

Spec: [v0.2](../spec.md), Hand-off. ADR 0007.

**Blocked by:** lucy-v0-1-build 24 (v0.1 acceptance), lucy-v0-1-build 34 (Input dialog: the Delegate `ask_typed`)

**Seams:** owns the Built-in tool `hand_to_claude`, the Hand-off Delegate kind, the permission tool, the `[handoff]` config section and the "working" `state` flag. Uses the Delegate tools and the keep-for-next-Session path (Authoring), and `ask_typed` and the Input dialog (owned by lucy-v0-1-build 34).

**Status:** ready-for-agent

**Acceptance:**

- [ ] "Tell Claude to …" starts the user's own `claude` with their settings sources and MCP config, `--permission-prompt-tool` pointing at Lucy, the subscription-guard environment, and the resolved folder
- [ ] Folders: one match is used; several matches are asked about; none is said plainly and nothing starts; absolute and `~` paths are used as given; no folder means `~`
- [ ] A permission request is spoken as "Claude wants to … Okay?" with the full input in a notification; yes allows, and no or silence denies
- [ ] `ask_user` is spoken when nobody is speaking; `ask_typed` opens the Input dialog
- [ ] When it finishes, a one-to-three-sentence summary is spoken and the full answer is notified; after Session end it is offered at the next Session, and parked questions are too
- [ ] The transcript is kept; the final notification (done, cancelled or over budget) carries `claude --resume <id>` and the folder
- [ ] One at a time with a queue of one, separate from Authoring; "cancel it" asks which one when both are running; there is a 60 min budget (configurable)
- [ ] Nothing about the request, folder, tool input or result reaches the public socket
- [ ] Once, by hand with real `claude`: "tell Claude to list my Linear tickets" (or another integration the user has), and a coding request in `~/develops/lucy`

## Comments

Moved from lucy-v0-1-build ticket 38 after the 2026-11-16 grilling session.
