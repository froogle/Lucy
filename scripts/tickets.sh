#!/usr/bin/env bash
# One line per archived .scratch ticket: file, status, blockers. Live tickets are GitHub issues. Reads both header styles
# (`**Status:** x` in build tickets, `Status: x` in wayfinder tickets).
# Usage: scripts/tickets.sh [issues-dir ...]  (default: every .scratch/*/issues)
set -euo pipefail
cd "$(dirname "$0")/.."

dirs=("$@")
[ ${#dirs[@]} -eq 0 ] && dirs=(.scratch/*/issues)

field() { # field <name> <file>: the first `<name>:` header line's value
    grep -m1 -E "^(\*\*)?$1:(\*\*)?" "$2" | sed -E "s/^(\*\*)?$1:(\*\*)? *//" || true
}

for dir in "${dirs[@]}"; do
    for f in "$dir"/*.md; do
        printf '%-70s %-16s %s\n' "$f" "$(field Status "$f")" "$(field 'Blocked by' "$f")"
    done
done
