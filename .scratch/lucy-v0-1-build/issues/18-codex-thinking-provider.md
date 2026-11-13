# 18: Codex Thinking Provider

**What to build:** A ChatGPT subscriber can choose Codex for either tier. A Thinking Provider of kind `codex` runs `codex app-server` over stdio, one process per Session. Lucy's MCP relay is injected with `-c mcp_servers.lucy.*`, login is pinned with `forced_login_method="chatgpt"`, `item/agentMessage/delta` streams into sentence chunking, and approvals are answered in-process. Barge-in uses app-server's interrupt.

Spec: Providers and configuration (`codex`).

**Blocked by:** 12, 11

**Seams:** owns `SessionModel::interrupt` (also used by 09); uses the dead-process `ThinkingEvent` (owned by 11); extends `Kind`, `ProviderKind` and `RECOMMENDATIONS` (owned by 12, resolved)

**Status:** resolved

- [ ] Selectable per tier in config
- [ ] Same Session behaviour as Claude through the harness (a fake app-server stream)
- [ ] Health checks and spoken failures as in 12
- [ ] Opt-in live test against a real `codex` login

## Comments

Resolved on `lucy-v0-1-slice-4` (80a2620..5e4e5a8). Adapter in `crates/lucyd/src/codex.rs`, routed by `ByKind`; fake app-server in `crates/lucyd/tests/fixtures/codex/` (hand-written from `generate-ts` types, not recorded). Recommendations: fast `gpt-6-luna`, advanced `gpt-6.1-sol` (alternative `gpt-6-astra`), unconfirmed. `allow_api_key` also lifts Codex's ChatGPT pin (user decision; spec updated). `SessionModel::interrupt` exists; nothing calls it until 09 (note added there); Claude's is empty. The live test is `#[ignore]` and not run: the user's codex login looked expired (`account/read` returned null). Rate-limit and failed-turn events aren't spoken yet, same as Claude.
