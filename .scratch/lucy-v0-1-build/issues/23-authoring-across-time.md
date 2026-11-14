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

**Status:** ready-for-agent

- [ ] Indicator "building" mark is carried over IPC
- [ ] Timeout ends the build with a plain report, and staging is kept
- [ ] Queue of one is enforced
- [ ] Approval and parked questions are offered once at the next Session start
- [ ] Harness tests for budget, queue, Session-end survival, parked question and cancel
