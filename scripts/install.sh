#!/usr/bin/env bash
# Installs Lucy from this checkout and restarts it: builds the five binaries
# (cargo rebuilds only what changed), stops lucyd and the Indicator, copies the
# binaries to ~/.cargo/bin and the shipped Plugins to /usr/share/lucy/plugins
# (sudo, only when they differ), starts both again and reloads the bar.
# The systemd units themselves come from `lucy setup`, not from here.
set -euo pipefail
root="$(git rev-parse --show-toplevel)"
cd "$root"

binaries=(lucyd lucy lucy-indicator lucy-news lucy-omarchy)
bin="$HOME/.cargo/bin"
shipped=/usr/share/lucy/plugins
services=(lucyd lucy-indicator)

echo "==> Building ${binaries[*]}"
packages=()
for b in "${binaries[@]}"; do packages+=(-p "$b"); done
cargo build --release --locked "${packages[@]}"
release="$(cargo metadata --format-version 1 --no-deps | jq -r .target_directory)/release"

for unit in "${services[@]}"; do
    if ! systemctl --user cat "$unit.service" >/dev/null 2>&1; then
        echo "install: $unit.service isn't installed; run 'lucy setup' and turn on starting at login" >&2
        exit 1
    fi
done

echo "==> Stopping ${services[*]}"
systemctl --user stop "${services[@]}"

echo "==> Installing binaries into $bin"
for b in "${binaries[@]}"; do
    install -Dm755 "$release/$b" "$bin/$b"
done

echo "==> Checking shipped Plugins in $shipped"
for plugin in news omarchy; do
    dir="$shipped/$plugin"
    binary="$release/lucy-$plugin"
    manifest="crates/lucy-$plugin/manifest.toml"
    if cmp -s "$binary" "$dir/lucy-$plugin" && cmp -s "$manifest" "$dir/manifest.toml"; then
        echo "    $plugin: up to date"
    else
        echo "    $plugin: changed, installing (needs sudo)"
        sudo install -Dm755 "$binary" "$dir/lucy-$plugin"
        sudo install -Dm644 "$manifest" "$dir/manifest.toml"
    fi
done

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

if [ -L "${XDG_CONFIG_HOME:-$HOME/.config}/omarchy/plugins/lucy-panel" ] && command -v omarchy-restart-shell >/dev/null; then
    echo "==> Reloading the bar"
    omarchy-restart-shell >/dev/null 2>&1 || echo "    couldn't reload the bar; run omarchy-restart-shell" >&2
fi

cat <<EOF

Lucy is installed and running ($(git log -1 --format='%h %s')).

Next:
  - Say "Hey Lucy", or run 'lucy listen', to start a Session.
  - 'lucy' opens the settings menu; 'lucy status' shows her state.
  - Follow the log with: journalctl --user -u lucyd -f
EOF
