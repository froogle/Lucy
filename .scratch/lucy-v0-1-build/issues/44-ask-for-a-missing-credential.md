# 44: A missing credential is asked for there and then

**What to build:** When a Plugin call needs a credential the user hasn't entered, Lucy doesn't fail with a pointer to the terminal any more. After any Confirmation the tool needs, she opens a `secret` Input dialog for each missing name ("I've put a box on screen for your Linear API key"). She stores what's typed in the Secret Service, starts the Plugin and carries on with the call. A cancel or timeout fails the call with the reason and the `lucy credentials <plugin>` pointer, as today.

Spec: [v0.1](../../lucy-v0-1/spec.md) as amended 2026-11-16: Plugins (missing credential). Stories 156, 157.

**Blocked by:** 43

**Seams:** none new. Uses the `secret` path (owned by 43) and `CredentialStore` (ticket 21).

**Status:** resolved

**Notes:**

- Settled in a grilling session (2026-11-16); split into tracer slices by `to-tickets` the same day.
- Confirmation comes first, so the user never types a key for an action they then decline.

**Acceptance:**

- [ ] A call with a missing credential: Confirmation (when the tool has one), then one dialog per missing name, then the call completes with the credential in the Plugin's env
- [ ] A declined Confirmation opens no dialog
- [ ] A cancel or timeout fails the call with the `lucy credentials <plugin>` pointer
- [ ] Once, by hand: a Plugin with a missing credential asks for it, and the request completes

## Comments

2026-11-16: built in slice 10 (`lucy-v0-1-slice-10`). Lucy names the credential from its env-var name ("your Linear API key"). Values typed before a later cancel stay stored. Hand check left: a Plugin with a missing credential asks for it (in the bar and in a terminal), and the request completes.

2026-11-16: hand-checked on the deployed build; passed.
