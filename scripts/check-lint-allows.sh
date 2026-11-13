#!/usr/bin/env bash
# Every #[allow(clippy::...)] needs a "// reason:" comment on the line above,
# so a lint is silenced on purpose rather than to get a merge through.
set -euo pipefail
root=${1:-.}

status=0
while IFS=: read -r file line _; do
    if [ "$line" -le 1 ] || ! sed -n "$((line - 1))p" "$file" | grep -q '// reason:'; then
        echo "$file:$line: #[allow(clippy::...)] without a \"// reason:\" comment on the line above." >&2
        echo "  Fix the lint instead (for too_many_arguments, bundle the arguments that travel together into a struct), or say why it must stay." >&2
        status=1
    fi
done < <(grep -rnE --include='*.rs' '#!?\[allow\([^)]*clippy::' "$root/crates" || true)
exit $status
