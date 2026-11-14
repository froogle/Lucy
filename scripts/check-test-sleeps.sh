#!/usr/bin/env bash
# Every sleep in a test needs a "// sleep-ok:" comment on the line above, so
# wall-clock waits are chosen on purpose (CODING_STANDARDS.md: tests prove
# ordering deterministically).
set -euo pipefail
root=${1:-.}

status=0
while IFS=: read -r file line _; do
    reason=$([ "$line" -gt 1 ] && sed -n "$((line - 1))p" "$file" | grep -o '// sleep-ok:.*' || true)
    if [ -z "$reason" ]; then
        echo "$file:$line: sleep in a test without a \"// sleep-ok:\" comment on the line above." >&2
        echo "  To order events, step the fakes or await the effect instead. A timer test drives Harness::clock with advance. A poll loop with a deadline may stay: say so." >&2
        status=1
    elif grep -qi 'clock seam' <<<"$reason"; then
        echo "$file:$line: lucyd's timers have a clock seam: drive Harness::clock with advance instead of sleeping." >&2
        status=1
    fi
done < <(grep -rnE --include='*.rs' '(thread|time)::sleep\(' "$root"/crates/*/tests || true)
exit $status
