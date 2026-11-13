# How is the code split into Rust crates, and how do lucyd and lucy talk to each other?

Type: grilling
Status: resolved
Blocked by: 08, 10, 13, 19

## Question

Lay out the Cargo workspace (daemon, TUI/CLI, MCP tool server, Provider crates, Indicator), process boundaries, the lucyd↔lucy socket protocol, and systemd user-service packaging. Also decide: is the Indicator a thread in lucyd or a separate `lucy-indicator` helper (crash isolation, headless runs)? And define a stable status/control IPC contract that the TUI, Indicator and third-party widgets (e.g. an Omarchy shell plugin) can all consume. Note: cpal can't name streams, so Lucy shows in mixers as `cpal-capture-<pid>`; decide whether to accept that or upstream a fix.

## Answer

Grilled with the user on 2026-11-12. ADR: `docs/adr/0006-single-daemon-public-socket-contract.md`. No glossary changes: `lucyd` and the sockets are implementation names, not domain terms.

- **Processes.**
  - `lucyd` is the only owner of audio (wake word, AEC, VAD, capture and playback), the Session, Lucy's MCP tool server, state writes (Approvals, Plugin enable/disable) and every child process: the `claude`/`codex` process, the Piper server and Plugins. Children die with it (systemd `KillMode=control-group` plus `PR_SET_PDEATHSIG`).
  - `lucy` is a thin CLI and TUI.
  - `lucy-indicator` is a separate client of the public IPC. A Wayland or drawing failure can't take down the voice loop, and the Indicator can be absent.
  - lucyd starts and works with no config file, using defaults.
- **MCP transport.**
  - The `--mcp-config` (or Codex `-c mcp_servers.lucy.*`) names `lucy mcp-relay` as a stdio server.
  - The relay pipes bytes to `$XDG_RUNTIME_DIR/lucy/mcp.sock` (0600). No TCP port is opened and there is no bearer token.
  - lucyd gives each relay a per-spawn token as an argument, so it knows which child is calling (the Session's model or a Delegate) and refuses unknown callers.
- **Public IPC.**
  - **Transport:** `$XDG_RUNTIME_DIR/lucy/lucyd.sock` (0600), newline-delimited JSON-RPC 2.0, separate from the MCP socket.
  - **Contract:** a JSON Schema generated from the Rust types in `lucy-ipc` and committed to the repo. Fields are only ever added within a major protocol version.
  - **Handshake:** `hello {protocol: 1, client, role}`. A major-version mismatch is refused with a clear error, for example when lucyd is still the old version after an upgrade; the TUI then says "restart lucyd".
  - **Requests:**
    - `status` (snapshot)
    - `subscribe {topics}`
    - `session.start`, `session.end`
    - `providers.status`, `provider.check`
    - `plugins.list`, `plugins.set_enabled`
    - `approvals.list`, `approvals.revoke`
    - `test.mic_level`, `test.wake_word`, `test.voice_preview`
  - **Events:**
    - `state {state, session_id}` (Idle, Listening, Thinking, Speaking or Confirming, plus a transient `error`)
    - `level {rms}` at about 30 Hz, only for subscribers to the `level` topic
    - `plugins.changed`
    - `notice {text}`
  - **Never on the bus:** transcripts, spoken text and tool arguments. Granting Approval and Confirmation stay voice-only (ADR 0005).
- **Setup tests.** The TUI's mic level, wake-word score, voice preview and "Reply OK" checks run inside lucyd and are reached over IPC. There is one audio implementation, and a TUI revoke takes effect mid-Session through `tools/list_changed`. The TUI still edits `config.toml` directly with `toml_edit` (08).
- **Crates** (under `crates/`):
  - **Binaries:**
    - `lucyd`
    - `lucy` (CLI, TUI, `mcp-relay`)
    - `lucy-indicator` (sctk + tiny-skia)
  - **Libraries:**
    - `lucy-ipc`: protocol types, a small client and the schema. Published to crates.io.
    - `lucy-config`: config types, defaults and the `toml_edit` writer.
  - **Inside lucyd, as modules:** `audio`, `providers`, `plugins`, `tools` and `session`. `session` is a pure, I/O-free state machine driven by events.
  - **Why the split:** `lucy` and `lucy-indicator` must never pull in whisper-rs, webrtc-audio-processing or ort. A module becomes a crate only for a concrete reason, such as build time or a second consumer.
  - **Left to 20:** a Plugin SDK or template crate.
- **systemd** (user units):
  - **`lucyd.service`:** `PartOf=` and `WantedBy=graphical-session.target`, `After=pipewire.service`, `Restart=on-failure`, no socket activation (the wake word is always listening). `lucy setup` offers to run `systemctl --user enable --now lucyd` once the user approves.
  - **`lucy-indicator.service`:** `PartOf=graphical-session.target`, `Restart=on-failure`, enabled alongside lucyd. With no layer-shell (GNOME) it exits cleanly with a log line.
  - **Notification fallback:** lucyd falls back to notifications whenever no client with `role: "indicator"` is connected. That covers GNOME, headless runs and an Indicator crash, and a third-party widget can claim the role instead.
- **CLI.**
  - `lucy` with no arguments opens the TUI.
  - `lucy setup` runs first-run setup.
  - `lucy status` prints a snapshot.
  - `lucy listen` and `lucy end` start and end a Session. `listen` is documented as an optional Hyprland keybinding, with no default binding shipped.
  - `lucy events` prints the raw NDJSON stream, for widget authors.
  - `lucy mcp-relay` is hidden.
  - If lucyd isn't running, every command except `setup` says so and offers `systemctl --user start lucyd`.
- **Mixer name.** v0.1 accepts that Lucy appears as `cpal-capture-<pid>`. Send an upstream cpal PR letting the PipeWire host set `node.name`, `media.name` and `application.name`, and adopt it when it lands, with no fork.
