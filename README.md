# Lucy

[![Watch the video about Lucy](https://img.youtube.com/vi/fcCv_0X4S8k/maxresdefault.jpg)](https://youtu.be/fcCv_0X4S8k)

Lucy is a voice assistant for the Linux desktop. Say "Hey Lucy" and talk to her: she answers out loud, opens apps and pages, finds your files, manages your windows, reads you the news, and passes bigger jobs to your own Claude. When she can't do something yet, she can write a new Plugin for it, and she asks you before using it.

Lucy runs on your own machine and uses what you already have:

- **Thinking** goes through the `claude` or `codex` CLI you're already logged in to, on your existing subscription. Lucy never asks for an API key and never adds a per-use bill.
- **Hearing** is local: [whisper.cpp](https://github.com/ggml-org/whisper.cpp) turns speech into text on your CPU or GPU, and the Wake word is detected on your machine.
- **Speaking** is local too, through [Piper](https://github.com/OHF-Voice/piper1-gpl) or [Kokoro](https://github.com/thewh1teagle/kokoro-onnx).
- **Plugins** run in a sandbox, and each one needs your spoken Approval before Lucy uses it.

> **Status: v0.1, early.** Lucy is developed and tested on [Omarchy](https://omarchy.org) (Arch Linux with Hyprland). The core works on other Linux desktops with PipeWire, but the window-management Plugin, the on-screen Indicator and the bar panel need Omarchy or a wlroots-style Wayland compositor.

## How it works

```mermaid
flowchart LR
    you(["You: “Hey Lucy, …”"]) --> hearing

    subgraph lucyd["lucyd, on your machine"]
        hearing["Hearing<br/>echo canceller → Wake word → whisper.cpp"] --> runner["Session runner"]
        runner --> speaking["Speaking<br/>Piper or Kokoro"]
        mcp["Lucy's MCP server"]
        mcp --> builtin["Built-in tools<br/>apps, pages, files"]
        mcp -- "spoken Confirmation<br/>for risky calls" --> plugins["Plugins<br/>sandboxed with Landlock<br/>desktop, news, …"]
        mcp --> authoring["Authoring<br/>Lucy writes a new Rust Plugin"]
        mcp --> handoff["Hand-off<br/>bigger jobs to your own Claude"]
        authoring -- "your spoken Approval" --> plugins
    end

    runner -- "what you said" --> thinking["Thinking<br/>your claude or codex CLI"]
    thinking -- "reply" --> runner
    thinking -- "tool calls" --> mcp
    speaking --> answer(["Lucy answers out loud"])
```

1. Lucy listens all the time on your machine, and nothing leaves it until she hears the Wake word. Then whisper.cpp turns what you say into text.
2. The text goes to the `claude` or `codex` CLI you're already logged in to. Its reply is spoken sentence by sentence, so Lucy starts talking before it has finished.
3. To act, the model calls tools on Lucy's own MCP server: Built-in tools for apps, pages and files, and Plugins for everything else. Lucy asks you out loud before anything risky, and only a clear yes counts.
4. When no Plugin can do what you ask, Lucy can write one in Rust. Before she uses it, you approve it by voice.

## What you need

- **Linux** with a kernel that has [Landlock](https://docs.kernel.org/userspace-api/landlock.html) turned on (Arch's and most current distros' kernels do). Plugins won't run without it.
- **PipeWire** (PulseAudio and ALSA also work, with Barge-in by speech turned off by default).
- A **microphone and speakers**. Headphones work best: Lucy cancels her own echo, but nothing beats not hearing it.
- **systemd** user services, which run Lucy in the background.
- A **thinking CLI**, logged in with a subscription:
  - [Claude Code](https://docs.anthropic.com/en/docs/claude-code) (`claude`), logged in with a claude.ai account, or
  - [Codex](https://github.com/openai/codex) (`codex`), logged in with a ChatGPT account.
- A **keyring** that speaks the Secret Service API (GNOME Keyring or KeePassXC), if a Plugin needs a password or API key.

## Build and install from source

These steps are for Arch Linux and Omarchy. On other distributions, install the same libraries under their local names.

### 1. Install the build tools and libraries

```sh
sudo pacman -S --needed base-devel git rustup cmake clang pkgconf \
    vulkan-headers vulkan-icd-loader shaderc \
    alsa-lib pipewire libpulse jq uv
```

whisper.cpp is built with Vulkan, so Lucy can use your GPU when there is one. `uv` installs the voice later on.

If `rustup` is new to you, run `rustup default stable` once. The repository pins its own Rust version in `rust-toolchain.toml`, and `cargo` fetches that version the first time you build.

### 2. Get the code and build it

```sh
git clone https://github.com/<owner>/lucy.git
cd lucy
cargo build --release --locked \
    -p lucyd -p lucy -p lucy-indicator -p lucy-news -p lucy-omarchy
```

The first build takes a while: it compiles whisper.cpp and the audio stack.

This builds five programs:

| Program | What it is |
|---|---|
| `lucyd` | The daemon. It listens for the Wake word and runs every Session. |
| `lucy` | The command you use: settings, setup, status and service control. |
| `lucy-indicator` | The small on-screen marker shown while Lucy is listening, thinking or speaking. |
| `lucy-news` | The news Plugin. |
| `lucy-omarchy` | The Omarchy desktop Plugin: windows, workspaces, themes and the screen. |

### 3. Install the programs

Put the four programs you run in `~/.cargo/bin` (make sure it's on your `PATH`):

```sh
for b in lucyd lucy lucy-indicator; do
    install -Dm755 "target/release/$b" "$HOME/.cargo/bin/$b"
done
```

The two Plugins that ship with Lucy go in `/usr/share/lucy/plugins`, where only root can change them. That is what lets Lucy trust them without asking for an Approval:

```sh
for p in news omarchy; do
    sudo install -Dm755 "target/release/lucy-$p" "/usr/share/lucy/plugins/$p/lucy-$p"
    sudo install -Dm644 "crates/lucy-$p/manifest.toml" "/usr/share/lucy/plugins/$p/manifest.toml"
done
```

### 4. Log in to a thinking CLI

Install Claude Code or Codex if you haven't already, then run it once and log in with your subscription account (not an API key):

```sh
claude      # then /login, and choose your claude.ai account
# or
codex login
```

Lucy checks this before each Session. If `claude` would bill an API key instead of your subscription, she refuses to use it, so a forgotten `ANTHROPIC_API_KEY` can't run up a bill.

### 5. Download a speech model

Lucy hears you through Whisper, and needs one of its models on disk. Pick by your hardware:

```sh
mkdir -p ~/.local/share/lucy/models
cd ~/.local/share/lucy/models

# With a GPU (the default when Vulkan finds one): more accurate
curl -LO https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-small.en.bin

# CPU only (the default without a GPU): faster on the CPU
curl -LO https://huggingface.co/ggerganov/whisper.cpp/resolve/main/ggml-base.en.bin
```

If you're unsure, download both: Lucy picks the right one.

### 6. Run setup

```sh
lucy setup
```

Setup walks through each section in turn: the thinking CLI, speech-to-text, Lucy's voice, your microphone and speakers, the Wake word, Plugins and the background service. Press Enter to accept each suggestion. Along the way it installs Piper (or Kokoro) for her voice, lets you hear it, and tests your microphone.

In the last section, **Service**, say yes to starting Lucy at login. That installs and starts the `lucyd` and `lucy-indicator` systemd user services. On Omarchy it also links the bar panel; add `lucy-panel` to your bar to see it.

### 7. Say hello

Say **"Hey Lucy"**. The first time, she introduces herself and asks what to call you.

The [user guide](docs/user-guide/README.md) explains everything she can do and how to set her up the way you like.

## Updating

After pulling new code, run the install script from the checkout:

```sh
git pull
scripts/install.sh
```

It rebuilds what changed, stops Lucy, installs the new programs (and the shipped Plugins, with `sudo`, only when they changed), starts her again and reloads the bar. It needs the services from step 6 to exist already.

## Uninstalling

```sh
systemctl --user disable --now lucyd lucy-indicator
rm ~/.config/systemd/user/lucyd.service ~/.config/systemd/user/lucy-indicator.service
rm ~/.cargo/bin/{lucyd,lucy,lucy-indicator}
sudo rm -r /usr/share/lucy
rm -f ~/.config/omarchy/plugins/lucy-panel
```

Lucy's settings, Persona, models and Plugins are in `~/.config/lucy`, `~/.local/share/lucy` and `~/.local/state/lucy`. Delete those too to remove everything. Credentials you entered for Plugins stay in your keyring under Lucy's name.

## Documentation

- **[User guide](docs/user-guide/README.md)**: getting started, talking to Lucy, Plugins, settings, privacy and troubleshooting.
- **[Developer guide](docs/developers/README.md)**: how Lucy is built, how to work on her, and **[how to write a Plugin](docs/developers/writing-a-plugin.md)**.
- **[Glossary](GLOSSARY.md)**: the words Lucy's code and docs use, and what each means.
- **[Decision records](docs/adr/)**: why Lucy is built the way she is.

## Licence

Lucy is dual-licensed under the MIT licence and the Apache License 2.0, at your option. Piper and espeak-ng are GPL-3.0; Lucy runs them as separate programs and never links them ([ADR 0003](docs/adr/0003-permissive-licence-gpl-out-of-process.md)).
