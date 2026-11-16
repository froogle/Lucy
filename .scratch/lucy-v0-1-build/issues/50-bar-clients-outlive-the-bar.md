# 50: The bar's `lucy` clients outlive the bar

**What to build:** When Quickshell restarts or is killed, the `lucy status --follow --json` it ran keeps running, re-parented to systemd. One was found still running 7 minutes after its bar had gone (2026-11-16 hand checks). Each bar restart can leave another one, and a leftover `lucy input --json` would hold the `input` role so that no new bar could claim it. Make the bar's `lucy` subcommands exit when their parent goes: for example, `PR_SET_PDEATHSIG` on Linux, or exiting when stdin closes.

Spec: none; found in the hand checks for tickets 34 and 42.

**Blocked by:** none

**Seams:** none.

**Status:** resolved

**Acceptance:**

- [ ] A test kills the parent of `lucy status --follow --json` and of `lucy input --json`, and each exits soon after, releasing the `input` role
- [ ] Once, by hand: `omarchy-restart-shell` twice leaves exactly one of each running

## Comments

2026-11-16: hand-checked on the redeployed build; passed.
