# 01: Workspace, lucyd and public IPC tracer

**What to build:** The thinnest end-to-end path through the system. lucyd starts with no config file and listens on the public socket. A client completes the versioned `hello` handshake and gets a `status` snapshot showing Idle. `lucy status` prints it, and `lucy events` streams raw NDJSON. This ticket also lays the groundwork every later ticket builds on:
- the five-crate workspace;
- the lucyd test harness that later tickets extend, which starts lucyd in-process with swappable adapters and connects through the real `lucy-ipc` client;
- the CI gates.

Spec: [../../lucy-v0-1/spec.md](../../lucy-v0-1/spec.md) (Processes and crates; Public IPC contract; Testing Decisions). ADR 0006.

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Workspace has the binaries `lucyd`, `lucy` and `lucy-indicator` (stub) and the libraries `lucy-ipc` and `lucy-config`; `lucy` and `lucy-indicator` don't depend on any audio or ML crates
- [ ] lucyd runs with no config file, using defaults from `lucy-config`
- [ ] Public socket in the runtime dir with mode 0600, carrying NDJSON JSON-RPC 2.0; `hello {protocol, client, role}` is answered, and a major-version mismatch is refused with a clear error
- [ ] `status` and `subscribe {topics}` work; `state` events are emitted (Idle only for now)
- [ ] `lucy status` prints the snapshot; `lucy events` prints the raw stream; every command except `setup` says plainly when lucyd isn't running and offers the start command
- [ ] JSON Schema is generated from the `lucy-ipc` types and committed; CI fails if it drifts
- [ ] `cargo-deny` licence allow-list (permissive only) runs in CI
- [ ] Harness test: start lucyd, handshake, assert an Idle `status`
