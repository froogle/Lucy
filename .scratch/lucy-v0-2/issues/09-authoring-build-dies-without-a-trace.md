# 09: Authoring builds die at once when `claude` is a mise shim, and leave no trace

**What to build:** Every Authoring build fails at once with "the builder stopped unexpectedly". On 2026-11-16 that happened twice: the `files` Plugin and, earlier, `linear`. The cause was found and confirmed that day:

- lucyd starts the Delegate as `Claude::new("claude", …)` (`crates/lucyd/src/main.rs`), which finds `claude` on `PATH`. On this machine the first `claude` on `PATH` is `~/.local/share/mise/command-wrappers/bin/claude`, a symlink to `/usr/bin/mise`.
- `sandbox::program_dir` canonicalises that to `/usr/bin`, so the Landlock rules make `/usr/bin` readable rather than the real Claude install (`~/.local/share/mise/installs/claude/<version>/`).
- Inside the sandbox, mise can't read `~/.config/mise/config.toml`, so it fails with "Permission denied (os error 13)" and exits 1 before `claude` ever runs. Its stderr goes to `/dev/null`, so lucyd sees only stdout closing and reports `ThinkingEvent::Died`.
- Lucy's Session `claude` isn't sandboxed, so mise works there and execs the real binary. That's why Sessions work and only Authoring fails.

Reproduced by running the mise wrapper under the same Landlock rules (it fails as above). The real binary under the same rules, with the exact Delegate flags, brief, MCP relay and lucyd's environment, runs and answers fine.

Fix it in two parts:

1. **Start the real binary.** Before confining a Delegate, resolve `claude` to the executable that will actually run, not a version-manager shim. Recommended:
   - a `program` field on a Provider in config, used as given when set;
   - otherwise, when the `PATH` hit canonicalises to `mise`, ask `mise which <name>` (unconfined), and use and allow its answer.
   - Do the same for `codex` (`crates/lucyd/src/codex.rs`), which is confined the same way.
   - `lucy setup`'s Provider check should show the resolved path.
2. **Make a dead Delegate diagnosable.** These gaps hid this cause, and the next one will be hidden too:
   - Keep the last lines of a Delegate's stderr instead of sending it to `/dev/null` (as `Build::run` already does for `build.sh` and `test.sh`).
   - Treat an error `result` line (`is_error: true` or an `error_*` subtype) as a failure carrying its message, not an ordinary `TurnEnd`.
   - When a build gives up, log the reason, exit status and stderr tail to the journal, and put a short form in the "couldn't build" notification.
   - Logs still never contain request text, the brief or tool input.

Spec: none. The user found it on 2026-11-16 while using Lucy, and it was diagnosed the same day. It was first filed as lucy-v0-1-build 51 and moved here because v0.1 is done.

**Blocked by:** none

**Seams:** owns the Provider `program` config field and the resolution of shims to real executables (used by the `claude` and `codex` adapters).

**Status:** ready-for-agent

**Notes:**

- Repro, outside Lucy: run `~/.local/share/mise/command-wrappers/bin/claude --version` under Landlock, with `/usr`, `/lib`, `/lib64`, `/bin`, `/sbin`, `/etc`, `/proc` and `~/.cargo/bin` readable, and the workspace, its tmp, `~/.claude`, `~/.claude.json` and `~/.cargo` writable. mise prints the permission error and exits 1.
- Don't fix this by making `~/.config/mise` and `~/.local/share/mise` readable to the Delegate. That widens the sandbox to every tool mise manages, when one binary is all it needs.

**Acceptance:**

- [ ] Harness, fake `claude` reached through a shim-like symlink (or a fake `mise` whose `which` names the real binary): the Delegate starts the real binary, and only its folder is made readable
- [ ] A Provider `program` in config is used as given
- [ ] Harness, fake `claude`: a Delegate that writes to stderr and exits non-zero gives up with its stderr tail in the journal and the notification
- [ ] Harness, fake `claude`: an error `result` gives up with its message, not "stopped unexpectedly"
- [ ] Nothing private reaches the journal or the bus (`assert_nothing_private_on_bus`)
- [ ] Once, by hand on this machine (mise-installed `claude`): "build me a Plugin that …" runs to the Approval

## Comments
