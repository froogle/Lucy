# 31: A menu-driven `lucy` TUI

**What to build:** Replace today's `lucy` home page and the linear `lucy setup` flow with one menu where the user picks what to configure. This changes the shape of the TUI only: it covers the same settings the TUI touches today (spec, Providers and configuration, TUI scope), and thresholds, timeouts, endpointing and paths stay file-only.

**Status:** ready-for-agent

## Layout

- A full-screen list of sections, with a status line at the top showing whether lucyd is running or stopped and whether a Session is open.
- Enter opens a section as its own full-screen page; Esc goes back to the list. Only one level: nothing inside a section opens another menu.
- Keys: arrows and `j`/`k` move, Enter opens or picks, Esc goes back, `q` or Ctrl-C quit from anywhere. Inside a section, single-letter actions stay as today (`c` runs a real check, `r` revokes after a yes) and are shown in a footer line.

## Sections, in order

1. **Thinking:** Providers per tier, each with its status and "run real check".
2. **Speech-to-text:** STT model, with the Superwhisper warnings when it is chosen.
3. **Voice:** the Piper install, offered only when Piper is missing; the voice picker with preview and licences of non-commercial voices.
4. **Audio devices:** mic and speaker, with the level meter.
5. **Wake word:** test it and see its score.
6. **Plugins:** each Plugin's state, and entering its credentials.
7. **Approvals:** the list, with revoke only.
8. **Service:** enable or disable `lucyd.service`; start, stop or restart it.

The current home page goes away; its two panes become Thinking and Approvals.

## First run and `lucy setup`

- When `config.toml` doesn't exist, `lucy` walks the sections in the order above. Enter or Next moves to the next section; Esc leaves the walk and drops into the list, keeping whatever was already picked.
- Once `config.toml` exists, `lucy` opens straight to the list, including after a walk left early.
- `lucy setup` reruns the same walk on purpose. The walk visits the sections; there is no separate setup logic.

## Saving and applying

- Each confirmed change is written to `config.toml` straight away, with comments kept. There is no Save step.
- The section says when the change applies: at the next Session, or after restarting lucyd. When a restart is needed, it offers one. The implementer finds out from lucyd which settings need a restart.

## When lucyd isn't running

Sections that need lucyd (real check, meter, wake-word test, voice preview, Approvals) still open. In place of the part that needs lucyd, they show "lucyd isn't running" and offer to start it with one key. Settings that are only written to the file still work.

## Other commands

- `lucy credentials <plugin>` stays as a shortcut that opens the Plugins section for that Plugin.
- `status`, `events`, `listen`, `end`, `say` and `mcp-relay` are unchanged.

## Out of scope

- Turning Plugins on or off.
- Ticket 36 (Quickshell bar panel), including any command-line option to open `lucy` at a given section.
- Bringing file-only settings into the TUI.

## Acceptance

- [ ] `lucy` opens the section list with the lucyd status line; Enter and Esc move between the list and sections
- [ ] With no `config.toml`, `lucy` walks the sections in order; Esc leaves the walk and keeps what was picked; `lucy setup` reruns the walk
- [ ] Each change is written straight away with comments preserved, and the section says when it takes effect, offering a restart when needed
- [ ] The Service section enables, disables, starts, stops and restarts `lucyd.service`
- [ ] Sections that need lucyd say so when it is stopped and offer to start it
- [ ] `lucy credentials <plugin>` opens the Plugins section for that Plugin
- [ ] Granting Approval never happens in the TUI, only revoking
- [ ] The old home page and linear setup flow are gone, with no duplicated step logic
- [ ] Pages are tested through `TestBackend`

## Comments

Settled in a grilling session with the user (2026-11-15). The user rejected a sidebar layout and asked for a full-screen list you go into and back out of.
