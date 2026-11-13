# 14: Spoken Confirmation and Approval

**What to build:** Risky actions and new Plugins need the user's spoken yes, heard by Lucy's runtime, never by the model.
- **Confirmation:** a tool with `confirm = true` suspends in the Confirming state. Lucy speaks its `confirm_prompt` (with `{arg}` filled in) or a generic prompt built from `handles`, and shows the full arguments in a notification. She then matches one utterance locally against the configurable yes-phrases. The model only receives `confirmed` or `declined`.
- **Approval:** uses the same listening step. A new Plugin gets the full spoken summary (handles, access, what it asks before doing); an update gets a changes-only summary, with the full diff on screen. A yes records the hash-pinned Approval, and `tools/list_changed` follows.

Spec: Confirmation and Approval are runtime-owned; Plugins (Approval). ADR 0005.

**Blocked by:** 13, 05, 06

**Seams:** owns the Confirming step in `session::Session` (`State::Confirming`) (also used by 09, 15); owns the spoken Approval flow and `plugins::Host`'s writes to `approvals.toml` (also used by 15, 20, 22, 23); owns `tools/list_changed` from the MCP tool server (also used by 15, 22); extends `NotificationKind` (owned by 08, resolved)

**Status:** resolved

- [ ] A yes during the prompt counts; anything else, or 8 s of silence, means no, and Lucy says "Okay, I won't"
- [ ] Indicator click while Confirming declines and ends the Session
- [ ] Silence timer is paused while Confirming
- [ ] No model-callable tool grants an Approval, and no IPC request does either
- [ ] Shipped Plugins can be approved during setup through the same spoken flow (setup wiring lands in 20)
- [ ] Mid-call questions over MCP `input_required` are relayed to the user
- [ ] Harness tests: yes, non-yes, silence, yes-during-prompt, click, update summary

## Comments

Resolved on `lucy-v0-1-slice-3` (427912a..1339254). One listening step, `session::Input::Ask`, serves Confirmations, Approvals and Plugin `input_required` questions. `[confirmation]` holds `yes_phrases` and `timeout_secs` (the timeout config isn't in the spec). The only Approval entry point is the in-process `Daemon::request_approval`; ticket 20 needs an IPC request that *starts* one, never grants it. `approvals.toml` now stores the approved Manifest for update diffs; older entries get the full summary. Plugins start with rmcp `ClientLifecycleMode::Auto` (older-protocol fallback untested). `tools/list_changed` uses rmcp's `notify_tool_list_changed`; a real `claude` receiving it is unchecked.
