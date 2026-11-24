# Working in Lucy

Facts about the codebase that aren't obvious from reading it.

Read the files under `working-in-lucy/` that your change touches:

- `harness.md`: any change with lucyd tests. The in-process harness, its fakes and clock, fake executables, and the private-data and model-text checks.
- `providers.md`: an adapter or Provider (STT, Thinking, TTS), or running real `claude`, `codex`, Piper or Kokoro.
- `hearing.md`: the mic, VAD, Whisper, the Wake word, Barge-in, the echo canceller, capture and playback.
- `plugins.md`: Plugins, Manifests, the sandbox, screen tools, the MCP relay, Confirming, credentials and the Input dialog.
- `authoring-and-hand-off.md`: Delegates building Plugins, and Hand-offs to the user's own CLI.
- `setup.md`: `lucy`'s full-screen pages, `config.toml` settings, their tests and the systemd units.
- `panel.md`: the Omarchy bar panel.

## Checks

`scripts/check.sh` runs every gate: fmt, clippy, tests, the client-deps check, the glossary-term check, the lint-allow check, the test-sleep check, the comment check, the rustdoc-summary check and `cargo deny`. The pre-commit hook runs it once you've run `git config core.hooksPath .githooks`; a commit that only touches markdown, `docs/` or `.scratch/` runs just the glossary check. Commit with the hook on; `--no-verify` leaves the gates to CI.

`target/` only grows (108 GB by slice 6). When a ticket is done and merged, with no agents running, run `scripts/clean.sh`: it clears the `target/` this checkout builds into (`CARGO_TARGET_DIR`, else its own) and the prototypes' own `target/` folders, prunes stale worktrees and says how much it freed. It refuses while another worktree builds into that same `target/`; a worktree with a `target/` of its own doesn't count. Run it inside such a worktree, with `CARGO_TARGET_DIR` unset, to clear that one. The next build is cold. Run it after removing a worktree too: its test binaries stay in the shared `target/` and still look for fixtures and the schema under the removed worktree's path.

`scripts/install.sh` puts this checkout on the machine: it builds the five binaries, stops `lucyd` and `lucy-indicator`, installs them into `~/.cargo/bin`, copies the shipped Plugins into `/usr/share/lucy/plugins/` with sudo when they differ, starts both again and reloads the bar. The systemd units come from `lucy setup`, not from it.

## Public IPC

New methods, events and enum variants go in `crates/lucy-ipc/src/schema.rs`. Regenerate the committed schema with `LUCY_UPDATE_SCHEMA=1 cargo test -p lucy-ipc`; a test fails if it drifts.

## Unix sockets

Socket paths are limited to about 108 bytes. The session scratchpad path is too long for a smoke run, so put the socket under `/tmp` or a short tempdir.

## Implementer subagents

The `tdd` skill asks the user to approve seams first, and a subagent can't ask. The ticket's acceptance checks driven through the harness, plus the adapter traits (`providers.md`), are the approved seams. A ticket that needs a new adapter trait names it in the implementer's report. The ticket's `Seams:` line says which shared seams it owns and which it only uses.

The last step before reporting: stop every background Bash task you started (TaskStop each one) and confirm none remain. A running one keeps your worktree locked.

Worktrees share the main checkout's `target/` and build with 8 jobs, through `CARGO_TARGET_DIR` and `CARGO_BUILD_JOBS` in the machine's `.claude/settings.local.json`. Keep them as set. "Blocking waiting for file lock on build directory" means another worktree is building; wait for it. On a new machine, create that file with its own checkout's path:

```json
{ "env": { "CARGO_TARGET_DIR": "/path/to/lucy/target", "CARGO_BUILD_JOBS": "8" } }
```

Without it, each worktree builds everything cold in its own `target/`, and parallel agents can stall the desktop.

## Merging

A merge that combines two tickets' changes to the same seam (both edit one adapter trait, the Session start, the notification fallback) gets one harness test of the combination before the merge commit. Each ticket's own tests pass alone; the combination is where the bugs were in slice 2 (a refused Provider swallowing the first-run introduction; a TTS-failure notification replacing the End button).
