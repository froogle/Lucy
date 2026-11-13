# 25: Tests that write executables can hit "Text file busy"

**What to build:** Close the last known source of the `ETXTBSY` flake that hung `notify_send`'s tests in slice 3. A test that writes an executable from its own process holds a write fd that a child forked by another test can inherit until it execs; running the file then fails with "Text file busy". `notify_send.rs` now writes its fake from a child process. These still write in-process:
- `piper.rs` tests (the fake Python written around line 417);
- `Harness::install_plugin`, which copies the fixture Plugin into a Plugin folder.

See `docs/agents/working-in-lucy.md` (lucyd test harness, Fake executables).

**Blocked by:** None (can start immediately)

**Status:** ready-for-agent

- [ ] Both write their executables the way `fake_notify_send` does (or another way that leaves no write fd in the test process)
- [ ] `cargo test -p lucyd` run 50x under load shows no "Text file busy" failure
