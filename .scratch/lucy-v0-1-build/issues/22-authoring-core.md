# 22: Authoring core

**What to build:** When no Plugin fits, Lucy builds one.
1. The fast tier clarifies what only the user knows, then calls `request_plugin{goal, clarifications}`.
2. lucyd spawns the advanced-tier Delegate through its native client. It runs under Landlock: read/write only to a staging build dir, toolchain caches and the CLI's own config dir; read-only system paths; open network; no user data. Its system prompt is the shipped authoring brief, and it starts from the shipped template folder (a minimal `rmcp` stdio server, a Manifest skeleton, a fixtures test harness).
3. The Delegate sees only `report_progress`, `ask_user`, `submit_plugin` and `cannot`.
4. `submit_plugin` builds the Plugin, checks it against `tools/list` and runs the Delegate's tests. On a pass Lucy speaks the Approval summary.
5. On a yes she answers the stored original request and asks "Is that the one?". A no is sent back as a fix through the update flow.
6. `cannot` makes Lucy state the limit plainly and offer the fallback.

Spec: Authoring. ADR 0002 (amendment).

**Blocked by:** 12, 14

**Status:** ready-for-agent

- [ ] Lucy says "This is going to take a few minutes" once, and the Session stays open in Listening
- [ ] Delegate has no Plugin or Built-in tools; it is identified by its per-spawn token
- [ ] Delegate can't read the user's data (Landlock test)
- [ ] `ask_user` is spoken only when neither side is speaking
- [ ] Failed build or tests: the Delegate retries or asks; when it gives up, Lucy says what failed
- [ ] Harness test with a scripted Delegate covering submit pass and fail, `cannot`, and the "Is that the one?" no → fix loop
