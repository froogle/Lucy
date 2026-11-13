# A Plugin's Manifest is its own file; Approvals are hash-pinned outside the Plugin

Each Plugin carries a `manifest.toml` beside its code, rather than Lucy deriving the Manifest from the Plugin's MCP tool listing. Lucy has to read every Manifest to choose a Plugin and to present it for Approval, and she must do both without running code nobody has approved yet. MCP also has nowhere to put declared access, authorship or which actions need confirmation. At install, Lucy checks the Manifest's tools against the live MCP `tools/list`, and a mismatch marks the Plugin broken.

Approvals live in Lucy's own state (`~/.local/state/lucy/approvals.toml`), not in the Plugin's folder, so a Plugin (or Lucy while writing one) can never approve itself. Each Approval pins a hash of the Manifest plus the executable, so any change, whoever makes it, needs a fresh yes. Rolling back to a previously approved hash does not. See `.scratch/lucy-v0-1/issues/19-plugin-manifest-lifecycle.md`.

## Considered Options

- Manifest derived from the MCP tool listing (plus annotations): only one source of truth, but the Plugin must be spawned before it is approved, and access and authorship would need non-standard extensions.
- Approval flag stored in the Manifest: simpler, but anything that can edit the Plugin can approve it.
- Version-number-based Approval: a changed Plugin that keeps its version would slip through.
