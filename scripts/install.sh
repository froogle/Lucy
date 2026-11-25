#!/usr/bin/env bash
# Installs Lucy, first time or as an update, from an unpacked release or from a
# checkout (which it builds first with scripts/build.sh):
#
#   - the programs into $LUCY_PREFIX/bin (default ~/.local/bin)
#   - the bar panel into $LUCY_PREFIX/share/lucy/omarchy-plugin
#   - the systemd user units, pointing at those programs (not enabled here)
#   - the shipped Plugins into /usr/share/lucy/plugins (sudo, only when changed)
#   - the Whisper speech model(s), when missing
#
# Then it restarts Lucy if she was set to run, or opens `lucy setup` on a first
# install, whose last section turns on starting at login.
set -euo pipefail
here="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"

if [ -x "$here/bin/lucyd" ]; then
    bundle="$here"
elif [ -x "$here/build.sh" ] && [ -f "$here/../Cargo.toml" ]; then
    bundle="$("$here/build.sh")"
else
    echo "install: no bin/lucyd beside this script, and it isn't in a Lucy checkout" >&2
    exit 1
fi

prefix="${LUCY_PREFIX:-$HOME/.local}"
bin="$prefix/bin"
panel="$prefix/share/lucy/omarchy-plugin"
config="${XDG_CONFIG_HOME:-$HOME/.config}"
units="$config/systemd/user"
models="${XDG_DATA_HOME:-$HOME/.local/share}/lucy/models"
link="$config/omarchy/plugins/lucy-panel"
shipped=/usr/share/lucy/plugins
programs=(lucyd lucy lucy-indicator)
plugins=(news omarchy)
services=(lucyd lucy-indicator)

missing="$(ldd "$bundle"/bin/* "$bundle"/plugins/*/lucy-* 2>/dev/null | awk '/not found/ {print $1}' | sort -u | paste -sd " ")"
if [ -n "$missing" ]; then
    echo "install: these libraries are missing: $missing" >&2
    echo "On Arch or Omarchy, install them with:" >&2
    echo "  sudo pacman -S --needed pipewire alsa-lib libpulse vulkan-icd-loader" >&2
    exit 1
fi

previous=""
if [ -f "$units/lucyd.service" ]; then
    previous="$(dirname "$(sed -n 's/^ExecStart=//p' "$units/lucyd.service" | head -1)")"
fi
wanted=false
if systemctl --user is-enabled --quiet lucyd.service 2>/dev/null ||
    systemctl --user is-active --quiet lucyd.service 2>/dev/null; then
    wanted=true
    echo "==> Stopping ${services[*]}"
    systemctl --user stop "${services[@]}"
fi

echo "==> Installing ${programs[*]} into $bin"
for p in "${programs[@]}"; do
    install -Dm755 "$bundle/bin/$p" "$bin/$p"
done
if [ -n "$previous" ] && [ "$previous" != "$bin" ] && [ -x "$previous/lucyd" ]; then
    echo "    removing the earlier install from $previous, so it can't shadow this one"
    for p in "${programs[@]}" lucy-news lucy-omarchy; do
        rm -f "$previous/$p"
    done
fi

echo "==> Installing the bar panel into $panel"
rm -rf "$panel"
mkdir -p "$(dirname "$panel")"
cp -r "$bundle/share/omarchy-plugin" "$panel"
if [ -L "$link" ]; then
    ln -sfn "$panel" "$link"
fi

echo "==> Installing the systemd units into $units"
mkdir -p "$units"
for unit in "$bundle"/share/systemd/*.service; do
    sed "s|^ExecStart=.*/|ExecStart=$bin/|" "$unit" >"$units/$(basename "$unit")"
done
systemctl --user daemon-reload

echo "==> Checking the shipped Plugins in $shipped"
for p in "${plugins[@]}"; do
    from="$bundle/plugins/$p"
    to="$shipped/$p"
    if cmp -s "$from/lucy-$p" "$to/lucy-$p" && cmp -s "$from/manifest.toml" "$to/manifest.toml"; then
        echo "    $p: up to date"
    else
        echo "    $p: changed, installing (needs sudo)"
        sudo install -Dm755 "$from/lucy-$p" "$to/lucy-$p"
        sudo install -Dm644 "$from/manifest.toml" "$to/manifest.toml"
    fi
done

fetch_model() {
    local file="ggml-$1.bin"
    [ -f "$models/$file" ] && return
    echo "    downloading $file"
    mkdir -p "$models"
    curl -fL --progress-bar -o "$models/$file.part" \
        "https://huggingface.co/ggerganov/whisper.cpp/resolve/main/$file"
    mv "$models/$file.part" "$models/$file"
}
echo "==> Checking the speech models in $models"
fetch_model base.en
if compgen -G "/dev/dri/renderD*" >/dev/null; then
    fetch_model small.en
fi

warnings=()
case ":$PATH:" in
    *":$bin:"*) ;;
    *) warnings+=("$bin isn't on your PATH; add it so 'lucy' runs from anywhere.") ;;
esac
if ! command -v claude >/dev/null && ! command -v codex >/dev/null; then
    warnings+=("Neither 'claude' nor 'codex' is installed. Lucy thinks through one of them: install it and log in with your subscription.")
fi

if $wanted; then
    echo "==> Starting ${services[*]}"
    systemctl --user start "${services[@]}"
    for _ in $(seq 50); do
        "$bin/lucy" status >/dev/null 2>&1 && break
        sleep 0.2
    done
    if ! "$bin/lucy" status; then
        echo "install: lucyd didn't come up; see: journalctl --user -u lucyd -e" >&2
        exit 1
    fi
    if [ -L "$link" ] && command -v omarchy-restart-shell >/dev/null; then
        echo "==> Reloading the bar"
        omarchy-restart-shell >/dev/null 2>&1 || echo "    couldn't reload the bar; run omarchy-restart-shell" >&2
    fi
fi

for w in "${warnings[@]}"; do echo "warning: $w" >&2; done

if $wanted; then
    echo
    echo "Lucy is updated and running. Say \"Hey Lucy\", or run 'lucy listen'."
elif [ ! -f "$config/lucy/config.toml" ] && [ -t 0 ] && [ -t 1 ]; then
    echo
    echo "Lucy is installed. Opening setup: press Enter to take each suggestion."
    exec "$bin/lucy" setup
else
    echo
    echo "Lucy is installed. Run 'lucy setup', and in its last section turn on starting at login."
fi
