# Should Plugins and agent delegation use MCP, A2A, or something else?

Type: research
Status: resolved
Blocked by: none

## Question

The user dislikes MCP's "fire up a tool" model and wants autonomous agents to delegate to each other ("Agent B, go do this"). Survey the current state (Nov 2026) of MCP, Google/Linux Foundation A2A (Agent2Agent, which absorbed IBM's ACP), and Zed's Agent Client Protocol: maturity, Rust SDKs, adoption, licence, and what each models (deterministic tool calls vs delegated long-running tasks with status/streaming). Assess for Lucy two distinct relationships: (a) Lucy calling a Plugin, mostly plain code; (b) Lucy delegating a goal to another agent (the advanced tier building a Plugin, a Herdr agent). Recommend a protocol for each, and whether Lucy should keep its own internal Plugin interface with protocols as adapters.

## Answer

- **Keep Lucy's own interfaces and treat protocols as adapters.** There are two seams: `Plugin` (call a capability) and `Delegate` (hand over a goal). Lucy's Manifest stays the source of truth for request matching, access and approval.
- **Plugins: MCP over stdio.** Use `rmcp` 3.x against spec 2026-07-28. The parts that matter for Lucy:
  - `tools/call` for ordinary calls
  - MRTR `input_required` for spoken confirmation
  - the Tasks extension for slow jobs
- **Delegation: Zed's Agent Client Protocol.** Use `agent-client-protocol` 5.x over stdio for local agents. Claude Code and Codex are reached through the official `claude-agent-acp` and `codex-acp` adapters. Lucy passes them her MCP server and a working directory, and gets back streamed progress, permission requests and cancellation.
- **A2A (v1.0, AAIF):** keep it for remote or cross-vendor agents later. It is HTTP-only, and no local agent speaks it.
- **Herdr:** stays on its bespoke socket transport.

[findings](../research/agent-protocol.md)

## Comments

- 2026-11-12, from [How are Providers and configuration structured, and what does the v0.1 TUI cover?](08-provider-config.md): the delegation answer is amended. `claude-agent-acp` runs on the Claude Agent SDK, whose policy forbids third-party products offering claude.ai login. `codex-acp` is just a Node wrapper around `codex app-server`. Claude and Codex Delegates therefore use the native clients (`claude -p` stream-json, `codex app-server`), and Zed-ACP stays only as an adapter for agents that speak it natively. See ADR 0002.
