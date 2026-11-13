#!/usr/bin/env bash
# Every sleep in a test needs a "// sleep-ok:" comment on the line above, so
# wall-clock waits are chosen on purpose (CODING_STANDARDS.md: tests prove
# ordering deterministically).
set -euo pipefail
root=${1:-.}

status=0
while IFS=: read -r file line _; do
    if [ "$line" -le 1 ] || ! sed -n "$((line - 1))p" "$file" | grep -q '// sleep-ok:'; then
        echo "$file:$line: sleep in a test without a \"// sleep-ok:\" comment on the line above." >&2
        echo "  To order events, step the fakes or await the effect instead. A poll loop with a deadline, or a timer test while lucyd has no clock seam, may stay: say which." >&2
        status=1
    fi
done < <(grep -rnE --include='*.rs' '(thread|time)::sleep\(' "$root"/crates/*/tests || true)
exit $status
