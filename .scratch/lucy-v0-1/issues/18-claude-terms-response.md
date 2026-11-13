# How should Lucy respond to Anthropic's ambiguous terms on driving a user's own Claude subscription?

Type: grilling
Status: resolved
Blocked by: none

## Question

[Do Anthropic's and OpenAI's terms allow an open-source tool to drive a user's own logged-in Claude Code and Codex CLIs?](15-subscription-terms.md) found OpenAI explicitly permits Lucy's Codex path but Anthropic's text is ambiguous for Lucy driving the user's own `claude -p`. Decide: whether to ask Anthropic in writing before relying on it (and who sends it); whether the default thinking Provider stays Claude subscription, becomes opt-in with a disclaimer, or switches to Codex/API key; whether API-key Providers become first-class in v0.1; and whether ADR 0001 is amended. Also decide the one-user policy both vendors' terms imply (guests or household members talking to Lucy: speaker recognition, a single-user rule, or a guest mode with no subscription use) and that any proactive or background turns need explicit opt-in consent.

## Answer

- Claude on the user's own subscription stays the default thinking Provider. Lucy is software each person installs on their own machine with their own accounts; no letter to Anthropic. ADR 0001 stands (amended with this reasoning).
- Lucy doesn't police how users configure Providers. API-key Providers are not a v0.1 priority; the Provider design just shouldn't preclude them.
- Lucy is single-user: one person's accounts, stated in the README. Guests can still talk to her; no speaker recognition.
- Lucy acts only when spoken to (Wake word). No proactive or background turns in v0.1; any later proactive behaviour is opt-in.
