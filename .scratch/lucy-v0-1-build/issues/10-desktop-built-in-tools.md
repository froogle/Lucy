# 10: Desktop Built-in tools

**What to build:** "Open github.com", "open Firefox" and "how's my battery?" work by voice. Lucy's MCP server gains `open_url` (http/https only), `launch_app` and `system_status`. `launch_app` matches XDG desktop entries by name, generic name and keywords. It launches through `uwsm app --` when uwsm is running and `gio launch` otherwise (config override). It returns `candidates[]` when several entries match and `not_found` when none do.

Spec: MCP tool server (Built-in tools table).

**Blocked by:** 03

**Status:** resolved

- [ ] `open_url` rejects non-http(s) schemes
- [ ] `launch_app` handles exactly one match, several, and none, with no fallback to `request_plugin`
- [ ] `system_status` topics: battery, disk, memory/CPU, network, volume, time, uptime
- [ ] None of these tools needs Confirmation
- [ ] Harness tests for each tool's contract, using fake desktop entries and a fake launcher

## Comments

Resolved on `lucy-v0-1-slice-2` (0a4e21d..890dded). Adds the `Launcher` seam; the `[desktop] launcher` override applies to `launch_app` only. The `system_status` topic is `memory_cpu`. Desktop entries are read one level deep.
