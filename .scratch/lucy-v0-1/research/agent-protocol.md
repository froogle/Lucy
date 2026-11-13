# Plugin and agent-delegation protocols: MCP, A2A, Agent Client Protocol

Ticket: [21-agent-protocol](../issues/21-agent-protocol.md). Researched 2026-11-12 from official spec sites, GitHub repos (via `gh api`), the AAIF/Linux Foundation blogs and the crates.io API. Star counts and download totals are as of 2026-11-12.

## Sources

- **[mcp-changelog]**: https://modelcontextprotocol.io/specification/2026-07-28/changelog. This is the changelog for the current spec, 2026-07-28.
- **[mcp-repo]**: https://github.com/modelcontextprotocol/modelcontextprotocol (9.7k stars). Its releases are `2026-07-28` (2026-07-28) and `2026-07-28-RC` (2026-05-29).
- **[rmcp]**: https://github.com/modelcontextprotocol/rust-sdk (4.1k stars). I read `crates/rmcp/CHANGELOG.md`, the README and the `LICENSE`. The crates.io entry for `rmcp` shows 3.9.0 (2026-11-05), about 48.5M downloads and the licence `Apache-2.0`.
- **[aaif-form]**: https://www.linuxfoundation.org/press/linux-foundation-announces-the-formation-of-the-agentic-ai-foundation (2025-12-09).
- **[aaif-a2a]**: https://aaif.io/blog/a2a-joins-aaif (2026-08-17).
- **[a2a-spec]**: https://a2a-protocol.org/latest/specification/ (v1.0.0). Its normative source is `spec/a2a.proto`.
- **[a2a-mcp]**: https://a2a-protocol.org/latest/topics/a2a-and-mcp/
- **[a2a-repo]**: https://github.com/a2aproject/A2A (Apache-2.0, 26.5k stars). Its releases are v1.0.0 (2026-03-12) and v1.0.1 (2026-05-28).
- **[a2a-rs]**: https://github.com/a2aproject/a2a-rs (Apache-2.0, 114 stars, created 2026-04-03). On crates.io:
  - `a2a-lf` 0.4.2: about 195k downloads
  - `a2a-client-lf` 0.2.9
  - `a2a-server-lf` 0.5.3

  All three were last updated 2026-10-16.
- **[acp-merge]**: the LF AI & Data post co-written by IBM Research and Google (August 2025), and IBM's ACP page (https://research.ibm.com/projects/agent-communication-protocol). The repo `i-am-bee/acp` was archived after v1.0.3 (2025-08-21).
- **[zacp-repo]**: https://github.com/agentclientprotocol/agent-client-protocol (Apache-2.0, 4.6k stars). Its releases are v1.17.0 and schema-v1.31.0 (both 2026-11-10). I read the README, `schema/v1/meta.json` and `schema/v1/schema.json`, plus these docs:
  - `docs/updates.mdx`
  - `docs/announcements/acp-v2-draft.mdx` (2026-07-20)
  - `docs/announcements/transports-working-group.mdx` (2026-04-22)
  - `docs/announcements/sergey-ignatov-lead-maintainer.mdx` (2026-02-18)
  - `docs/community/governance.mdx`
- **[zacp-rust]**: https://github.com/agentclientprotocol/rust-sdk. The crates.io entry for `agent-client-protocol` shows 5.2.0 (2026-11-10), about 6.2M downloads and the licence `Apache-2.0`.
- **[zacp-agents]**: https://agentclientprotocol.com/overview/agents
- **[zacp-adapters]**: two adapter repos:
  - https://github.com/agentclientprotocol/claude-agent-acp: "Use Claude Agent SDK from any ACP client". Apache-2.0, v0.106.0 (2026-11-10).
  - https://github.com/agentclientprotocol/codex-acp: v2.6.0 (2026-11-08). GitHub can't detect its licence (`NOASSERTION`).

## Naming hazard

"ACP" refers to two unrelated protocols:

- **IBM's Agent Communication Protocol** merged into A2A in August 2025 and is archived [acp-merge].
- **Zed's Agent Client Protocol** is alive and active [zacp-repo].

The rest of this file uses **Zed-ACP** for the second one.

## The three protocols

| | MCP | A2A | Zed-ACP |
|---|---|---|---|
| Models | An agent *using* a capability: a tool call, resource or prompt. Long-running work goes through the optional **Tasks extension**. | An agent *delegating to another, opaque agent*. The unit of work is a Task with a lifecycle, Messages and Artifacts. | A client (editor/host) *driving one agent session*. That covers prompts, streamed updates, tool-call reporting, permission requests, cancellation and session load/resume. |
| Current version | Spec `2026-07-28`, stable since 2026-07-28 [mcp-repo]. It has a 12-month deprecation policy [mcp-changelog]. | v1.0.0 (2026-03-12), v1.0.1 (2026-05-28) [a2a-repo] | Protocol v1 is stable. v2 has been in draft since 2026-07-20 [zacp-repo]. |
| Transport | stdio, or Streamable HTTP | JSON-RPC over HTTP, HTTP+JSON/REST, gRPC, and custom bindings. **It has no stdio binding.** Streaming uses SSE, and push notifications use HTTP webhooks [a2a-spec]. | JSON-RPC over **stdio to a subprocess**. A WebSocket/HTTP remote transport is still at Draft RFD stage [zacp-repo]. |
| Governance | AAIF, under the Linux Foundation, since 2025-12-09 [aaif-form] | Linux Foundation since 2025. It became an AAIF hosted project on 2026-08-17 [aaif-a2a]. | Governed jointly by Zed and JetBrains, who are "working toward transitioning to an independent foundation". It is not in a foundation yet [zacp-repo governance]. |
| Licence | The spec repo is moving from MIT to Apache-2.0, and new contributions are Apache-2.0. The `rmcp` crate is `Apache-2.0` [rmcp]. | Apache-2.0 | Apache-2.0 |
| Rust SDK | Official `rmcp` 3.9.0: about 48M downloads, implements 2026-07-28 including the Tasks extension and MRTR [rmcp]. **Mature.** | Official `a2a-rs` workspace (`a2a-lf` 0.4.2 and siblings), still 0.x and seven months old. It supports JSON-RPC, REST, gRPC and SSE, and is built on axum and tonic [a2a-rs]. **Usable but young.** | Official `agent-client-protocol` 5.2.0: about 6.2M downloads, with both client and agent sides [zacp-rust]. **Mature.** |
| Adoption | Universal for tools. Claude Code, Codex and others are clients. | 150+ organisations. It is in Google Cloud, Azure AI Foundry and AWS Bedrock AgentCore, and has enterprise deployments (HarmonyOS, WeChat, PayPal) [aaif-a2a]. It is not in local CLI agents. | About 50 agents [zacp-agents]. Gemini CLI, Goose, Copilot, Cursor, Cline, OpenCode, Junie and Kimi support it, among others. **Claude** (via `claude-agent-acp`) and **Codex** (via `codex-acp`) use official adapters [zacp-adapters]. Clients include Zed and the JetBrains IDEs. |

### MCP in detail (2026-07-28)

These are the changes relevant to Lucy [mcp-changelog]:

- **Stateless protocol.** There is no `initialize` handshake. Each request carries its version and capabilities in `_meta`. Servers must implement `server/discover`.
- **Tasks extension.** Tasks moved from the core protocol into an official extension, `io.modelcontextprotocol/tasks`. Clients poll with `tasks/get`, and `tasks/update` carries input from client to server. Servers can return task handles without being asked. This is "a long-running tool call", not delegation. The caller still owns the goal.
- **MRTR replaces server-initiated requests.** A result can be `resultType: "input_required"`, and the client retries the request with the answers. This is how a tool asks for confirmation.
- **Deprecations.** Sampling, Roots and Logging are deprecated. Servers are told to "integrate directly with LLM provider APIs instead of Sampling". So MCP is moving away from servers that think, and toward plain tools.
- **SDK support.** `rmcp` shipped 3.0.0 on spec day (2026-07-28) with the Tasks extension (#1020) and MRTR (#929). 3.9.0 adds the Skills extension, SEP-2640 [rmcp].

### A2A in detail (v1.0)

- **Task states** [a2a-spec]:
  - Active: `SUBMITTED`, `WORKING`
  - Terminal: `COMPLETED`, `FAILED`, `CANCELED`, `REJECTED`
  - Interrupted: `INPUT_REQUIRED`, `AUTH_REQUIRED`
- **Agent Card.** Each agent publishes one describing its skills, endpoint and auth. The Agent Card is the closest analogue to Lucy's Manifest.
- **Streaming.** `TaskStatusUpdateEvent` and `TaskArtifactUpdateEvent` arrive over SSE. Push notifications go to webhooks.
- **Positioning.** A2A's docs frame it like this [a2a-mcp]: "A2A is about agents *partnering* on tasks; MCP is more about agents *using* capabilities … Use both together."
- **Locality.** A2A assumes network-addressable agents with HTTP endpoints and auth. To use it locally, every agent has to run an HTTP server on loopback. None of the agents Lucy delegates to (Claude Code, Codex, Herdr panes) speaks A2A.

### Zed-ACP in detail (v1, v2 draft)

- **Agent methods** [zacp-repo `meta.json`]:
  - `initialize`, `authenticate`
  - `session/new`, `session/load`, `session/resume`, `session/list`, `session/close`
  - `session/prompt`, `session/cancel`
  - `session/set_mode`, `session/set_config_option`
- **Client methods**, which the agent calls on the host:
  - `session/update`, a stream of message chunks, tool calls and plans
  - `session/request_permission`
  - `fs/read_text_file`, `fs/write_text_file`
  - `terminal/*`
  - `elicitation/*`
- **`session/new` parameters.** It takes `cwd`, `additionalDirectories`, **`mcpServers`** and `systemPrompt` [zacp-repo `schema.json`]. The host can therefore hand the delegated agent its own MCP tool server and a working directory, and the agent's permission prompts come back to the host. Client-provided system prompts were stabilised 2026-11-06.
- **v2 draft (2026-07-20).** v2 is about "moving beyond the turn". Updates can flow at any time, agents report idle/ready, and work can run in the background, be queued or be steered. That is the delegated, long-running pattern the user wants, in a local, stdio form.
- **Scope.** The spec is written for "code editors ↔ coding agents". Lucy would be an unusual ACP client, a voice host rather than an editor, but nothing in the protocol requires an editor. File and terminal capabilities are optional.

## Assessment for Lucy

### (a) Lucy calling a Plugin (mostly plain code)

This is a deterministic call with typed arguments, a result, an occasional confirmation ("send this?"), and sometimes a slow job such as searching photos. That maps directly onto MCP:

- `tools/call` for the call itself
- MRTR `input_required` for the spoken confirmation
- the Tasks extension for the slow job

A2A and Zed-ACP would turn every Plugin into an "agent" with sessions or HTTP. That conflicts with the "Plugins are mostly plain code" note in the map.

- **Recommendation.** Keep Lucy's own `Plugin` interface and Manifest as the source of truth for request matching, the access list, user approval and the risky-action flag. MCP has no concept of approval state or of Lucy's permission model.
- **Wire format.** Use MCP over stdio via `rmcp` as the wire format for out-of-process Plugins, with each Plugin acting as a small MCP server.
- **Benefits:**
  - It reuses the stack Lucy already uses for her action tool server.
  - The advanced tier, which is Claude Code or Codex (both fluent MCP clients), can write, run and test the Plugins it builds.
  - Users get existing tooling, such as the MCP Inspector.
  - Third-party MCP servers can be wrapped as Plugins with a generated Manifest.
- **Risk.** MCP made breaking changes in 2026-07-28, when it went stateless and moved Tasks into an extension. A thin `Plugin` trait in Lucy keeps MCP churn inside one adapter.

### (b) Lucy delegating a goal to another agent

This covers two cases: the advanced tier building a Plugin, and a Herdr agent. Here the other side owns the goal, works for minutes, streams progress, asks for permissions and can be cancelled. Both MCP Tasks and A2A Tasks model this. Only Zed-ACP is spoken by the actual local agents today (Claude via `claude-agent-acp`, Codex via `codex-acp`), over stdio, with a mature Rust client.

- **Recommendation.** Make Zed-ACP the delegation protocol for local agents, behind a Lucy `Delegate` seam.
- **What it gives Lucy:**
  - The host passes `mcpServers`, so the delegated agent sees only Lucy's tools.
  - It passes `cwd`, for example a scratch Plugin directory.
  - Permission requests arrive as `session/request_permission`, which Lucy can speak aloud or route to the approval gate.
  - Progress arrives as streamed `session/update`, which is good for spoken status like "still working on it".
- **A2A.** Keep it as a later adapter for *remote* or third-party agents (cloud agents, agents on another machine). It is the right protocol for opaque cross-vendor agents over a network, but in v0.1 it would mean loopback HTTP servers, and no local agent speaks it.
- **Herdr.** It speaks none of these protocols and stays the bespoke socket transport from ticket 04.

### Should Lucy keep an internal interface with protocols as adapters?

**Yes.** Lucy should keep two internal seams, `Plugin` (call) and `Delegate` (goal), each with one protocol adapter in v0.1: MCP for `Plugin`, Zed-ACP for `Delegate`.

- **Why keep both seams.** The user's "Agent B, go do this" is exactly the line between them. A2A's own docs draw the same line [a2a-mcp].
- **Why use adapters.** All three specs are still moving. MCP has a breaking revision every 6 to 12 months, Zed-ACP v2 is in draft, and A2A's Rust SDK is 0.x.
- **Rejected alternative.** A single "everything is A2A" design would push network and auth machinery into what should be a subprocess call.

## Open points this surfaces

- **Zed-ACP instead of the bespoke stream-json client?** Ticket 03 chose stream-json `claude -p` and `codex app-server` for the thinking Provider. Could Zed-ACP replace both bespoke clients with one Rust client? Before deciding, check two things:
  - Latency and warm-start of `claude-agent-acp`, which is a Node layer over the Claude Agent SDK.
  - Whether driving the user's own Claude subscription through the Agent SDK via an ACP adapter changes the terms picture from tickets 15 and 18.
- **`codex-acp` licence.** It shows as `NOASSERTION` on GitHub. Check the repo before depending on it.
- **Delegation while a Session is open.** How should a delegated job's progress and permission prompts surface when no Session is open? This interacts with "acts only when spoken to" and with the proactive-behaviour roadmap item.
- **Manifest vs MCP `tools/list`.** Should a Plugin's Manifest be a separate file, or derived from or embedded in MCP `tools/list` metadata (`_meta`)? It needs a Manifest schema decision.
