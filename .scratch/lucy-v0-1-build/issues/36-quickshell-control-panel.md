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

- [x] The bar icon shows the four states and follows lucyd live, including a crash
- [x] The power switch starts and stops `lucyd.service`, and settles to the real state if the command fails
- [x] End Session works while a Session is open
- [x] Tier, STT model and device choices are written to `config.toml` with comments kept, and the flash line says when they apply, offering Restart when needed
- [x] Settings can be changed while lucyd is stopped
- [x] "More settings…" opens `lucy` in a floating terminal
- [x] `lucy status --follow --json` and `lucy set` have harness tests: snapshot shape, the stopped state, applies-when wording, unknown keys rejected
- [ ] The panel is checked by hand on the desktop, noted in Comments

## Comments

Settled in a grilling session with the user (2026-11-15). The ticket first said Waybar; the user meant the Omarchy Quickshell bar, which is what this machine runs. The user's reference was the Omarchy network panel (header with a switch, stats, chip rows, lists).

Built (2026-11-15, branch `ticket-36-quickshell-panel`). `lucy status --json` prints one snapshot and `--follow` one per change: `state` (`stopped`, `starting`, `waiting`, `session`, `failed`) and, for `fast`, `advanced`, `whisper_model` (null while Superwhisper hears) and `mic`/`speaker`, the `key`, `current` and `options`. lucyd's state comes from its socket; while it's away, `systemctl --user is-active` says stopped, starting (active, not yet listening) or failed (`failed`, or `activating` while systemd restarts it after a crash). Devices come from `pactl` and are listed again when `pactl subscribe` reports one added or removed. `lucy set <key> <value>` and `lucy unset <key>` (Whisper's "sized to this computer") take the dotted path and print `{"applies", "message"}` from `Applies`. "Starting" is a fifth snapshot state the ticket didn't name; the icon draws it dimmed, like stopped, and the header says "Starting". The panel's chips and rows are mouse-driven; the keyboard only closes it (Esc) and switches panels (Tab). Tests: `crates/lucy/tests/panel.rs`, plus the link tests in `tests/sections.rs`; the QML runs offscreen through `omarchy-plugin/tests/run.sh` (all checks pass) and `omarchy plugin validate omarchy-plugin` passes.

For the user to check by hand: enable lucyd from `lucy` > Service (or run `ln -sfn "$PWD/omarchy-plugin" ~/.config/omarchy/plugins/lucy-panel` from the checkout), then `omarchy-shell shell rescanPlugins` and add "Lucy" from Omarchy's bar settings or `omarchy plugin enable lucy.panel`. Check the icon dims on `systemctl --user stop lucyd`, changes on "Hey Lucy", turns red on `pkill -SEGV lucyd`; the switch; End Session; a chip and a device (comments kept in `config.toml`, flash line, Restart after Whisper's model); "More settings…" floats `lucy`.

Review fixes (2026-11-15, branch `slice-7-review-fixes`). The Speech-to-text row is one row of chips: Whisper sized to this computer, each of Whisper's models, and Superwhisper where it is installed. The snapshot's `stt` choice replaces `whisper_model`; `lucy set stt <superwhisper|model>` and `lucy unset stt` write `stt.kind` and `whisper.model` together. Superwhisper's warnings stay in the TUI. The waiting line names the Wake word from the snapshot's `wake_word`. Power and Restart run `lucy service start|stop|restart`, which clears a crashed lucyd's start limit first; the QML never runs `systemctl`. An active lucyd that isn't listening after a minute shows as failed, not starting. Symlink: the Service row now asks "Start Lucy whenever you log in?", and each yes enables and makes or refreshes `~/.config/omarchy/plugins/lucy-panel`, even when lucyd is already enabled. The link points at `omarchy-plugin/` beside `lucy`, or else at the checkout whose `target/` holds `lucy`, which is the main checkout under the shared `target/`. If neither has it, the section warns and the enable stands.

Updated hand check: once this is on the main checkout's branch, run `lucy` > Service > Enter > `y`; until then, run `ln -sfn "$PWD/omarchy-plugin" ~/.config/omarchy/plugins/lucy-panel` from this checkout. Then run `omarchy-shell shell rescanPlugins` and `omarchy plugin enable lucy.panel`. Check that the header says "Listening for 'Hey Lucy'". Check that the switch stops lucyd (dimmed icon) and starts it again. Run `pkill -SEGV lucyd` and check the icon turns red, then that the switch brings lucyd back. Check End Session. Pick a Thinking chip. In Speech-to-text, pick Superwhisper if it's installed (flash: next Session), then base.en (flash: after restart, with Restart), checking that `config.toml` keeps its comments. Pick a device, then check that "More settings…" floats `lucy`.
