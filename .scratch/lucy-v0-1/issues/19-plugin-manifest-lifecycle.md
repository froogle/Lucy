# What does a Plugin's Manifest contain, and how are Plugins chosen, approved, updated and removed?

Type: grilling
Status: resolved
Blocked by: 21

## Question

Define the Manifest (what requests it handles, examples, required access and credentials, confirmation-needed actions, version, author: Lucy or human) and how the fast tier picks a Plugin from all Manifests in one call, including what happens when none fits. Define the lifecycle: where Plugins live on disk, install, the user approval step for new or changed Plugins, updates, disabling and removal, and how hand-written and Lucy-written Plugins coexist. Also decide whether the Manifest is its own file or derived from/embedded in the Plugin's MCP tool listing (see [Should Plugins and agent delegation use MCP, A2A, or something else?](21-agent-protocol.md)).

## Answer

Grilled with the user on 2026-11-12. ADR: `docs/adr/0004-manifest-file-hash-pinned-approvals.md`.

- **Manifest is its own file**: `manifest.toml` in the Plugin's folder. It is not derived from MCP. At install Lucy checks it against the live `tools/list`, and a mismatch marks the Plugin broken. The shape:

  ```toml
  name = "photos"
  version = "0.2.0"          # shown to the user; Approval uses the hash
  author = "lucy"            # or "human"
  summary = "Find and show photos from your local photo folders"
  command = ["./target/release/photos"]   # any language, as long as it speaks MCP over stdio

  [[tools]]
  name = "search"
  handles = "find photos by place, date or name"
  examples = ["find my Mount Rainier photo", "photos from June 2024"]
  confirm = false

  [access]
  read = ["~/Pictures"]
  write = []
  network = []               # hosts
  credentials = []           # names only; where secrets live is ticket 20
  commands = ["xdg-open"]
  ```

  `summary` and `handles` are one line each, with at most 3 examples per tool. There is no cap on the number of Plugins in v0.1.
- **Selection**: one fast-tier call takes every Manifest that is enabled, approved and not broken, plus each tool's input schema saved at Approval. It ends in exactly one of four outcomes:
  - call `{plugin, tool, args}`
  - answer directly
  - ask a clarifying question, then select again
  - **no Plugin fits**, which hands over to authoring (ticket 20)

  When two Plugins fit equally, Lucy asks the user which one.
- **On disk**:
  - Lucy-written and user-installed Plugins go in `~/.local/share/lucy/plugins/<name>/`.
  - Shipped Plugins go in `/usr/share/lucy/plugins/<name>/`, and a user copy with the same name overrides the shipped one.
  - Each folder holds `manifest.toml`, the source and the built executable.
- **Approval** (see GLOSSARY):
  - Approvals are stored in `~/.local/state/lucy/approvals.toml`, outside the Plugins, as a hash of the Manifest plus the executable.
  - Any change makes a Plugin unusable until it is approved again.
  - Shipped Plugins (the news briefing) are approved at setup.
  - A new Plugin gets a full spoken summary: what it handles, what it can access, and what it asks before doing. An update speaks only what changed (a one-line summary plus new access, tools or actions that need confirmation), with the full diff on screen (notification or TUI).
- **Access**: filesystem read and write paths are enforced with Landlock. Network hosts are declared and shown but not enforced in v0.1. Credentials are declared by name only.
- **Confirmation**: `confirm = true` on a tool means Lucy asks for a spoken yes before the call. A Plugin may also ask mid-call through MCP MRTR `input_required`.
- **Authorship**: Lucy edits only `author = "lucy"` Plugins. For a human-written one she describes the change for the person, or offers to write her own replacement (which still needs Approval).
- **Updates**: the new version is built alongside the current one, and the current one stays in use until the new one is approved. One previous version is kept, and rolling back to it by voice needs no new Approval, because that hash was approved before.
- **Disable / remove**: both work by voice or from the TUI.
  - Disable keeps the files and the Approval and leaves the Plugin out of selection; enable brings it straight back.
  - Remove deletes the folder and the Approval, after a spoken confirmation.
  - Shipped Plugins can be disabled but not removed.
- **Broken**: a Manifest/`tools/list` mismatch, a failed build or a failure during a call marks the Plugin broken and leaves it out of selection. Lucy says plainly what failed. For her own Plugins she offers a fix through the update flow; for human-written ones she only reports it.
- **Process lifetime**: a Plugin starts on its first use in a Session and is stopped when the Session ends. Slow calls use the MCP Tasks extension within the Session, and nothing runs between Sessions.
- **Hand-installed Plugins**: Lucy finds them on her next scan. They show as "awaiting approval" in the TUI, and Lucy mentions them once at the start of the user's next Session.

## Comments

- From [Which tools does Lucy provide in v0.1, and which need a spoken "yes"?](09-tool-catalogue.md) (2026-11-12): amendment. Each `[[tools]]` entry may set an optional `confirm_prompt` with `{arg}` placeholders, which Lucy speaks as the Confirmation prompt. It is covered by the Manifest hash. Selection is carried out by proxying Plugin tools through Lucy's MCP server (ADR 0005).
- From [How does Lucy write a new Plugin when nothing can handle a request?](20-plugin-authoring-loop.md) (2026-11-13): credentials declared by name are kept in the Secret Service keyring (`oo7`) and passed as env vars at spawn, entered only through the TUI or a desktop prompt. Each Plugin also gets a private state dir `~/.local/state/lucy/plugins/<name>/`. No general OAuth flow in v0.1.
