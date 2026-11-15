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

- [x] `lucy` opens the section list with the lucyd status line; Enter and Esc move between the list and sections
- [x] With no `config.toml`, `lucy` walks the sections in order; Esc leaves the walk and keeps what was picked; `lucy setup` reruns the walk
- [x] Each change is written straight away with comments preserved, and the section says when it takes effect, offering a restart when needed
- [x] The Service section enables, disables, starts, stops and restarts `lucyd.service`
- [x] Sections that need lucyd say so when it is stopped and offer to start it
- [x] `lucy credentials <plugin>` opens the Plugins section for that Plugin
- [x] Granting Approval never happens in the TUI, only revoking
- [x] The old home page and linear setup flow are gone, with no duplicated step logic
- [x] Pages are tested through `TestBackend`

## Comments

Settled in a grilling session with the user (2026-11-15). The user rejected a sidebar layout and asked for a full-screen list you go into and back out of.

Built (2026-11-15): `home.rs` and `setup.rs` are gone. `lucy::menu::run` is the one full screen, opened at `menu::Open::{Lucy, Setup, Credentials(plugin)}`; the eight sections are `crates/lucy/src/sections/`, each a page of rows (`ui::Rows`) plus letter actions in the footer. The walk is those same pages with a last "Next" row; the cursor moves on after each pick, so Enter alone takes every suggestion. Esc on a row's question goes back to the page, Esc on a page to the list, Esc on the list quits. A walk left early still writes a one-line `config.toml`, so `lucy` opens at the list next time. Service: Enter toggles start-at-login (enable is `enable --now` of both units, as setup did), `s`/`x`/`r` start, stop, restart. Sections needing lucyd show "lucyd isn't running. s starts it." The scrollback print of setup's transcript is dropped.

For ticket 36: `lucy::settings` is the writer (`settings::set`/`unset`, over `lucy_config::set` and the new `lucy_config::unset`), `settings::Key` names each setting the TUI writes, and `Key::applies() -> Applies::{NextSession, Restart}`, with `Display` giving "Applies at the next Session." / "Applies after restarting lucyd.". From lucyd's code, only Whisper's model needs a restart (loaded once in `main.rs`); Providers, tiers, STT kind, voice, mic and speaker are re-read at each Session start. Installing Piper also needs a restart (lucyd looks for it once). Switching STT to Whisper works at the next Session only if lucyd loaded Whisper at start; `Applies` doesn't model that.
