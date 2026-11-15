# 30: No `unwrap()` outside tests

**What to build:** No `unwrap()` outside tests, as `CODING_STANDARDS.md`'s "Errors are values, bugs panic" asks. Something that can fail at runtime returns `Result`; where failure means a bug, `expect` says why it can't happen. Then make it mechanical: Clippy's `unwrap_used` denied for the workspace, with unwraps still allowed in tests.

On 2026-11-15 there are 79, all in `lucyd`; the other crates have none outside tests. 65 are `.lock().unwrap()` on a `std::sync::Mutex`, so lock poisoning is the main decision. By file: `plugins/mod.rs` 19, `authoring/mod.rs` 17, `claude.rs` 14, `piper.rs` 7, `status.rs` 5, `playback.rs` 5, `mcp.rs` 3, `codex.rs` 3, `plugins/manage.rs` 2, `notify_send.rs` 2, others 1 each.

Spec: none; `CODING_STANDARDS.md`.

**Blocked by:** none. Don't run it alongside ticket 29: both touch most of `lucyd`.

**Seams:** none.

**Status:** ready-for-agent

**Notes:**

- **Locks:** a poisoned lock means another thread panicked while holding it, which is a bug. The options are `expect` with the same message everywhere, one small helper that does it, or a mutex without poisoning (`parking_lot`, a new dependency, which `cargo deny` must accept). Pick the one that leaves the least code; say which in the commit.
- **The other 14:** look at each one. If it can fail at runtime (a parse, a channel whose other end can go away), it returns or handles the error. If it can't, `expect` says why.
- **The gate:** a workspace `[lints.clippy]` table in the root `Cargo.toml` with `unwrap_used = "deny"`, each crate taking `[lints] workspace = true`, and `allow-unwrap-in-tests = true` in `clippy.toml`. `scripts/check.sh` already runs Clippy with `-D warnings`.

**Acceptance:**

- [ ] No `unwrap()` outside tests in any crate
- [ ] Every `expect` message states the assumption, not the operation ("the Session thread panicked holding the queue", not "lock failed")
- [ ] `cargo clippy` denies `unwrap_used` outside tests across the workspace, and an `unwrap()` added to non-test code fails `scripts/check.sh`
- [ ] `scripts/check.sh` passes; behaviour is unchanged
