#!/usr/bin/env bash
# Type names must not use terms GLOSSARY.md lists under _Avoid_.
# Terms with a "(when ...)" qualifier are context-dependent and are skipped.
set -euo pipefail
root=${1:-.}

terms=$(sed -n 's/^_Avoid_: //p' "$root/GLOSSARY.md" \
    | tr ',' '\n' \
    | grep -v '(' \
    | sed 's/^ *//; s/ *$//' \
    | grep -E '^[A-Za-z]+$' \
    | sort -u)

status=0
for term in $terms; do
    word="$(tr '[:lower:]' '[:upper:]' <<<"${term:0:1}")${term:1}"
    if found=$(grep -rnE --include='*.rs' "\b(struct|enum|trait|type) [A-Za-z0-9_]*${word}[A-Za-z0-9_]*\b" "$root/crates"); then
        echo "Type name uses \"$term\", which GLOSSARY.md says to avoid:" >&2
        echo "$found" >&2
        status=1
    fi
done
exit $status
