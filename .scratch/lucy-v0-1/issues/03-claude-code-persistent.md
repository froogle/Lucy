# How does a long-running Claude Code instance behave (speed, Lucy's tools, permissions, plan limits), and how does Codex compare?

Type: research
Status: resolved
Blocked by: none

## Question

For Claude Code in headless mode (claude -p with --input-format stream-json / --output-format stream-json, or the Agent SDK driving the CLI): time-to-first-token cold vs warm, how to keep one process alive across turns, how to register a custom MCP server (Lucy's tools), headless permission handling (allowedTools, permission-prompt-tool), streaming partial text for early TTS, and how usage counts against a Claude Max subscription. Do the same, more briefly, for codex exec / Codex app-server with a ChatGPT login. Recommend how Lucy should drive each.

## Answer

Drive Claude Code as **one persistent `claude -p --input-format stream-json --output-format stream-json --include-partial-messages` process per Session**, pre-spawned on the Wake word. Use an isolated config (`--strict-mcp-config --mcp-config <lucy> --setting-sources "" --tools ""`), `--permission-mode dontAsk --allowedTools "mcp__lucy__*"`, and spoken confirmation enforced inside Lucy's own MCP tools. With that setup, warm turns reached first token in ~0.6–1.5 s, against ~2–3 s cold.
Don't use `--bare`: it can't use subscription OAuth. Scrub `ANTHROPIC_API_KEY` from the child environment. Usage draws on the shared Max five-hour and weekly windows, which arrive as `rate_limit_event` in the stream.
For Codex, use **`codex app-server`** (JSON-RPC over stdio, `item/agentMessage/delta` for streaming, approvals answered in-process). Inject Lucy's MCP server via `-c mcp_servers.lucy.*`, and keep `codex exec --json` only as the spawn-per-turn fallback.
Risks are in the findings: `--bare` may become the `-p` default; the SDK policy note about third-party claude.ai login; the undocumented interrupt protocol.
[findings](../research/claude-code-persistent.md)
