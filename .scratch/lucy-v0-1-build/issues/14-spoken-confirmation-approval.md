# 14: Spoken Confirmation and Approval

**What to build:** Risky actions and new Plugins need the user's spoken yes, heard by Lucy's runtime, never by the model.
- **Confirmation:** a tool with `confirm = true` suspends in the Confirming state. Lucy speaks its `confirm_prompt` (with `{arg}` filled in) or a generic prompt built from `handles`, and shows the full arguments in a notification. She then matches one utterance locally against the configurable yes-phrases. The model only receives `confirmed` or `declined`.
- **Approval:** uses the same listening step. A new Plugin gets the full spoken summary (handles, access, what it asks before doing); an update gets a changes-only summary, with the full diff on screen. A yes records the hash-pinned Approval, and `tools/list_changed` follows.

Spec: Confirmation and Approval are runtime-owned; Plugins (Approval). ADR 0005.

**Blocked by:** 13, 05, 06

**Seams:** owns the Confirming step in `session::Session` (`State::Confirming`) (also used by 09, 15); owns the spoken Approval flow and `plugins::Host`'s writes to `approvals.toml` (also used by 15, 20, 22, 23); owns `tools/list_changed` from the MCP tool server (also used by 15, 22); extends `NotificationKind` (owned by 08, resolved)

**Status:** ready-for-agent

- [ ] A yes during the prompt counts; anything else, or 8 s of silence, means no, and Lucy says "Okay, I won't"
- [ ] Indicator click while Confirming declines and ends the Session
- [ ] Silence timer is paused while Confirming
- [ ] No model-callable tool grants an Approval, and no IPC request does either
- [ ] Shipped Plugins can be approved during setup through the same spoken flow (setup wiring lands in 20)
- [ ] Mid-call questions over MCP `input_required` are relayed to the user
- [ ] Harness tests: yes, non-yes, silence, yes-during-prompt, click, update summary
