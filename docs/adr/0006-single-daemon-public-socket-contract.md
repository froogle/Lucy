# One daemon owns everything; clients, the Indicator included, use one public JSON-RPC socket

`lucyd` is the only process that owns audio, the Session, Lucy's MCP tool server, state writes (Approvals, Plugin enable/disable) and every child process. Everything else is a client of one public Unix socket at `$XDG_RUNTIME_DIR/lucy/lucyd.sock`, which speaks newline-delimited JSON-RPC 2.0 with a versioned `hello` handshake and a JSON Schema generated from the Rust types (`lucy-ipc`, published to crates.io). The `lucy` TUI/CLI, our own `lucy-indicator` and third-party widgets (such as an Omarchy shell plugin) are all clients. The Indicator is a separate process: a Wayland or drawing failure can't take down the voice loop, it can be absent on headless or GNOME machines, and a widget can replace it by claiming the `indicator` role. Whenever no client holds that role, lucyd falls back to notifications. The thinking process reaches Lucy's MCP server through a stdio relay (`lucy mcp-relay`) to a separate private socket, so no TCP port is opened and MCP traffic never shares the public contract. The bus never carries transcripts, spoken text or tool arguments, and Approval and Confirmation stay voice-only (ADR 0005). See `.scratch/lucy-v0-1/issues/12-crate-architecture.md`.

## Considered Options

- D-Bus (zbus): Linux-native and introspectable, but heavier for widget authors than a socket you can read with `socat` or Quickshell's `Socket`.
- Indicator as a thread in lucyd: no IPC needed, but it shares a process (and its crashes) with the audio loop, and lucyd would have to handle "no Wayland" itself.
- MCP over streamable HTTP on 127.0.0.1 with a bearer token: any local process could reach the port, so everything would rest on the token.
- TUI setup tests run in-process: `lucy` would link whisper/ort/webrtc, and the tests would compete with lucyd for the mic and trigger Sessions.
