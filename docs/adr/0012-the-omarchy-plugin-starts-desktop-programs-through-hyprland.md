# The Omarchy Plugin starts desktop programs through Hyprland's `exec`

The Omarchy Plugin's `screenshot` and its night light toggle start `omarchy-capture-screenshot` and `omarchy-toggle-nightlight` through Hyprland's `exec` dispatcher (`dispatch hl.dsp.exec_cmd("omarchy-capture-screenshot fullscreen save")`) over the request socket it already uses for windows. They run as a key binding would: outside Lucy's sandbox, with the whole desktop session. The call can't see their exit status, so the tool says it started the screenshot or the night light, not that it finished. The night light's `--status` check still runs inside the sandbox.

Both start desktop programs that a sandboxed child can't host. `omasnap` (the screenshot tool, a Qt app) needs its own directory in `$XDG_RUNTIME_DIR` and a `/dev/shm` buffer. `hyprsunset` starts through `uwsm-app`, which needs the D-Bus session bus, and as a daemon it would keep the sandbox for the rest of the session.

Hyprland's socket already lets this Plugin run any command outside its sandbox, and Landlock in v0.1 doesn't confine connecting to a socket. So for this shipped Plugin the sandbox is a safety rail, not a wall: it keeps the commands the Plugin runs itself from writing where they shouldn't by mistake. What keeps the Plugin honest is that it is shipped and reviewed with Lucy, and its Manifest lists every command it runs, whichever way it runs it. The Manifest's `[access]` names only what the commands run inside the sandbox need.

## Considered Options

- Widening `[access]` for `omasnap` (its runtime dir, `/dev/shm`) and `hyprsunset` (the D-Bus socket): more holes in the sandbox for the same result, and the daemon would still live in Lucy's sandbox and systemd scope.
- Starting them through lucyd's Launcher (`uwsm app --`): it starts apps by name for the desktop Built-in tools, and a Plugin has no way to reach it.
