# 15: Plugin management

**What to build:** Users manage Plugins by voice and over IPC.
- `manage_plugin` supports enable, disable, rollback and remove. Remove needs Confirmation; shipped Plugins can be disabled but not removed.
- One previous approved version is kept, and rolling back to it needs no new Approval.
- IPC `plugins.list`, `plugins.set_enabled`, `approvals.list` and `approvals.revoke`; a revoke takes effect mid-Session through `tools/list_changed`, and `plugins.changed` events are emitted.
- Hand-installed Plugins are found on the next scan, listed as awaiting Approval, and mentioned once at the start of the next Session.
- Lucy edits only `author = "lucy"` Plugins.

Spec: Plugins (Lifecycle); MCP tool server (`manage_plugin`); Public IPC contract.

**Blocked by:** 14

**Seams:** owns IPC `plugins.list`, `plugins.set_enabled`, `approvals.list`, `approvals.revoke` and the `plugins.changed` event (also used by 20); owns the once-only mention at the start of the next Session (also used by 23); uses the Confirming step, the Approval flow and `tools/list_changed` (owned by 14)

**Status:** ready-for-agent

- [ ] Disable keeps files and Approval; enable restores straight away
- [ ] Remove deletes the folder and the Approval after a spoken yes
- [ ] Rollback restores the previous approved hash with no prompt
- [ ] Revoke over IPC removes the tools mid-Session
- [ ] Awaiting-Approval Plugins are mentioned once, not every Session
- [ ] Harness tests for each action
