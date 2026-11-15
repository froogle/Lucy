# 36: A Quickshell bar panel for controlling Lucy

**What to build:** An Omarchy shell (Quickshell) bar-widget plugin: an icon in the bar that opens a panel where the user turns Lucy on or off, ends a Session, and changes her models and audio devices without opening the `lucy` TUI. It is separate from the Indicator, which only shows during a Session. The shape follows the `superwhisper-panel` plugin in `~/.config/omarchy/plugins/`: a `KeyboardPanel` with a header, a power `ToggleSwitch`, rows of choices, and a flash line.

**Blocked by:** 31

**Status:** ready-for-agent

## The icon

Four states: stopped (dimmed), running and waiting for the Wake word, in a Session, and lucyd failed (red).

## The panel

- **Header:** "Lucy", a one-line state ("Stopped", "Listening for 'Hey Lucy'", "In a Session", "lucyd failed"), and a power switch that starts or stops `lucyd.service`. Off means the service is stopped; there is no mute.
- **End Session** button while a Session is open. No button to start one.
- **Thinking:** fast tier and advanced tier, each a row of chips, one per configured Provider.
- **Speech-to-text:** model chips.
- **Audio devices:** mic and speaker, each a row that opens a list.
- **Footer:** "More settings…" opens `lucy` in a floating terminal at the section list (no deep link to a section).

Voice, wake-word test, Plugins and Approvals stay TUI-only.

## How the panel talks to Lucy

The QML holds no product knowledge and never touches the socket or `config.toml` itself. It runs `lucy` subcommands that print JSON:

- `lucy status --follow --json`: long-running, prints one snapshot per change (state and current choices, plus the options for each choice), and reports stopped when the socket goes away.
- `lucy set <key> <value>`: writes `config.toml` through ticket 31's comment-keeping writer, rejects unknown keys, and prints whether the change applies at the next Session or after restarting lucyd. Works with lucyd stopped.

## Applying changes

The flash line says "Applies at the next Session" or "Applies after restart"; in the second case it shows a Restart button. During a Session the chips stay usable, and the change waits for the next Session or restart.

## Installing

The plugin lives in the repo at `omarchy-plugin/`. The existing install step symlinks it to `~/.config/omarchy/plugins/lucy-panel`. The user adds it to the bar from Omarchy's bar settings or `omarchy bar`; Lucy never edits `shell.json`.

## Plugin rules

English only, with strings read through `Commons.Translations` from `i18n/en.json`. Follow the Omarchy plugin rules: `Text.PlainText` for any text from `lucy`, `Style.duration` for motion, `KeyboardPanel` and the `qs.Ui` kit instead of hand-made animation.

## Acceptance

- [ ] The bar icon shows the four states and follows lucyd live, including a crash
- [ ] The power switch starts and stops `lucyd.service`, and settles to the real state if the command fails
- [ ] End Session works while a Session is open
- [ ] Tier, STT model and device choices are written to `config.toml` with comments kept, and the flash line says when they apply, offering Restart when needed
- [ ] Settings can be changed while lucyd is stopped
- [ ] "More settings…" opens `lucy` in a floating terminal
- [ ] `lucy status --follow --json` and `lucy set` have harness tests: snapshot shape, the stopped state, applies-when wording, unknown keys rejected
- [ ] The panel is checked by hand on the desktop, noted in Comments

## Comments

Settled in a grilling session with the user (2026-11-15). The ticket first said Waybar; the user meant the Omarchy Quickshell bar, which is what this machine runs. The user's reference was the Omarchy network panel (header with a switch, stats, chip rows, lists).
