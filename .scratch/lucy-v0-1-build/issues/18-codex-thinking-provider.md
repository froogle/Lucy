# 18: Codex Thinking Provider

**What to build:** A ChatGPT subscriber can choose Codex for either tier. A Thinking Provider of kind `codex` runs `codex app-server` over stdio, one process per Session. Lucy's MCP relay is injected with `-c mcp_servers.lucy.*`, login is pinned with `forced_login_method="chatgpt"`, `item/agentMessage/delta` streams into sentence chunking, and approvals are answered in-process. Barge-in uses app-server's interrupt.

Spec: Providers and configuration (`codex`).

**Blocked by:** 12

**Status:** ready-for-agent

- [ ] Selectable per tier in config
- [ ] Same Session behaviour as Claude through the harness (a fake app-server stream)
- [ ] Health checks and spoken failures as in 12
- [ ] Opt-in live test against a real `codex` login
