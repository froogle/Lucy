# 13: Plugin host

**What to build:** Approved Plugins become tools the model can call. lucyd scans the user and shipped Plugin folders (a user copy overrides a shipped one by name) and parses each `manifest.toml`. Every enabled, approved, working Plugin tool is listed on the MCP server as `<plugin>__<tool>`, described from `handles` and `examples`, with the input schema saved at Approval. A call starts the Plugin on first use in the Session, under Landlock read/write rules plus its private state dir, and forwards the call over MCP stdio. Plugins stop when the Session ends. Approvals are hash-pinned (Manifest plus executable) in Lucy's own state; in this ticket tests pre-seed them, and the spoken flow comes in 14.

Spec: Plugins; MCP tool server. ADRs 0002, 0004, 0005. Manifest shape is in the spec.

**Blocked by:** 03

**Status:** resolved

- [ ] Manifest parsed and validated (one-line `summary`/`handles`, ≤ 3 examples)
- [ ] At scan, the Manifest is checked against the live `tools/list`; a mismatch marks the Plugin broken
- [ ] Any change to the Manifest or executable makes the Plugin unusable until re-approved; Approvals are stored outside every Plugin folder
- [ ] Broken Plugins (mismatch, failed call) are left out of the tool listing, and Lucy says what failed
- [ ] Landlock blocks reads and writes outside the declared paths (a fixture Plugin proves it)
- [ ] Private writable state dir per Plugin
- [ ] Plugins are stopped at Session end
- [ ] Fixture Plugins (tiny MCP stdio binaries) back every harness test here

## Comments

Resolved on `lucy-v0-1-slice-2` (0a4e21d..890dded). Approvals are kept in `approvals.toml` in Lucy's state, hashed with SHA-256 over the Manifest length, the Manifest and the executable. Scan-time failures go into the Session instructions so Lucy mentions them. `tools/list_changed` is left for ticket 14.
