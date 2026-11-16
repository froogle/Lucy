# 38: Hand a request to Claude

**What to build:** The user asked Lucy to send a request to Claude in `~/develops/lucy` and she said she couldn't. Lucy should be able to pass on "tell Claude to …" requests to Claude Code as a Delegate, and report back what it did. Some requests are about a folder the user names ("tell Claude to fix the failing test in lucy"). Others need no folder but use the user's own Claude integrations over MCP ("tell Claude to grab my Linear tickets", "tell Claude to check my email"). Those integrations are what make this most useful, especially for users who have connected Claude to other systems.

This conflicts with the spec as it stands. Lucy's own `claude` runs with an isolated config (no setting sources, strict MCP config, no built-in tools). The authoring Delegate is sandboxed away from user data. Raw shell access for the model is out of scope. A "tell Claude" Delegate would need the user's real Claude config, MCP servers and possibly shell access, so it can't use either of those setups. The grilling needs to decide where it sits.

Still to settle:
- Whether this is a Plugin or a Built-in tool, and how it relates to wayfinder 04 (Herdr) and the Delegates in tickets 22 and 23 (Authoring).
- Which Claude config the Delegate runs with (the user's own, or a chosen subset of MCP servers), and what permission mode it uses: who answers when Claude wants to run a command or send something.
- Which requests need Confirmation before handing off. Reading Linear is not the same as sending an email.
- How results come back: spoken summary, on screen, or both. The same goes for long-running work that outlasts the Session, and for questions Claude asks mid-task (perhaps through ticket 34's dialog).
- What happens to the transcript afterwards. Lucy deletes her own `claude` transcripts; a user-facing hand-off might be worth keeping.
- Whether "Codex" (ticket 18) gets the same hand-off.

Details to be settled in a grilling session.

**Status:** resolved (moved to lucy-v0-2 01–05 (from [01](../../lucy-v0-2/issues/01-hand-off-to-claude.md)))

## Comments

2026-11-16: the user widened the scope beyond a project folder. Lucy refused a "tell Claude to …" request again yesterday. The user wants hand-offs that use their Claude MCP integrations (Linear, email), and sees this as especially important for users with such integrations.

2026-11-16: settled in a grilling session and moved to v0.2 as lucy-v0-2 01–05 (from [01](../../lucy-v0-2/issues/01-hand-off-to-claude.md)), spec [v0.2](../../lucy-v0-2/spec.md), ADR 0007. The questions above are answered there.
