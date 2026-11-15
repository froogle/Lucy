#!/usr/bin/env bash
# A Clippy lint is silenced only with #[expect(clippy::..., reason = "...")],
# so it is silenced on purpose and a stale one fails the build.
set -euo pipefail
root=${1:-.}

status=0
while IFS=: read -r file line text; do
    if [[ $text =~ \#!?\[allow\( ]] || [[ ! $text =~ reason[[:space:]]*= ]]; then
        echo "$file:$line: silence a Clippy lint with #[expect(clippy::..., reason = \"...\")]." >&2
        echo "  Fix the lint instead (for too_many_arguments, bundle the arguments that travel together into a struct), or say why it must stay." >&2
        status=1
    fi
done < <(grep -rnE --include='*.rs' '#!?\[(allow|expect)\([^)]*clippy::' "$root/crates" || true)
exit $status
