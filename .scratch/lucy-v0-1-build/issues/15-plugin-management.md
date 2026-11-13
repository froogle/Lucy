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

**Status:** resolved

- [ ] Disable keeps files and Approval; enable restores straight away
- [ ] Remove deletes the folder and the Approval after a spoken yes
- [ ] Rollback restores the previous approved hash with no prompt
- [ ] Revoke over IPC removes the tools mid-Session
- [ ] Awaiting-Approval Plugins are mentioned once, not every Session
- [ ] Harness tests for each action

## Comments

Resolved on `lucy-v0-1-slice-4` (80a2620..5e4e5a8). `manage_plugin` lives in `crates/lucyd/src/plugins/manage.rs`; `plugins.changed` has empty params (clients re-fetch). Decisions (user, for now): rollback is refused for Plugins not written by Lucy; remove is allowed for any non-shipped Plugin after a spoken yes. Rollback works only for versions approved by voice after this change (kept copies under `state/versions/`). Review fixes: revoke keeps the previous version (in a `revoked` table) so rollback works after re-approval; shipped Plugins are now approved at scan, so removing a user override brings the shipped copy back approved at the next Session, and revoking a shipped Plugin doesn't stick (disable keeps it out). Rollback writes the executable from a child process (no write fd in lucyd).
