# Lucy user guide

Lucy is a voice assistant that lives on your Linux desktop. This guide is for people using her. If you haven't installed her yet, start with the [README](../../README.md).

## Contents

1. **[Getting started](getting-started.md)**: the setup walk, the Indicator and bar panel, and your first conversation.
2. **[Talking to Lucy](talking-to-lucy.md)**: opening and ending a Session, cutting in, what she can do, and saying yes.
3. **[Plugins](plugins.md)**: the abilities Lucy can add, how Approval works, and asking her to build one.
4. **[Handing work to Claude](hand-off.md)**: "tell Claude to …", and what it may do on your behalf.
5. **[Settings](settings.md)**: the `lucy` menu, the bar panel, and every setting in `config.toml`.
6. **[Privacy and safety](privacy-and-safety.md)**: what leaves your machine, and what Lucy asks before she does.
7. **[Troubleshooting](troubleshooting.md)**: when she doesn't hear you, doesn't answer, or won't speak.

## Words this guide uses

Lucy's docs use a few words in a particular sense. The full list is in the [glossary](../../GLOSSARY.md).

| Word | Meaning |
|---|---|
| **Wake word** | The phrase that gets Lucy's attention: "Hey Lucy" by default. |
| **Session** | One conversation, from the Wake word until you finish or go quiet. |
| **Turn** | One thing you say, and everything Lucy does in reply. |
| **Barge-in** | Cutting in while Lucy is talking. She stops and listens. |
| **Provider** | Where one of Lucy's abilities comes from: the thinking CLI, speech-to-text, or her voice. |
| **Plugin** | An extra ability, such as the news or window management. |
| **Approval** | Your yes that lets Lucy use a new or changed Plugin. |
| **Confirmation** | Your spoken yes before Lucy does something risky. |
| **Input dialog** | A box on screen for typing something Lucy shouldn't hear, such as a password. |
| **Persona** | Lucy's personality and manner, which you shape. |

## Quick reference

| You want to | Do this |
|---|---|
| Start talking | Say "Hey Lucy", or run `lucy listen` |
| Stop talking | Say "that's all, thanks", click the Indicator, or run `lucy end` |
| Interrupt her | Just start speaking (or say "Hey Lucy") |
| Cancel a long job | Say "cancel it" |
| Change settings | Run `lucy` |
| Walk through setup again | Run `lucy setup` |
| See what she's doing | Run `lucy status` |
| Start, stop or restart her | `lucy service start`, `stop` or `restart` |
| Read her log | `journalctl --user -u lucyd -f` |
