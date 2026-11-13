# 03: MCP server, relay and `end_session`

**What to build:** The model can call Lucy's tools. lucyd runs its MCP tool server on a private 0600 socket. `lucy mcp-relay` (a hidden subcommand) is the stdio bridge the thinking process is configured with. Each spawned child gets a per-spawn token, so lucyd knows who is calling (the Session model, or later a Delegate) and refuses unknown callers. The first Built-in tool, `end_session`, makes the Closing phrase work: Lucy finishes speaking the model's text, then ends the Session.

Spec: MCP tool server; Session state machine (End). ADRs 0005, 0006.

**Blocked by:** 02

**Status:** ready-for-agent

- [ ] Private MCP socket separate from the public socket; no TCP port is opened
- [ ] `lucy mcp-relay <token>` pipes stdio to the private socket; an unknown or missing token is refused
- [ ] Tool listing depends on caller type (Session model vs Delegate), ready for later tickets
- [ ] `end_session` ends the Session after queued speech finishes
- [ ] Harness test: the fake Thinking calls `end_session` through the real relay and the Session ends after speech; a call with a bad token is refused
