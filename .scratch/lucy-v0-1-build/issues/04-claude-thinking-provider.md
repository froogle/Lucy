# 04: Claude Thinking Provider

**What to build:** Real conversation with Claude on the user's subscription. When a Session opens, lucyd pre-spawns one persistent `claude -p` stream-json process with an isolated config: a strict MCP config naming only the relay, no setting sources, no built-in tools, `dontAsk` permissions, and only Lucy's MCP tools allowed. Partial messages are chunked into sentences, and each sentence goes to TTS as soon as it is complete. The child environment is scrubbed of anything that outranks the subscription. The process ends with the Session.

Spec: Providers and configuration (`claude`, subscription guard). ADR 0001. Stream shapes: the voice-loop prototype (wayfinder 07).

**Blocked by:** 03

**Status:** ready-for-agent

- [ ] Process is pre-spawned on Session start and reused for every Turn in the Session; never passes `--bare`
- [ ] Child env drops the Bedrock/Vertex/Foundry switches, `ANTHROPIC_AUTH_TOKEN`, `ANTHROPIC_API_KEY` and `apiKeyHelper`, and keeps `CLAUDE_CODE_OAUTH_TOKEN`
- [ ] Sentence chunking hands each completed sentence to TTS straight away
- [ ] `rate_limit_event`s are parsed and exposed to the Session (used by 11)
- [ ] Default system prompt asks for one to three short spoken sentences, no markdown, and a "Let me check" before slow tools (Persona arrives in 16)
- [ ] The fake Thinking adapter's stream matches recorded real stream shapes
- [ ] Opt-in live test against a real `claude` login: a two-Turn exchange keeps context
