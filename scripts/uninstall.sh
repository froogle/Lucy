#!/usr/bin/env bash
# Removes what install.sh put on the machine: the services and their units, the
# programs, the bar panel and its link, and the shipped Plugins (sudo).
# Lucy's settings, Persona, models and your own Plugins stay; it says where.
set -euo pipefail
config="${XDG_CONFIG_HOME:-$HOME/.config}"
units="$config/systemd/user"
link="$config/omarchy/plugins/lucy-panel"

bin="${LUCY_PREFIX:-$HOME/.local}/bin"
if [ -f "$units/lucyd.service" ]; then
    bin="$(dirname "$(sed -n 's/^ExecStart=//p' "$units/lucyd.service" | head -1)")"
fi

echo "==> Stopping and removing the services"
systemctl --user disable --now lucyd.service lucy-indicator.service 2>/dev/null || true
rm -f "$units/lucyd.service" "$units/lucy-indicator.service"
systemctl --user daemon-reload

echo "==> Removing the programs from $bin"
rm -f "$bin"/{lucyd,lucy,lucy-indicator,lucy-news,lucy-omarchy}

echo "==> Removing the bar panel"
rm -rf "$(dirname "$bin")/share/lucy/omarchy-plugin"
if [ -L "$link" ]; then
    rm "$link"
fi

if [ -d /usr/share/lucy ]; then
    echo "==> Removing the shipped Plugins (needs sudo)"
    sudo rm -r /usr/share/lucy
fi

cat <<EOF

Lucy is uninstalled. Her settings, Persona, models and Plugins are still in:
  $config/lucy
  ${XDG_DATA_HOME:-$HOME/.local/share}/lucy
  ${XDG_STATE_HOME:-$HOME/.local/state}/lucy
Delete those to remove everything. Credentials stay in your keyring under Lucy's name.
EOF
