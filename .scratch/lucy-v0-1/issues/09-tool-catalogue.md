# Which tools does Lucy provide in v0.1, and which need a spoken "yes"?

Type: grilling
Status: resolved
Blocked by: 03, 19

## Question

Pin the v0.1 MCP tool catalogue (open_url, launch_app, read_page, system_status, speak, …), each tool's inputs/outputs, and the confirmation policy: which actions are free, which need spoken confirmation, and how opt-in raw shell is gated. Now that Plugins exist: decide which capabilities are built into Lucy (e.g. open_url, launch_app, speak, system_status) versus shipped as Plugins, and whether the confirmation policy is declared in each Plugin's Manifest.

## Comments

- From [What does a Plugin's Manifest contain, and how are Plugins chosen, approved, updated and removed?](19-plugin-manifest-lifecycle.md) (2026-11-12): settled. Confirmation is declared per tool in the Manifest (`confirm = true` means a spoken yes before the call), and a Plugin can also ask mid-call through MRTR `input_required`. What remains here is the built-in tool catalogue and its own confirmation policy.

## Answer

Grilled with the user on 2026-11-12. ADR: `docs/adr/0005-proxied-plugin-tools-runtime-owned-confirmation.md`. Glossary: added **Confirmation** and **Built-in tool**.

- **How Plugins reach the model.** Lucy's MCP server lists each enabled, approved, working Plugin tool as `<plugin>__<tool>`, next to the Built-in tools.
  - The description comes from `handles` and `examples`. The input schema is the one saved at Approval.
  - A model call to that tool *is* the selection from 19.
  - Lucy starts the Plugin on first use, applies Landlock and `confirm`, and forwards the call.
  - When a Plugin is approved mid-Session, she sends `tools/list_changed`.
- **Confirmation belongs to Lucy's runtime, not the model.**
  - The tool call waits while Lucy speaks a fixed prompt.
  - She listens for one utterance and matches it locally against a list of yes-phrases ("yes", "yeah", "yep", "go ahead", "do it"). The list can be changed in `config.toml`.
  - Saying "yes" while she is still speaking the prompt counts.
  - Anything other than a clear yes means no, and so does 8 s with no answer. Lucy then says "Okay, I won't."
  - The model only receives `confirmed` or `declined`.
- **Approval belongs to Lucy too.** No model-callable approve tool exists.
- **Confirmation prompt.** A Plugin tool can set an optional `confirm_prompt` with `{arg}` placeholders, for example `"Send an email to {to} about {subject}?"`.
  - It is part of the Manifest, so it is covered by the hash and the Approval.
  - Without one, Lucy builds the prompt from `handles` ("Use photos to find photos by place, date or name?").
  - The full arguments are always shown in a notification. This amends 19.
- **Built-in tools** need no Approval, so there are only a few of them. A capability is built in only if it touches Lucy's internals or is a small, trusted desktop action needed everywhere. Everything else is a Plugin.

  | Tool | Inputs → output | Confirmation |
  |---|---|---|
  | `open_url` | `url` (http/https only) → ok | no |
  | `launch_app` | `name` → launched / `candidates[]` / `not_found` | no |
  | `system_status` | `topic` ∈ battery, disk, memory/CPU, network, volume, time, uptime → facts | no |
  | `request_plugin` | `goal`, `clarifications` → starts the authoring Delegate (19's "no Plugin fits"); Approval comes later | no |
  | `manage_plugin` | `plugin`, `action` ∈ enable, disable, rollback, remove → result | remove only |
  | `end_session` | → ends the Session (the closing phrase as understood by the model) | no |

- **`launch_app`.**
  - **Matching:** Lucy matches the spoken name against XDG desktop entries by name, generic name and keywords.
  - **One clear match:** she launches it.
  - **Several matches:** she returns the candidates, and the model asks which one.
  - **No match:** she returns `not_found`. There is no fallback to `request_plugin`.
  - **Launcher:** chosen at runtime. Lucy uses `uwsm app --` when uwsm is running and `gio launch` otherwise. `config.toml` can override it.
- **No overrides in v0.1.** For a Plugin, changing `confirm` means changing its Manifest, which needs a fresh Approval. For Built-in tools it is fixed in Lucy.
- **Dropped:**
  - `speak`: the model's text is already spoken.
  - `read_page`: reading web pages is left to Plugins; the news briefing fetches its own pages.
  - Memory tools: they stay in the Memory fog.
- **Raw shell** is out of scope for v0.1.
