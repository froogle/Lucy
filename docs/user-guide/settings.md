# Settings

There are three ways to change Lucy's settings:

- **The `lucy` menu.** Run `lucy` for the full-screen settings menu (or `lucy setup` to walk through every section in order). This covers everything most people need.
- **The bar panel** on Omarchy, for the settings you change most often.
- **`config.toml`**, for everything else. It lives at `~/.config/lucy/config.toml`. You don't need one: every setting has a default, and Lucy runs without the file.

You can also change one setting from the command line. `lucy` keeps the file's comments and layout:

```sh
lucy set tiers.fast claude-fast
lucy set audio.mic alsa_input.usb-Blue_Yeti-00.analog-stereo
lucy unset whisper.model        # back to the default
```

`lucy set` prints whether Lucy takes the change up at the start of the next Session, or only after a restart (`lucy service restart`). Most settings apply at the next Session.

## Commands

| Command | What it does |
|---|---|
| `lucy` | Opens the settings menu. |
| `lucy setup` | Walks through every section of the menu in order. |
| `lucy credentials <plugin>` | Enters the passwords or API keys a Plugin needs into your keyring. |
| `lucy status` | Shows what Lucy is doing now. `--json` prints it as JSON; add `--follow` to keep printing changes. |
| `lucy listen` | Opens a Session, as if you'd said the Wake word. |
| `lucy end` | Ends the open Session. |
| `lucy set <key> <value>` | Changes one setting in `config.toml`. |
| `lucy unset <key>` | Removes one setting, so its default applies. |
| `lucy service start\|stop\|restart` | Starts, stops or restarts Lucy's background service. |
| `lucy events` | Prints Lucy's raw event stream, for people writing their own widgets. |

## `config.toml` reference

Every setting below is shown with its default.

### Thinking Providers

```toml
[providers.claude-fast]
kind = "claude"           # "claude" or "codex"
model = "haiku"           # passed to the CLI; leave out for the CLI's own default
# allow_api_key = false   # true lets the CLI bill an API key instead of your subscription
# program = "/path/to/claude"   # leave out to find it on PATH

[providers.claude-advanced]
kind = "claude"
model = "opus"

[tiers]
fast = "claude-fast"          # everyday conversation
advanced = "claude-advanced"  # research and writing Plugins
```

Each `[providers.<name>]` section is one thinking Provider; `[tiers]` says which one each Model tier uses. To use Codex for the fast tier:

```toml
[providers.codex-fast]
kind = "codex"
model = "gpt-6-luna"

[tiers]
fast = "codex-fast"
```

Leave `allow_api_key` off unless you mean to pay per use. With it off, Lucy refuses to run a `claude` that would bill an API key.

### `[stt]` and `[whisper]`

```toml
[stt]
kind = "whisper"       # or "superwhisper"

[whisper]
# model = "small.en"   # default: small.en on a GPU, base.en on the CPU
# gpu = true           # default: use the GPU if Vulkan finds one
```

Whisper models are read from `~/.local/share/lucy/models/ggml-<model>.bin`. Changing the model needs a restart.

### `[tts]`, `[piper]` and `[kokoro]`

```toml
[tts]
kind = "piper"              # or "kokoro"

[piper]
voice = "en_GB-cori-medium" # downloaded on first use

[kokoro]
voice = "bf_emma"           # the first letter picks the accent: a = American, b = British
```

### `[audio]`

```toml
[audio]
mic = "default"                 # or a PipeWire node name
speaker = "default"
echo_cancellation = "lucy"      # or "pipewire"
```

`echo_cancellation = "pipewire"` turns Lucy's own echo canceller off, for when you've set up PipeWire's echo-cancel module yourself and pointed `mic` at it.

### `[wake_word]`

```toml
[wake_word]
# model = "/path/to/hey_computer.onnx"   # default: the built-in "Hey Lucy"
threshold = 0.5                          # 0 to 1: higher means fewer false wakes, more misses
```

A Wake word model is a LiveKit WakeWord `.onnx` file. Its file name is the phrase: `hey_computer.onnx` listens for "hey computer". Changing it needs a restart.

### `[session]`

```toml
[session]
silence_timeout_secs = 20   # quiet time before a Session ends
```

### `[barge_in]`

```toml
[barge_in]
mode = "speech"   # or "wake_word": only "Hey Lucy" cuts her off
force = false     # true allows Barge-in by speech on PulseAudio or ALSA
```

The section has more settings for tuning how much speech counts as cutting in (`threshold`, `sustain_ms` and others). Their defaults suit most rooms; see `crates/lucy-config/src/lib.rs` if you need them.

### `[endpointing]`

How Lucy decides you've finished speaking.

```toml
[endpointing]
end_silence_ms = 700        # the pause that ends what you're saying
max_utterance_secs = 30     # longer speech is cut here
```

### `[confirmation]`

```toml
[confirmation]
yes_phrases = ["yes", "yeah", "yep", "go ahead", "do it"]
timeout_secs = 8            # silence that counts as no
```

### `[input]`

```toml
[input]
cancel_phrases = ["cancel", "never mind", "forget it"]
timeout_secs = 120          # how long the Input dialog stays open
```

### `[screen]`

```toml
[screen]
enabled = true    # false: Lucy can't look at or use your screen
```

### `[files]`

Where Lucy looks when you ask her to find files.

```toml
[files]
roots = ["~"]
skip = ["node_modules", "target", ".cache", ".git", "venv", "__pycache__"]
```

Hidden files and folders are always skipped.

### `[desktop]`

```toml
[desktop]
# launcher = ["gtk-launch"]   # the command that starts apps, given the app's desktop entry
```

By default Lucy starts apps through `uwsm app --` when uwsm is running, and `gio launch` otherwise.

### `[authoring]`

```toml
[authoring]
budget_secs = 1200              # how long a Plugin build may take
progress_notifications = true   # show the build's progress as notifications
```

### `[handoff]`

```toml
[handoff]
roots = ["~"]     # where Hand-off folders are looked for by name
budget_mins = 60  # how long a Hand-off may run
```

## Where Lucy keeps things

| Path | What's there |
|---|---|
| `~/.config/lucy/config.toml` | Your settings. |
| `~/.config/lucy/persona.md` | Lucy's Persona. |
| `~/.local/share/lucy/models/` | Whisper and Wake word models. |
| `~/.local/share/lucy/voices/` | Piper voices. |
| `~/.local/share/lucy/kokoro/` | Kokoro, its model and its voices. |
| `~/.local/share/lucy/plugins/` | Plugins you installed or Lucy wrote. |
| `/usr/share/lucy/plugins/` | Plugins shipped with Lucy. |
| `~/.local/state/lucy/` | Approvals, Plugin settings, and each Plugin's private state. |
| `$XDG_RUNTIME_DIR/lucy/lucyd.sock` | The socket Lucy's clients talk to her through. |
