# 48: Approve a hand-installed Plugin by voice

**What to build:** A Plugin the user drops into the Plugins folder is found, shown as awaiting Approval, and mentioned once at the next Session (story 87). Nothing then lets the user approve it. The only caller of `Host::request_approval` outside tests is Authoring. The `lucy` TUI's Approvals section only revokes. In the 2026-11-16 hand check, Lucy told the user to "approve it in the terminal", a terminal that doesn't exist.

Add a Built-in tool, `approve_plugin {name}`, that starts lucyd's spoken Approval for a Plugin awaiting it. Lucy speaks the full summary for a new Plugin, or the changes-only summary plus the full diff on screen for an update, exactly as Authoring's Approval does. lucyd matches the user's yes or no itself (Confirming). The model can ask for an Approval but never grant one. On a yes, the Plugin's tools join the Session (`tools/list_changed`). The tool returns `approved`, `declined` or `timed_out`, or an error when no Plugin by that name is awaiting Approval.

Spec: [v0.1](../../lucy-v0-1/spec.md): Plugins (Approval, hand-installed Plugins), Built-in tools. Story 87. ADR 0005 (Approvals are granted only by voice).

**Blocked by:** none

**Seams:** owns the `approve_plugin` Built-in tool. Uses `Host::request_approval` and the spoken Approval as they are.

**Status:** ready-for-agent

**Notes:**

- The awaiting-Approval mention in the Session instructions (`scan_report`) should tell the model it can offer `approve_plugin`, so Lucy can say "Want me to go through it?" rather than invent a way.
- Amend the spec's Built-in tools table and the hand-installed Plugins line.

**Acceptance:**

- [ ] A hand-installed fixture Plugin: `approve_plugin` speaks the summary, a spoken yes approves it and its tools appear in the same Session; a no leaves it awaiting
- [ ] `approve_plugin` on an approved, shipped or unknown name returns an error and speaks nothing
- [ ] The awaiting-Approval mention names the way to approve
- [ ] Once, by hand: drop a Plugin in, "Hey Lucy, approve the demo-key plugin", hear the summary, say yes, and use it

## Comments
