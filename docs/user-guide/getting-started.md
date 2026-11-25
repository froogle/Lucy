# Getting started

This page picks up after you've built and installed Lucy (see the [README](../../README.md)). It explains the setup walk, what you'll see on screen, and your first conversation.

## The setup walk

Run:

```sh
lucy setup
```

`lucy setup` opens a full-screen menu that walks through every section in order. Each section suggests a choice; press **Enter** to take it and move on, or pick another row. **Esc** goes back one step. You can come back to any section later by running `lucy` on its own.

The sections are:

### Thinking

Which thinking CLI Lucy uses. Lucy has two **Model tiers**:

- The **fast tier** handles everyday conversation and understanding what you asked. It should be quick: Lucy suggests Claude's `haiku`.
- The **advanced tier** does research and writes new Plugins. It can be slower and cleverer: Lucy suggests Claude's `opus`.

Each tier names a Provider: `claude` or `codex`, with a model. Setup checks that the CLI is installed and logged in with a subscription, not an API key. You can use Claude for one tier and Codex for the other.

### Speech-to-text

How Lucy turns what you say into text.

- **Whisper, on this computer** (recommended). Runs locally. The model is sized to your hardware: `small.en` on a GPU, `base.en` on the CPU. You downloaded it during install; if you pick a model you haven't downloaded, Lucy tells you it's missing.
- **Superwhisper**, offered only if you have its `superwhisper` command. It adds about a quarter of a second of delay, runs your words through its own clean-up model, and keeps everything you say in its history. Setup warns you about each of these before you choose it.

### Voice

How Lucy sounds.

- **Piper**: fast and light. Setup installs it for you (with `uv`, `pipx` or your distribution's package) and downloads Lucy's default voice, `en_GB-cori-medium`.
- **Kokoro**: more natural, but each sentence takes longer to start. Setup installs it into Lucy's own folder; it downloads about 360 MB.

Pick a voice and Lucy says a sentence in it, so you can hear it before choosing. If no voice is installed, Lucy shows her replies as notifications instead of speaking.

### Audio devices

Which microphone and speaker Lucy uses. "Default" follows whatever your desktop's default is. A meter shows your microphone's level, so you can check she can hear you.

### Wake word

The phrase that wakes Lucy. "Hey Lucy" is built in. This section tests it: say the phrase within a few seconds and setup shows Lucy's score for it, from 0 to 1. She counts the Wake word as heard at 0.5. If she misses it often, or wakes when you didn't call her, change `[wake_word] threshold` in [settings](settings.md#wake_word).

### Plugins

The Plugins Lucy has, which are working, and which are turned off. Turn any of them on or off here. If a Plugin needs a password or API key, enter it here (see [Plugins](plugins.md#credentials)).

### Approvals

The Plugins you've approved. You can revoke an Approval here. You can't grant one here: Approvals are only ever given by voice, so you hear exactly what a Plugin can do before saying yes.

### Service

Whether Lucy starts when you log in. Say yes and setup turns on two systemd user services, `lucyd` (Lucy herself) and `lucy-indicator` (the on-screen marker), and starts them.

On Omarchy, it also links Lucy's bar panel into Omarchy's plugins folder. Lucy never edits your bar's layout, so add the `lucy-panel` widget to your bar yourself.

## What you'll see

### The Indicator

While a Session is open, a small marker sits in the bottom-right corner of your screen. It shows whether Lucy is listening, thinking or speaking. Extra marks appear while she is looking at your screen, building a Plugin, or waiting on a Hand-off. **Click it to end the Session.**

The Indicator needs a Wayland compositor with the layer-shell protocol (Hyprland, Sway and most others; not GNOME). Without it, Lucy uses desktop notifications instead.

### Notifications

Lucy uses ordinary desktop notifications to tell you about things that happen outside a conversation: a Plugin build finishing, a problem, or something she needs to ask you about next time you talk. A notification during a Session may carry an **End** button.

### The bar panel (Omarchy)

The bar panel shows Lucy's state, and opens a panel with her most-used settings: the model for each tier, speech-to-text, microphone and speaker, and a switch to start or stop her. It is a front end to the same settings as `lucy`. When Lucy needs you to type something, the Input dialog opens in this panel.

## Your first conversation

Say **"Hey Lucy"**. The Indicator appears and Lucy starts listening.

The first time, Lucy introduces herself and asks what to call you. She remembers your answer in her [Persona](talking-to-lucy.md#her-persona).

Try:

- "What's the time?"
- "Open Firefox."
- "What's the news?"
- "Find my tax return."
- "Move this window to workspace 3."

When you're finished, say "that's all, thanks", or just stop talking: after 20 seconds of quiet, the Session ends on its own.

Next: [Talking to Lucy](talking-to-lucy.md).
