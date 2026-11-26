#!/usr/bin/env bash
# Frees what building and testing leave behind. Run it when a ticket is done,
# with no agents running; the next build is cold.
set -euo pipefail
root="$(git rev-parse --show-toplevel)"
cd "$root"

target="${CARGO_TARGET_DIR:-$root/target}"

# Another worktree builds into this target/ if it is that worktree's own, or
# if it is the main checkout's and the worktree has none of its own (it
# builds there through CARGO_TARGET_DIR). An agent there may still be building.
here="$(realpath -m "$target")"
main=""
sharing=()
while read -r key path; do
    [ "$key" = worktree ] || continue
    main="${main:-$path}"
    [ "$path" != "$root" ] || continue
    if [ "$(realpath -m "$path/target")" = "$here" ] ||
        { [ ! -d "$path/target" ] && [ "$(realpath -m "$main/target")" = "$here" ]; }; then
        sharing+=("$path")
    fi
done < <(git worktree list --porcelain)
if [ "${#sharing[@]}" -gt 0 ]; then
    echo "clean: these worktrees build into $target, so an agent may still be building:" >&2
    printf '  %s\n' "${sharing[@]}" >&2
    exit 1
fi

builds=()
[ -d .scratch ] && mapfile -t builds < <(find .scratch -type d -name target -prune)
plugin_builds="${XDG_DATA_HOME:-$HOME/.local/share}/lucy/plugins/.build"
[ -d "$plugin_builds" ] &&
    mapfile -t -O "${#builds[@]}" builds < <(find "$plugin_builds" -mindepth 2 -maxdepth 2 -type d -name target)
size="$({ du -scb "$target" "${builds[@]}" 2>/dev/null || true; } | tail -n 1 | cut -f 1)"
cargo clean --quiet
rm -rf "${builds[@]}"
git worktree prune
echo "clean: removed $(( size / 1024 / 1024 / 1024 )) GB of build output"
