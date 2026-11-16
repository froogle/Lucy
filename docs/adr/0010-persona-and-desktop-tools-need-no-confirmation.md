# The Persona tool and the desktop Built-in tools need no Confirmation or Approval

`update_persona` and the desktop Built-in tools (`open_url`, `launch_app`, `system_status`) run without a Confirmation or an Approval. The Persona is the user's to shape by voice, and these desktop tools act only on the user's own session: they open, launch or report, and change nothing that a Confirmation would protect.

Plugin tools still go through the runtime-owned Confirmation (ADR 0005), and a Plugin still needs its Approval (ADR 0004).

## Considered Options

- A Confirmation for every Built-in tool: asking "Okay?" before opening a page or saying the battery level makes Lucy slow to use for nothing.
