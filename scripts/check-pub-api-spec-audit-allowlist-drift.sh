#!/usr/bin/env bash
# mokumo#654 — drift gate for `.config/pub-api-spec-audit/allowlist.txt`.
#
# For each entry in the allowlist, parse its `tracked: <repo>#<n>`
# annotation and use `gh issue view <repo>#<n> --json state` to assert
# the issue is still OPEN. If the issue has CLOSED, the gate fails —
# the operator should either:
#   * Remove the allowlist entry (the underlying restoration work is
#     already merged), or
#   * Reopen the tracking issue (the work isn't actually done).
#
# Pre-filter: comment lines (starting with `#`) — including format-doc
# blocks at the top of the file — are stripped BEFORE the regex extracts
# tracked refs, so example refs in comments don't false-trigger.
#
# Designed to run from CI only (needs `gh` auth) — local dev shells skip.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${HERE}/.." && pwd)"
cd "$ROOT"

ALLOWLIST="${1:-.config/pub-api-spec-audit/allowlist.txt}"

if [[ ! -f "$ALLOWLIST" ]]; then
    echo "drift-gate: $ALLOWLIST not present, nothing to drift."
    exit 0
fi

if ! command -v gh >/dev/null 2>&1; then
    echo "::warning::drift-gate: gh CLI missing, skipping (dev shell?)" >&2
    exit 0
fi

# Strip comment-only lines (start with `#`) BEFORE pulling tracked refs,
# so the format example in the file header doesn't false-trigger.
mapfile -t REFS < <(
    grep -vE '^[[:space:]]*#' "$ALLOWLIST" 2>/dev/null \
        | grep -oE 'tracked:[[:space:]]*[A-Za-z0-9_/.-]+#[0-9]+' \
        | sed -E 's/tracked:[[:space:]]*//' \
        | sort -u
)

failed=0
for ref in "${REFS[@]}"; do
    [[ -z "$ref" ]] && continue
    repo="${ref%#*}"
    issue="${ref#*#}"
    if [[ "$repo" == "mokumo" ]]; then
        repo="breezy-bays-labs/mokumo"
    fi
    state=$(gh issue view "$issue" --repo "$repo" --json state --jq '.state' 2>/dev/null || echo "unknown")
    if [[ "$state" == "CLOSED" ]]; then
        echo "::error::drift-gate: $ref is CLOSED but still allowlisted in $ALLOWLIST" >&2
        failed=1
    elif [[ "$state" == "unknown" ]]; then
        echo "::warning::drift-gate: could not resolve $ref (gh auth? typo?)" >&2
    fi
done

if (( failed != 0 )); then
    echo "::error::drift-gate: at least one allowlist entry tracks a CLOSED issue. Either drop the entry (coverage restored) or reopen the issue (work not done)." >&2
    exit 1
fi

echo "drift-gate: ok (${#REFS[@]} tracked ref(s) checked)"
