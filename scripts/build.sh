#!/usr/bin/env bash
# Builds Lucy for release and lays it out as an installable bundle:
#
#   <target>/dist/lucy-<version>-<arch>-linux/
#     install.sh  uninstall.sh  README.md  LICENSE-*
#     bin/        lucyd lucy lucy-indicator
#     plugins/    news/ omarchy/   (each: its binary and manifest.toml)
#     share/      omarchy-plugin/ (the bar panel)  systemd/ (the units)
#
# With --tarball, also packs that folder into lucy-<version>-<arch>-linux.tar.gz
# beside it, for a GitHub release. install.sh installs from either.
# Prints the bundle's path as its last line; everything else goes to stderr.
set -euo pipefail
tarball=false
case "${1:-}" in
    "") ;;
    --tarball) tarball=true ;;
    *) echo "usage: $0 [--tarball]" >&2; exit 2 ;;
esac
root="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$root"

say() { echo "$@" >&2; }

missing=()
for tool in cargo cmake clang pkg-config glslc jq; do
    command -v "$tool" >/dev/null || missing+=("$tool")
done
if [ ${#missing[@]} -gt 0 ]; then
    say "build: missing ${missing[*]}. On Arch or Omarchy, install the build tools with:"
    say "  sudo pacman -S --needed base-devel git rustup cmake clang pkgconf \\"
    say "      vulkan-headers vulkan-icd-loader shaderc alsa-lib pipewire libpulse jq"
    say "  rustup default stable"
    exit 1
fi

programs=(lucyd lucy lucy-indicator)
plugins=(news omarchy)

packages=()
for p in "${programs[@]}"; do packages+=(-p "$p"); done
for p in "${plugins[@]}"; do packages+=(-p "lucy-$p"); done

say "==> Building ${programs[*]} and the ${plugins[*]} Plugins (the first build takes a while)"
cargo build --release --locked "${packages[@]}" >&2

metadata="$(cargo metadata --format-version 1 --no-deps)"
target="$(jq -r .target_directory <<<"$metadata")"
version="$(jq -r '.packages[] | select(.name == "lucyd") | .version' <<<"$metadata")"
release="$target/release"
name="lucy-$version-$(uname -m)-linux"
bundle="$target/dist/$name"

say "==> Laying out $bundle"
rm -rf "$bundle"
for p in "${programs[@]}"; do
    install -Dm755 "$release/$p" "$bundle/bin/$p"
done
for p in "${plugins[@]}"; do
    install -Dm755 "$release/lucy-$p" "$bundle/plugins/$p/lucy-$p"
    install -Dm644 "crates/lucy-$p/manifest.toml" "$bundle/plugins/$p/manifest.toml"
done
mkdir -p "$bundle/share"
cp -r omarchy-plugin "$bundle/share/omarchy-plugin"
rm -rf "$bundle/share/omarchy-plugin/tests"
cp -r dist/systemd "$bundle/share/systemd"
install -m755 scripts/install.sh scripts/uninstall.sh "$bundle/"
install -m644 README.md LICENSE-MIT LICENSE-APACHE "$bundle/"

if $tarball; then
    tar -czf "$target/dist/$name.tar.gz" -C "$target/dist" "$name"
    say "==> Packed $target/dist/$name.tar.gz"
fi
echo "$bundle"
