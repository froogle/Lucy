#!/usr/bin/env bash
# Runs the panel offscreen in Quickshell against a fake lucy, systemctl and
# omarchy-launch-tui (tests/qml/shell.qml), then checks the commands it ran
# (every one through lucy, none through systemctl) and what its Input dialog
# sent to `lucy input --json`.
# Needs quickshell and Omarchy's shell modules; OMARCHY_SHELL points at them.
# With LUCY_PANEL_CAPTURE set to a .png path, it saves the Details window
# above the Input dialog there, to check by eye.
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
  "status --follow") echo '"'"'{"state":"waiting","wake_word":"Hey Lucy","fast":{"key":"tiers.fast","current":"claude-fast","options":["claude-fast"]},"advanced":{"key":"tiers.advanced","current":"claude-fast","options":["claude-fast"]},"stt":{"key":"stt","current":null,"options":[null]},"mic":{"key":"audio.mic","current":"default","options":["default"]},"speaker":{"key":"audio.speaker","current":"default","options":["default"]}}'"'"'; exec sleep 15;;
  "service start") exit 1;;
  "set nope") echo "lucy: nope is not a setting" >&2; exit 2;;
  "set stt") [ "$3" = superwhisper ] && echo '"'"'{"applies":"next_session","message":"Applies at the next Session."}'"'"' || echo '"'"'{"applies":"restart","message":"Applies after restarting lucyd."}'"'"';;
  "unset stt") echo '"'"'{"applies":"restart","message":"Applies after restarting lucyd."}'"'"';;
  "input --json") exec cat >> "'"$dir"'/replies.log";;
  set*) echo '"'"'{"applies":"next_session","message":"Applies at the next Session."}'"'"';;
esac'
fake systemctl ''
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
    'lucy service start' \
    'lucy service stop' \
    'lucy set tiers.fast codex-fast' \
    'lucy set stt superwhisper' \
    'lucy set stt base.en' \
    'lucy unset stt' \
    'lucy service restart' \
    'lucy set audio.mic alsa_input.usb-Yeti' \
    'omarchy-launch-tui --app-id=TUI.float lucy' \
    'lucy input --json'; do
    if grep -qxF "$call" "$calls"; then
        echo "ran: $call"
    else
        echo "never ran: $call" >&2
        status=1
    fi
done
if grep '^systemctl' "$calls" >&2; then
    echo "the panel ran systemctl itself, not through lucy" >&2
    status=1
fi
# What the Input dialog sent: each reply once, in order; nothing for a
# dialog lucyd closed, nor for a Details window alone the user closed.
cat > "$dir/replies.want" <<'EOF'
{"method":"input.answer","params":{"id":"secret-1","values":{"LINEAR_API_KEY":"sk-test-not-a-real-key"}}}
{"method":"input.cancel","params":{"id":"cancel-1"}}
{"method":"input.cancel","params":{"id":"esc-1"}}
{"method":"input.answer","params":{"id":"kinds-1","values":{"name":"Tibbles","age":"12","colour":"ginger"}}}
{"method":"input.answer","params":{"id":"details-1","values":{"why":"old mail"}}}
EOF
if diff "$dir/replies.want" "$dir/replies.log" >&2; then
    echo "sent: each answer and cancel once"
else
    echo "the Input dialog's replies differ (want, got)" >&2
    status=1
fi
if grep -l 'sk-test-not-a-real-key' "$calls" "$out" >&2; then
    echo "the secret reached a command line or the log" >&2
    status=1
fi
exit $status
