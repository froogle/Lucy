#!/usr/bin/env bash
# Runs the panel offscreen in Quickshell against a fake lucy, systemctl and
# omarchy-launch-tui (tests/qml/shell.qml), then checks the commands it ran.
# Needs quickshell and Omarchy's shell modules; OMARCHY_SHELL points at them.
set -euo pipefail
plugin="$(cd "$(dirname "$0")/.." && pwd)"
shell="${OMARCHY_SHELL:-/usr/share/omarchy/shell}"
command -v quickshell >/dev/null || { echo "quickshell is not installed" >&2; exit 1; }
[ -d "$shell" ] || { echo "Omarchy's shell modules aren't at $shell; set OMARCHY_SHELL" >&2; exit 1; }

dir="$(mktemp -d -t lucy-panel.XXXXXX)"
trap 'kill "${pid:-}" 2>/dev/null || true; rm -rf "$dir"' EXIT
cp "$plugin/tests/qml/shell.qml" "$plugin/Service.qml" "$dir/"
cp -r "$plugin/i18n" "$dir/"
# Under another name: a Panel.qml beside it would shadow qs.Ui's Panel.
cp "$plugin/Panel.qml" "$dir/BarPanel.qml"
ln -s "$shell/Commons" "$dir/Commons"
# The real KeyboardPanel is a layer shell window, which has no offscreen
# backend.
cp -rP "$shell/Ui" "$dir/Ui"
cp "$plugin/tests/qml/KeyboardPanel.qml" "$dir/Ui/KeyboardPanel.qml"
mkdir -m 700 "$dir/runtime" "$dir/bin"

calls="$dir/calls.log"
fake() {
    printf '#!/bin/sh\necho "%s $*" >> "%s"\n%s\n' "$1" "$calls" "$2" > "$dir/bin/$1"
    chmod +x "$dir/bin/$1"
}
fake lucy 'case "$1 $2" in
  "status --follow") echo '"'"'{"state":"waiting","fast":{"key":"tiers.fast","current":"claude-fast","options":["claude-fast"]},"advanced":{"key":"tiers.advanced","current":"claude-fast","options":["claude-fast"]},"whisper_model":null,"mic":{"key":"audio.mic","current":"default","options":["default"]},"speaker":{"key":"audio.speaker","current":"default","options":["default"]}}'"'"'; exec sleep 15;;
  "set nope") echo "lucy: nope is not a setting" >&2; exit 2;;
  "set whisper.model"|"unset whisper.model") echo '"'"'{"applies":"restart","message":"Applies after restarting lucyd."}'"'"';;
  set*) echo '"'"'{"applies":"next_session","message":"Applies at the next Session."}'"'"';;
esac'
fake systemctl '[ "$2" != start ]'
fake omarchy-launch-tui ''

out="$dir/out"
env -u WAYLAND_DISPLAY -u DISPLAY -u QT_QPA_PLATFORMTHEME \
    QT_QPA_PLATFORM=offscreen PATH="$dir/bin:/usr/bin" \
    XDG_RUNTIME_DIR="$dir/runtime" XDG_CACHE_HOME="$dir/cache" XDG_STATE_HOME="$dir/state" \
    quickshell -p "$dir" > "$out" 2>&1 &
pid=$!
# Quickshell colours its log; read it without.
plain() { sed 's/\x1b\[[0-9;]*m//g' "$out"; }
for _ in $(seq 150); do
    plain | grep -qE 'qml: +(ALL OK|[0-9]+ FAILED)' && break
    sleep 0.2
done
kill "$pid" 2>/dev/null || true
wait "$pid" 2>/dev/null || true

status=0
plain | grep -E 'qml: +(ok|FAIL)' | sed 's/.*qml: *//' || true
if ! plain | grep -q 'qml: *ALL OK'; then
    echo "the harness didn't pass:" >&2
    plain | grep -vE 'qml: +ok' | tail -40 >&2
    status=1
fi
for call in \
    'lucy status --follow --json' \
    'lucy end' \
    'systemctl --user start lucyd.service' \
    'systemctl --user stop lucyd.service' \
    'lucy set tiers.fast codex-fast' \
    'lucy set whisper.model base.en' \
    'lucy unset whisper.model' \
    'systemctl --user restart lucyd.service' \
    'lucy set audio.mic alsa_input.usb-Yeti' \
    'omarchy-launch-tui --app-id=TUI.float lucy'; do
    if grep -qxF "$call" "$calls"; then
        echo "ran: $call"
    else
        echo "never ran: $call" >&2
        status=1
    fi
done
exit $status
