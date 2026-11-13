# Herdr as a transport for Lucy's thinking Provider

Ticket: [04-herdr-interface](../issues/04-herdr-interface.md). Researched 2026-11-12. The research was read-only: I sent no prompts, created no panes and changed nothing in the live session.

## Sources

- **[skill]**: `~/.claude/skills/herdr/SKILL.md`. This is the herdr agent skill, the same text that `herdr --skill` prints.
- **[hook]**: `~/.claude/hooks/herdr-agent-state.sh`. Herdr installed this Claude Code integration hook.
- **[cli]**: the local `herdr` 0.9.1 binary. I read `herdr --help`, the help for `herdr agent`, `pane`, `session`, `integration` and `api`, plus `herdr status` and `herdr agent list`.
- **[schema]**: `herdr api schema --json` (protocol 22, schema_version 1). This is the JSON Schema of every socket method and event that the installed binary supports.
- **[docs-socket]**: https://raw.githubusercontent.com/herdrdev/herdr/v0.9.5/docs/next/website/src/content/docs/socket-api.mdx
- **[docs-auto]**: https://raw.githubusercontent.com/herdrdev/herdr/v0.9.5/docs/next/website/src/content/docs/agent-automation.mdx
- **[repo]**: https://github.com/herdrdev/herdr. The licence is Apache-2.0 (from `LICENSE` and the `Cargo.toml` `license` field). The project is written in Rust, and the latest release is v0.9.5 (2026-10-14).

## Two integration surfaces

1. **CLI.** `herdr agent …` and `herdr pane …` return JSON on stdout. Server errors come back as JSON on stderr with exit code 1; syntax errors exit with code 2 [skill].
2. **Socket API.** The CLI is a thin wrapper over this. Clients send newline-delimited JSON over a Unix domain socket (a named pipe on Windows), one request per line [docs-socket]. The envelope looks like this:
   - Request: `{"id":"req_1","method":"agent.prompt","params":{...}}`
   - Success: `{"id":…,"result":{"type":…}}`
   - Error: `{"id":…,"error":{"code":…,"message":…}}`

   The hook speaks this protocol directly with roughly 20 lines of Python [hook]. A Rust client is therefore just `UnixStream` plus `serde_json`, and needs no Herdr crate.

Herdr finds the socket in this order: `--session <name>`, then `HERDR_SOCKET_PATH`, then `HERDR_SESSION=<name>`, then the default `~/.config/herdr/herdr.sock`. Named sessions live at `~/.config/herdr/sessions/<name>/herdr.sock` [docs-socket]. Lucy runs as a daemon outside Herdr, so `HERDR_SOCKET_PATH` won't be set. She has to resolve the default path herself or take a configured session name.

## Q1: targeting an agent

- **Hierarchy.** Herdr has workspaces (`w1`), tabs (`w1:t1`) and panes (`w1:p1`). IDs are opaque and stable, and closed IDs are never reused [skill]. A pane that moves to another workspace gets a new ID [skill].
- **Agent targets.** `agent.*` methods take a `target` that is either a unique live **agent name** (`[a-z][a-z0-9_-]{0,31}`) or the pane ID that currently hosts an agent [skill][schema `AgentTarget`].
  - A name follows its occupant: it is cleared when the agent exits, is released or is replaced [skill].
  - Names are set with `agent start <name>` or `agent rename` [cli].
- **Discovery.** `agent.list` returns an `AgentInfo` for each agent [schema]. Its fields are:
  - `agent` (kind, for example `claude`), `name`, `agent_status`
  - `pane_id`, `workspace_id`, `cwd`, `terminal_title`
  - `agent_session` (`{source, agent, kind: id|path, value}`)
  - `interactive_ready`, `state_change_seq`

  On this machine all three live agents are `claude`, and none has a `name`. The user would have to name a pane (for example `lucy`) for Lucy to target it stably [cli `agent list`].
- **Kinds Herdr recognises.** pi, claude, codex, gemini, cursor, devin, agy, cline, omp, mastracode, opencode, copilot, kimi, kiro, droid, amp, grok, hermes, kilo, qodercli, qwen, letta, maki and muse [cli].
- **Remote machines.** `--machine <label>` forwards API calls to a saved SSH machine [skill]. Lucy doesn't need this for v0.1.

## Q2: sending a prompt

`agent.prompt {target, text, wait?: {until?: [AgentStatus], timeout_ms?}}` [schema]. On the CLI this is `herdr agent prompt <target> "<text>" --wait --timeout 120000`.

- It writes the text and then Enter as one submission, and honours the pane's bracketed-paste mode [skill][docs-auto].
- If the agent is sitting at an approval or question dialog, it returns `agent_blocked` without sending anything [docs-auto].
- Putting `wait` inside the same request avoids a race between submitting and waiting [docs-socket].
- **What it does not do:** it just types into a TUI. There is no per-request ID, no structured turn and no way to attach a system prompt or tools. The prompt joins whatever conversation the pane already holds.

## Q3: detecting completion

Herdr tracks a lifecycle state for each agent: `idle | working | blocked | done | unknown` [schema `AgentStatus`].

- **Settled states.** `idle` and `done` both mean the agent is ready for input. `blocked` means Herdr recognised an approval or question UI. `unknown` "does not prove successful completion" [docs-auto].
- **Prompting with `--wait`.** After submitting, Herdr must observe `working` or `blocked` within 5 seconds, otherwise it returns `agent_prompt_stalled`. It then waits for the first settled `idle`, `done` or `blocked` [skill][docs-auto].
- **The wait is not tied to a turn.** It tracks lifecycle state, not one turn: "If the agent is already working, completion of that active turn may satisfy the wait" [docs-auto]. Timeouts and stalls do not prove the prompt was never delivered, so Lucy must not blindly retry [skill][docs-auto].
- **Push events.** `events.subscribe` with `{"type":"pane.agent_status_changed","pane_id":…}` keeps the connection open: the first line acknowledges and later lines are pushed events [docs-socket][schema].
  - If the server falls behind, it sends `events_lost` and the client must resubscribe and reconcile using `session.snapshot` [docs-socket].
  - Other subscribable events: `pane.output_matched` (substring or regex on pane text), `pane.exited` and `pane.agent_detected` [schema].
- **How state is detected.** For Claude, state comes from Herdr's integration hooks plus screen heuristics [hook][cli `pane report-agent`]. It is accurate enough for a status badge, but it is still inference, not a protocol.

## Q4: receiving the reply (the weak point)

- **The prompt result carries no reply text.** `agent.prompt` returns only `{"type":"agent_prompted","agent":AgentInfo}` [schema].
- **Reading the screen.** The reply has to be scraped from the terminal with `agent.read {target, source, lines?, format, strip_ansi}`, which returns `.result.read.text` [schema][docs-auto]. The sources are:
  - `visible`
  - `recent` (80 rows by default)
  - `recent_unwrapped`
  - `detection`
- **Limits of screen reading.**
  - It returns rendered TUI rows, including UI chrome, tool-call boxes and the prompt echo. It is not the model's message text.
  - For full-screen agents like Claude Code, Herdr scrolls an idle agent to recover history. While the agent is working, an explicit `--lines` read returns `agent_not_idle` [docs-auto].
  - Very long replies may not be recoverable this way. Herdr's own fallback is to ask the agent to write Markdown to a temp file and reply with only the path [skill][docs-auto].
- **No token streaming.** The closest thing is polling `agent.read` or subscribing to `pane.output_matched`, both of which work on rendered screen text.
- **Possible better channel (unverified).** `AgentInfo.agent_session` exposes the agent's own session ID (`kind: id`) or transcript path (`kind: path`) [schema]. Herdr's Claude hook reports both `session_id` and `transcript_path` [hook], and live panes here show a Claude session UUID [cli].
  - Lucy could use that ID to tail Claude Code's own JSONL transcript and get the clean assistant text.
  - This is Claude-specific, and it relies on another tool's private file format. I have not verified it.

## Assessment: can Herdr serve as a transport for the thinking Provider?

**Yes, as an optional "attach to an agent I already have running" transport, but not as the default.**

### For

- It reuses agents that are already warm and already subscribed, with no extra spawn latency.
- The user can watch and steer the agent in Herdr while Lucy talks to it.
- It works with many agent kinds through one interface.
- The local JSON socket API comes with a published schema and an Apache-2.0 licence, which is compatible with Lucy's MIT/Apache licensing.

### Against

- Replies are scraped from the screen, not structured. They need cleanup per agent kind, and long replies can be truncated.
- There is no streaming, so Lucy can't start text-to-speech on the first sentence. That hurts voice latency.
- There are no turn IDs, and completion is a lifecycle heuristic.
- The pane carries shared conversation state with the human. If the user is typing in that pane, Lucy's prompt collides with them.
- Lucy cannot inject her MCP tool server or her system prompt. The agent was started by someone else, with its own configuration, so Lucy's actions/MCP design only applies if the user launched that agent with Lucy's MCP configured.
- The JSON API has no semantic-versioning promise. The guidance is "ignore unknown fields and handle unsupported methods as normal errors", plus checking `ping` / `herdr status` [docs-socket]. The local server (0.9.1) already lags upstream (0.9.5).
- `blocked` dialogs (tool approvals) need a policy. Lucy could speak the question, or refuse and tell the user to look at Herdr.

### Recommended integration surface (`HerdrTransport`)

- **Configuration.** Optional session name or socket path, plus a target agent name (for example `lucy`). Lucy never targets by focus. If the named agent isn't live, she fails loudly rather than guessing.
- **Connection.** A raw `UnixStream` that sends NDJSON. Don't shell out to the CLI per call, except perhaps in a prototype.
- **Per turn:**
  1. `agent.get`. If the agent is `blocked` or `working`, report "your Herdr agent is busy" and send nothing.
  2. `agent.prompt` with `wait: {timeout_ms}`.
  3. On a settled state, `agent.read` with `recent_unwrapped`, then strip everything up to and including the echoed prompt.
  4. Alternatively, for `claude`, read the reply from the Claude transcript via `agent_session`. This needs a spike.
- **While a turn runs.** Optionally `events.subscribe` to `pane.agent_status_changed`, for the Indicator's "thinking" and "needs approval" states.
- **Never** `agent.start`, `pane.split`, close anything, or answer `blocked` dialogs on the user's behalf without spoken confirmation.

## Open questions surfaced

- Can a Claude Code transcript (located via `agent_session`) be tailed reliably for clean, incremental reply text? This would fix both the reply-quality and the streaming gaps for the default agent kind.
- When the target agent is `blocked` on a tool approval, should Lucy voice the dialog and relay the answer through `agent.send_keys`, or decline? This ties into the spoken-confirmation policy for risky actions.
- Should Lucy start her own named agent in a Herdr pane (with Lucy's MCP server and system prompt), or only attach to agents the user started? Starting her own one creates layout in the user's session, which the herdr skill discourages unless requested.
