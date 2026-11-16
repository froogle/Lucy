# 43: Plugin questions in the Input dialog

**What to build:** A Plugin's mid-call question opens the Input dialog when it shouldn't be spoken. That means a question with a `uri` field, with more than one field, or with Lucy's `secret` field. A question with one plain string field is still asked aloud, as today. `secret` is accepted only on a credential name the Plugin's Manifest declares. What's typed goes to the Secret Service, the Plugin gets it as that env var on its next start, and the model learns only `provided`, `cancelled` or `timed_out`.

Spec: [v0.1](../../lucy-v0-1/spec.md) as amended 2026-11-16: Plugins (mid-call questions), Testing (Input dialog). Stories 153, 165.

**Blocked by:** 34

**Seams:** owns the `secret` elicitation field. Uses the `input` role and `Typing` (owned by 34), and `CredentialStore` (ticket 21).

**Status:** resolved

**Notes:**

- Settled in a grilling session (2026-11-16); split into tracer slices by `to-tickets` the same day.
- Field types are MCP form elicitation's (string with `uri`/`email` formats, number, boolean, enum) plus `secret`, with no custom protocol.
- The fixture Plugin's question tool gains a URL question, a two-field question and a `secret` question, for the harness.

**Acceptance:**

- [ ] A `uri` or multi-field question opens the dialog, and the answers go back to the Plugin
- [ ] A one-field plain question is still spoken
- [ ] `secret` on a declared credential: the value lands in the fake credential store, the Plugin's next start gets it as its env var, and the model sees only `provided`
- [ ] `secret` on an undeclared name is declined without opening the dialog
- [ ] No secret in IPC, logs, notifications or model traffic (`assert_nothing_private_on_bus` counts it)

## Comments

2026-11-16: built in slice 9 (`lucy-v0-1-slice-9`). A Plugin marks a secret in the elicitation's `_meta` as `"lucy/secret"` (ADR 0011). A lone number, boolean or enum field also opens the dialog (user decision; spec amended). Plugin-author docs (the authoring brief and template) don't yet cover mid-call questions or `lucy/secret`.

2026-11-16: hand-checked on the deployed build; passed.
