# 12: Config, Model tiers and subscription guard

**What to build:** Users configure Lucy in one TOML file, and Lucy never bills them by accident.
- `config.toml` in the XDG config dir is reloaded at each Session start. It holds named `[providers.<name>]` entries and `[tiers] fast/advanced`. `lucy-config` writes it with comments preserved.
- When `claude --version` changes, Lucy re-verifies before the next Session: auth status must show `claude.ai`, and one turn must show `apiKeySource: "none"` plus a `rate_limit_event`. If not, she refuses that Provider and says why.
- A per-Provider `allow_api_key = true` opts out of the guard.
- Health checks: binary present and auth correct on every launch. A real "Reply OK" round trip through IPC `providers.status`/`provider.check`.

Spec: Providers and configuration. ADR 0001.

**Blocked by:** 04

**Status:** resolved

- [ ] Config edits apply at the next Session without a restart; a missing file still means defaults
- [ ] Comment-preserving writer round-trips a commented file unchanged apart from the edited key
- [ ] Tiers resolve to named Providers; the fast tier drives Sessions (the advanced tier is used in 22)
- [ ] Version-change re-verification and refusal, harness-tested with a fake CLI
- [ ] Shipped per-tier model recommendation table; any model the CLI accepts can be chosen
- [ ] Provider failures are spoken with the fix

## Comments

Resolved on `lucy-v0-1-slice-2` (0a4e21d..890dded). Health checks run at each Session start, when the CLI is spawned (spec wording updated). The live guard check (spends subscription) has not been run. `codex` is not yet in the config enum (ticket 18).
