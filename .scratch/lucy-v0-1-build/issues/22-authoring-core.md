# 22: Authoring core

**What to build:** When no Plugin fits, Lucy builds one.
1. The fast tier clarifies what only the user knows, then calls `request_plugin{goal, clarifications}`.
2. lucyd spawns the advanced-tier Delegate through its native client. It runs under Landlock: read/write only to a staging build dir, toolchain caches and the CLI's own config dir; read-only system paths; open network; no user data. Its system prompt is the shipped authoring brief, and it starts from the shipped template folder (a minimal `rmcp` stdio server, a Manifest skeleton, a fixtures test harness).
3. The Delegate sees only `report_progress`, `ask_user`, `submit_plugin` and `cannot`.
4. `submit_plugin` builds the Plugin, checks it against `tools/list` and runs the Delegate's tests. On a pass Lucy speaks the Approval summary.
5. On a yes she answers the stored original request and asks "Is that the one?". A no is sent back as a fix through the update flow.
6. `cannot` makes Lucy state the limit plainly and offer the fallback.

Spec: Authoring. ADR 0002 (amendment).

**Blocked by:** 12, 14, 17

**Seams:** owns the Delegate tools on `Caller::Delegate` grants, the Delegate spawn through `Thinking` (for `codex` too if 18 has landed), the harness's scripted Delegate, and the build-running state in `session::Session` (also used by 23); uses the Approval flow and `tools/list_changed` (owned by 14); uses the Plugin fixtures test harness (owned by 17)

**Status:** resolved

- [ ] Lucy says "This is going to take a few minutes" once, and the Session stays open in Listening
- [ ] Delegate has no Plugin or Built-in tools; it is identified by its per-spawn token
- [ ] Delegate can't read the user's data (Landlock test)
- [ ] `ask_user` is spoken only when neither side is speaking
- [ ] Failed build or tests: the Delegate retries or asks; when it gives up, Lucy says what failed
- [ ] Harness test with a scripted Delegate covering submit pass and fail, `cannot`, and the "Is that the one?" no → fix loop

## Comments

Resolved on `lucy-v0-1-slice-5` (5cf704a..5caafa4). `crates/lucyd/src/authoring/`, template in `crates/lucyd/plugin-template/`, Delegate Landlock in `plugins/sandbox.rs`. User-approved choices: `request_plugin` also takes `name` and `request` (spec table updated); only an exact yes answers "Is that the one?"; the Delegate sandbox can read `/proc`; a second request during a build is refused (queueing is 23). With no Session open, a build's `ask_user`/`cannot`/give-up messages become notifications until 23 parks them; `report_progress` only logs (notifications are 23). Not yet run against real `claude` or `codex` as a Delegate, nor a full real build with `cargo` under the sandbox. Claude's other per-session files outside `projects/` (`file-history/<id>`, `session-env/<id>`) are not deleted.
