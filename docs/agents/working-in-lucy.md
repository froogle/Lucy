# Working in Lucy

Facts about the codebase that aren't obvious from reading it.

## Checks

`scripts/check.sh` runs every gate: fmt, clippy, tests, the client-deps check, the glossary-term check and `cargo deny`. The pre-commit hook runs it once you've run `git config core.hooksPath .githooks`.

## lucyd test harness

- **Where it is:** `crates/lucyd/tests/harness/mod.rs` starts lucyd in-process through `lucyd::spawn(Options { .. })` and connects through the real `lucy-ipc` client.
- **Fakes:** they plug in through `Options.adapters`.
- **Real binaries:** the harness runs the `lucy` binary cargo builds next to the test binary. `cargo test -p lucyd` on its own needs `cargo build -p lucy` first; `cargo test --workspace` handles this.
- **Private data:** after a Session ends, `assert_nothing_private_on_bus()` checks that nothing private reached the public socket.

## Adapters (STT, Thinking, TTS)

- **Lifetime:** an adapter keeps the event sender it was given alive for as long as its handle. Today, a dropped sender makes the runner spin on the closed channel.
- **Sentences:** the Thinking adapter finds sentence boundaries itself and emits `SentenceEnd`. The Session sends each sentence to TTS.
- **Placeholders:** `Adapters::inert()` stands in for Providers that haven't been built yet.

## Public IPC

New methods, events and enum variants go in `crates/lucy-ipc/src/schema.rs`. Regenerate the committed schema with `LUCY_UPDATE_SCHEMA=1 cargo test -p lucy-ipc`; a test fails if it drifts.

## Real `claude`

Automated tests use the fake `claude` and the recorded fixtures in `crates/lucyd/tests/fixtures/claude/`. The live check is `cargo test -p lucyd --test claude -- --ignored`, and it spends the user's subscription, so run it only when a ticket needs a real round trip.

## Unix sockets

Socket paths are limited to about 108 bytes. The session scratchpad path is too long for a smoke run, so put the socket under `/tmp` or a short tempdir.

## Implementer subagents

The `tdd` skill asks the user to approve seams first, and a subagent can't ask. The ticket's acceptance checks driven through the harness, plus the adapter traits above, are the approved seams. A ticket that needs a new adapter trait names it in the implementer's report.
