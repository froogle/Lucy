# 42: The Input dialog in the Quickshell bar plugin

**What to build:** On Omarchy, the Lucy bar plugin (ticket 36) draws the Input dialog. It holds the `input` role on lucyd's socket, shows the question and its fields in the bar's style, and sends the answer or a cancel back. Secret fields are masked, and their values are never kept in QML state after sending.

Spec: [v0.1](../../lucy-v0-1/spec.md) as amended 2026-11-16: Public IPC (Roles), Testing (Input dialog). Story 163.

**Blocked by:** 34

**Seams:** uses the `input` role and messages (owned by 34). The bar plugin's own files only.

**Status:** ready-for-agent

**Notes:**

- The QML holds no product knowledge, as in ticket 36. It talks to lucyd through a `lucy` subcommand, not the socket directly, if that keeps it in line with `lucy status --follow`. Decide while building and say which.
- Field kinds to draw: text, url, number, choice, secret.

**Acceptance:**

- [ ] With the bar plugin running, a `secret` question opens in the bar; the value is masked, sent once and cleared
- [ ] Cancel and Esc send `input.cancel`; `input.close` from lucyd closes it
- [ ] `omarchy-plugin/tests/run.sh` covers it offscreen against a fake `lucy`; `omarchy plugin validate omarchy-plugin` passes
- [ ] Once, by hand: a Plugin with a missing credential asks for it in the bar, and the call carries on

## Comments
