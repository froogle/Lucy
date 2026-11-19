# Troubleshooting

Start with two commands:

```sh
lucy status                       # is Lucy running, and what is she doing?
journalctl --user -u lucyd -e     # the end of her log
```

For much more detail in the log, add `Environment=RUST_LOG=lucyd=debug` to the `[Service]` section of `~/.config/systemd/user/lucyd.service`, then run `systemctl --user daemon-reload` and `lucy service restart`.

## Lucy doesn't respond to "Hey Lucy"

- **Is she running?** `lucy status` says so. If not, `lucy service start`, then check the log for why she stopped.
- **Is she hearing you?** Run `lucy`, open **Audio devices**, and watch the microphone meter while you speak. If it doesn't move, pick another microphone.
- **Is she hearing the Wake word?** In `lucy`, open **Wake word** and test it. A score near the threshold (0.5) means she only just hears it: speak a little more clearly, move closer, or lower `[wake_word] threshold` in `config.toml`.
- **Try `lucy listen`.** If a Session opens and she answers, everything but the Wake word is fine.

## She opens a Session but doesn't understand me

- **"The Whisper model … is missing"** in the log: download the model it names into `~/.local/share/lucy/models/` (see the [README](../../README.md#5-download-a-speech-model)), then `lucy service restart`.
- **She mishears you a lot:** on a GPU, try the more accurate `small.en` model.
- **She cuts you off mid-sentence:** raise `[endpointing] end_silence_ms` (the default is 700 ms).

## She hears me but doesn't answer

Usually the thinking CLI is the problem. Run `lucy`, open **Thinking**, and check each tier.

- **Not installed or not logged in:** run `claude` (or `codex login`) yourself and log in with your subscription account.
- **Refused because it would bill an API key:** `claude` is set up to use an API key, through an `ANTHROPIC_API_KEY` variable or an `apiKeyHelper` in its settings. Log in with your claude.ai account instead. (If you really want to pay per use, set `allow_api_key = true` for that Provider.)
- **`config.toml` doesn't parse:** Lucy says so when you wake her. Fix the line it names, or `lucy unset` the setting.

## She answers in notifications instead of speaking

Her voice isn't installed. Run `lucy`, open **Voice** and pick Piper or Kokoro; setup installs it. If installing Piper fails, install it yourself:

```sh
uv tool install 'piper-tts[http]'
```

Note that Arch's `piper` package is a mouse-configuration tool, not this Piper.

## She keeps interrupting herself

Lucy hears her own voice through your speakers and takes it for you cutting in. Try, in order:

1. Use headphones.
2. Turn the speaker volume down, or move the microphone away from the speakers.
3. Set `[barge_in] mode = "wake_word"`, so only "Hey Lucy" can cut her off.

## I can't cut in while she's talking

- On PulseAudio or ALSA, Barge-in by speech is off by default. Say "Hey Lucy" to cut in, or set `[barge_in] force = true`.
- If you use PipeWire's echo-cancel module, set `[audio] echo_cancellation = "pipewire"` and point `[audio] mic` at its source.

## A Plugin won't work

- **"Landlock isn't available"** in the log: your kernel doesn't have Landlock turned on, and Lucy won't run Plugins without it. Most distributions' kernels have it; check `cat /sys/kernel/security/lsm` lists `landlock`.
- **Lucy says it's broken:** she tells you why at the start of a Session (its Manifest doesn't parse, or it lists different tools from the ones it offers). For a Plugin she wrote, ask her to fix it.
- **She never mentions a Plugin you installed:** check its folder name matches the `name` in its `manifest.toml`, and that it's directly inside `~/.local/share/lucy/plugins/`. A Plugin whose Manifest `requires` something your desktop doesn't have (the Omarchy Plugin outside Omarchy, for example) is quietly left out.
- **It needs a credential:** `lucy credentials <plugin>`.

## The Indicator doesn't appear

The Indicator needs a Wayland compositor with layer-shell (Hyprland, Sway, river and most others; GNOME doesn't have it). Without it, Lucy uses notifications instead. Check it's running with `systemctl --user status lucy-indicator`.

## The bar panel doesn't appear (Omarchy)

Lucy links the panel when you turn on starting at login in `lucy setup`'s **Service** section, but she never changes your bar's layout. Add the `lucy-panel` widget to your bar yourself, then run `omarchy-restart-shell`.

## Still stuck?

Open an issue with what you said, what happened, and the relevant part of `journalctl --user -u lucyd`. Lucy's log never contains your secrets or screenshots, but read it before you share it.
