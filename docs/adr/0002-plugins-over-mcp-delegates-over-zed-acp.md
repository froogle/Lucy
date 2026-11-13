# Plugins speak MCP; Delegates speak Zed's Agent Client Protocol

Lucy owns her own Plugin and Delegate interfaces, with each Plugin's Manifest as the source of truth, and treats wire protocols as adapters. Calling a Plugin is a deterministic tool call, so Plugins are separate processes speaking MCP over stdio. Handing a goal to a Delegate is a long-running task with progress, questions and cancellation, so Delegates (Claude, Codex) are driven over Zed's Agent Client Protocol, which local agents actually support today. See `.scratch/lucy-v0-1/research/agent-protocol.md`.

## Considered Options

- MCP for everything: fits tool calls, but models delegation poorly.
- A2A for delegation: the right model, but HTTP/gRPC only and unsupported by Claude Code, Codex or Herdr today. Deferred until Lucy talks to remote agents.
- A Lucy-specific protocol: nobody else would speak it.

## Amendment (2026-11-12, ticket 08)

Claude and Codex Delegates do not go through Zed-ACP. `claude-agent-acp` is built on the Claude Agent SDK, and Anthropic's SDK policy forbids third-party products offering claude.ai login "including agents built on the Claude Agent SDK". Driving the `claude` CLI directly stays in the grey zone already accepted in ADR 0001. `codex-acp` is only a Node wrapper around `codex app-server`. Delegates therefore use the same native clients as the Thinking Providers: `claude -p` stream-json and `codex app-server`. The `Delegate` seam is unchanged, and Zed-ACP stays as an adapter for agents that speak it natively. See `.scratch/lucy-v0-1/research/provider-config.md`.
