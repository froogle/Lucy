# Plugin tools are proxied through Lucy's MCP server; Lucy's runtime, not the model, hears Confirmation and Approval

The thinking Provider sees only Lucy's own MCP server. Lucy lists each enabled, approved, working Plugin tool there as `<plugin>__<tool>`, alongside her few Built-in tools. The description comes from the Manifest's `handles` and `examples`, and the input schema is the one saved at Approval. A model call to that tool *is* the Plugin selection. Lucy then starts the Plugin, applies Landlock and `confirm`, and forwards the call; she sends `tools/list_changed` when a Plugin is approved mid-Session.

Confirmation and Approval are handled by Lucy's runtime, never by the model. A tool call that needs Confirmation waits while Lucy speaks a fixed prompt (the tool's `confirm_prompt` from its Manifest, or a generic one built from `handles`). She matches the reply locally against a list of yes-phrases. Anything other than a clear yes, including 8 s of silence, means no. The model only receives `confirmed` or `declined`, and no tool exists that lets it approve a Plugin. A model can mishear, hallucinate a yes, or be steered by prompt-injected content, so a safety gate that the model relays would be no gate at all. See `.scratch/lucy-v0-1/issues/09-tool-catalogue.md`.

## Considered Options

- One `use_plugin{plugin, tool, args}` meta-tool with every Manifest in the system prompt: simpler to build, but the model gets no per-tool schemas, the prompt grows with every Plugin, and Built-in tools and Plugins look different to the model.
- A risky tool returns "needs confirmation" and the model asks the user (the pattern 03's research suggested): no extra listening step in Lucy, but the yes passes through the model, which can invent or misread it.
- Claude Code's own permission prompts (`--permission-prompt-tool`): ties the safety gate to one Provider, and Codex would need a separate path.
