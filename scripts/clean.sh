#!/usr/bin/env bash
# Frees what building and testing leave behind. Run it when a ticket is done,
# with no agents running; the next build is cold.
set -euo pipefail
root="$(git rev-parse --show-toplevel)"
cd "$root"

if [ "$(git worktree list | wc -l)" -gt 1 ]; then
    echo "clean: other worktrees exist, so an agent may still be building:" >&2
    git worktree list >&2
    exit 1
fi

target="${CARGO_TARGET_DIR:-$root/target}"
mapfile -t builds < <(find .scratch -type d -name target -prune)
size="$({ du -scb "$target" "${builds[@]}" 2>/dev/null || true; } | tail -n 1 | cut -f 1)"
cargo clean --quiet
rm -rf "${builds[@]}"
git worktree prune
echo "clean: removed $(( size / 1024 / 1024 / 1024 )) GB of build output"
