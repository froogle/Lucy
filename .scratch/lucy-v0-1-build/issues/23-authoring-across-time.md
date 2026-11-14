# 23: Authoring across time

**What to build:** Builds behave well over minutes and across Sessions.
- While a build runs, the silence timer is paused and the Indicator shows a "building" mark.
- `report_progress` goes to notifications only (config toggle, default on), never the TUI, never spoken.
- Each build has a 20 min budget (configurable), and the staging dir is kept so "try again" resumes.
- One build at a time, with a queue of one ("I'm already building the photos Plugin, I'll do this after").
- Ending the Session doesn't stop a build: Lucy says she'll keep working. When it finishes she sends a notification and offers the Approval once at the start of the next Session. A parked `ask_user` is raised the same way.
- Only "cancel it" stops a build.

Spec: Authoring; Session state machine (Authoring amendment).

**Blocked by:** 22, 08, 15

**Seams:** uses the Delegate tools, the scripted Delegate and the build-running state (owned by 22); uses the once-only Session-start mention (owned by 15); uses the Approval flow (owned by 14); extends `NotificationKind` (owned by 08, resolved)

**Status:** resolved

- [x] Indicator "building" mark is carried over IPC
- [x] Timeout ends the build with a plain report, and staging is kept
- [x] Queue of one is enforced
- [x] Approval and parked questions are offered once at the next Session start
- [x] Harness tests for budget, queue, Session-end survival, parked question and cancel

## Comments

Resolved on `lucy-v0-1-slice-6` (3c50b29..c56f613). `state`/`status` carry `building`, true only while the Delegate works or checks (`authoring::Building {No, Working, Waiting}`); the silence timer pauses on the same condition. `[authoring] budget_secs` (default 1200) counts only while Working, so a question parked overnight doesn't time the build out; staging is kept and a retry resumes on it. Queue of one; a third request is refused. Ending by voice, Lucy says she'll keep working; an Indicator click ends with a Build notification instead. Outcomes, parked questions and Approvals that find no Session become one Build notification and are raised once at the next Session start (`runner::When`, `Command::TurnOrNextSession`). `report_progress` goes to notifications only (`[authoring] progress_notifications`), not the TUI (user decision; spec updated). "cancel it" must be the whole utterance, is matched by lucyd, and also drops the queued build. New `Clock`/`Timer` adapter traits (`Adapters.clock`, harness `FakeClock`) make the budget tests deterministic. Not yet checked with a person: the corner dot on a real desktop.
