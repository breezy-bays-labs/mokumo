#!/usr/bin/env bash
# Guard: every touched ADR that opts into YAML frontmatter must declare an
# `enforced-by:` contract.
#
# Scope: `docs/adr/**.md`. The gate is dormant on legacy files that have no
# YAML frontmatter (no opening `---` line) — adoption of the contract is
# voluntary at the file level. Any file that DOES open with `---` must
# contain `enforced-by:` inside that frontmatter block, or the gate fails.
#
# This is a syntactic check. Full reference resolution (test names exist,
# workflows exist, etc.) lives in `tools/docs-gen/src/bin/adr-validate.rs`
# and runs from lefthook + local dev. Keeping CI syntactic-only avoids
# pulling the ops vault (where the canonical 76 ADRs live) into CI runners.
#
# Diff base: BASE_REF (default origin/main). CI sets STRICT_BASE_REF=1.
#
# Self-test injection: NAME_OVERRIDE points at a file containing a
# pre-recorded list of paths (one per line) to use in place of git.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${HERE}/.." && pwd)"
cd "$ROOT"

BASE_REF="${BASE_REF:-origin/main}"
STRICT_BASE_REF="${STRICT_BASE_REF:-0}"
NAME_OVERRIDE="${NAME_OVERRIDE:-}"

# Acquire the list of touched ADR files.
if [ -n "$NAME_OVERRIDE" ]; then
    touched=$(cat "$NAME_OVERRIDE")
elif git rev-parse --verify "$BASE_REF" >/dev/null 2>&1; then
    touched=$(git diff --name-only --diff-filter=AM "$BASE_REF...HEAD" -- 'docs/adr/*.md' 'docs/adr/**/*.md' 2>/dev/null || true)
else
    if [ "$STRICT_BASE_REF" = "1" ]; then
        echo "::error::adr-frontmatter: BASE_REF '$BASE_REF' not found and STRICT_BASE_REF=1" >&2
        exit 1
    fi
    echo "adr-frontmatter: BASE_REF '$BASE_REF' not found; warn-skipping (CI is the source of truth)" >&2
    exit 0
fi

if [ -z "$touched" ]; then
    echo "adr-frontmatter ok: no ADR changes in this diff"
    exit 0
fi

failures=()
# while-read over a here-string keeps the loop safe for paths containing
# spaces, and IFS reset prevents word-splitting on tabs.
#
# Delimiter handling: the Rust parser in `tools/docs-gen/src/adr.rs` uses
# `trim_end()` on each line before comparing to `---`. We mirror that
# tolerance here — strip CR (CRLF endings) and trailing whitespace before
# the check — so a CRLF-encoded ADR can't be parsed by Rust while being
# silently classified as legacy by this gate.
while IFS= read -r path; do
    [ -z "$path" ] && continue
    [ -f "$path" ] || continue
    first_line=$(head -n 1 < "$path" | sed -e 's/\r$//' -e 's/[[:space:]]*$//' || true)
    if [ "$first_line" != "---" ]; then
        # Legacy format — no YAML frontmatter, gate is dormant for this file.
        continue
    fi
    # Extract the frontmatter block (everything between the first `---` and
    # the next `---` line). awk emits the block; grep checks for the key.
    # `grep -E '^enforced-by:[[:space:]]*$'` is exact-line shape: matches
    # only the block-sequence opener, never a substring like
    # `enforced-by-legacy:` or an inline-value variant.
    # `sed 's/\r$//'` upstream of awk normalizes CRLF; awk's delimiter
    # match accepts trailing whitespace to mirror the Rust parser.
    block=$(
        sed 's/\r$//' < "$path" |
        awk 'NR==1 && /^---[[:space:]]*$/ { in_fm=1; next } in_fm && /^---[[:space:]]*$/ { exit } in_fm { print }'
    )
    if ! printf '%s\n' "$block" | grep -Eq '^enforced-by:[[:space:]]*$'; then
        failures+=("$path")
    fi
done <<< "$touched"

if [ ${#failures[@]} -eq 0 ]; then
    echo "adr-frontmatter ok: all touched ADRs with YAML frontmatter declare enforced-by"
    exit 0
fi

echo "::error::adr-frontmatter: the following touched ADRs use YAML frontmatter but lack \`enforced-by:\`:" >&2
for f in "${failures[@]}"; do
    echo "  - $f" >&2
done
echo "" >&2
echo "Add an enforced-by: list-of-objects entry. See AGENTS.md §Synchronized-Docs and docs/adr-index.md." >&2
exit 1
