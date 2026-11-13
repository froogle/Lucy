# How does Herdr accept agent requests and return replies?

Type: research
Status: resolved
Blocked by: none

## Question

The user runs Herdr core in the background with agents ready to receive requests. Find out (via the herdr skill and the local herdr CLI/docs) how an external program sends a prompt to an agent pane, receives its reply (streaming or final), detects completion, and targets a specific agent/workspace. Assess whether Herdr can serve as a transport for Lucy's thinking Provider and what the integration surface looks like.

## Answer

Herdr can serve as an optional "attach to a running agent" transport, but it should not be the default. Lucy can talk NDJSON over Herdr's Unix socket (`~/.config/herdr/herdr.sock`, Apache-2.0, schema from `herdr api schema --json`) and target a named agent:
- `agent.prompt` with `wait` to submit the prompt and wait for a settled `idle`/`done`/`blocked`;
- then `agent.read` (`recent_unwrapped`) for the reply;
- and `events.subscribe` (`pane.agent_status_changed`) for live state.

The catches are that replies are scraped from the terminal screen (no streaming, no turn IDs, long replies can be truncated), that completion is a lifecycle guess, and that Lucy can't inject her MCP server or system prompt into an agent the user started. Keep the persistent Claude Code Provider as the default, and treat Herdr as a transport that fails loudly when the named agent is missing, busy or blocked.

[findings](../research/herdr-interface.md)
