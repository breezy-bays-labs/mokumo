#!/usr/bin/env bash
# mokumo#654 — public-API spec audit fail-closed gate.
#
# Reads the producer artifact `pub-api-spec-audit.json` and asserts that
# every walked pub item is either:
#   * baselined in `.config/pub-api-spec-audit/<crate>.txt` (frozen at
#     gate-live), or
#   * allowlisted in `.config/pub-api-spec-audit/allowlist.txt` with a
#     `tracked: <repo>#<n> — <reason>` annotation, or
#   * BDD-covered (≥ 1 source line in its span has lcov hit ≥ 1 from a
#     cucumber-driven test binary).
#
# Why per-crate baseline files: 1029+ pub items across 13 crates would
# make a single baseline file unreviewable; per-crate keeps diffs scoped.
#
# Inputs:
#   --artifact <PATH>    producer JSON (default ./pub-api-spec-audit.json)
#   --baseline-dir <DIR> per-crate baseline dir
#                        (default .config/pub-api-spec-audit/)
#   --allowlist <PATH>   workspace-wide allowlist file
#                        (default .config/pub-api-spec-audit/allowlist.txt)
#
# Exit codes:
#   0 — all walked pub items covered or exempted; no producer diagnostics.
#   1 — CLI / I/O error.
#   2 — at least one new pub item missing BDD coverage without an exemption.
#   3 — producer diagnostics non-empty (parse errors, lcov errors).

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${HERE}/.." && pwd)"
cd "$ROOT"

ARTIFACT="pub-api-spec-audit.json"
BASELINE_DIR=".config/pub-api-spec-audit"
ALLOWLIST=".config/pub-api-spec-audit/allowlist.txt"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --artifact) ARTIFACT="$2"; shift 2;;
        --baseline-dir) BASELINE_DIR="$2"; shift 2;;
        --allowlist) ALLOWLIST="$2"; shift 2;;
        -h|--help)
            sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "::error::pub-api-spec-audit: unknown arg $1" >&2
            exit 1
            ;;
    esac
done

if [[ ! -f "$ARTIFACT" ]]; then
    echo "::error::pub-api-spec-audit: artifact not found at $ARTIFACT" >&2
    echo "Run \`cargo run -p docs-gen --bin pub-api-spec-audit -- ...\` first." >&2
    exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "::error::pub-api-spec-audit: jq is required" >&2
    exit 1
fi

# === Read allowlist (workspace-wide, line-oriented) ======================
# Format: <crate>::<item_path>  # tracked: <repo>#<n> — <reason>

normalise_allowlist() {
    awk '
        { gsub(/\r/, ""); sub(/^[ \t]+/, ""); sub(/[ \t]+$/, "") }
        /^$/ || /^#/ { next }
        {
            entry = $0
            n = index(entry, "#")
            if (n > 1) {
                comment_part = substr(entry, n)
                entry = substr(entry, 1, n - 1)
                sub(/[ \t]+$/, "", entry)
            } else {
                comment_part = ""
            }
            if (entry == "") next
            print entry "\t" comment_part
        }
    ' "$1"
}

declare -A ALLOWLIST_SET=() ALLOWLIST_NOTE=()
ALLOWLIST_INVALID=()

if [[ -f "$ALLOWLIST" ]]; then
    while IFS=$'\t' read -r item_path note; do
        [[ -z "${item_path:-}" ]] && continue
        if [[ ! "$note" =~ tracked:[[:space:]]*[A-Za-z0-9_/.-]+\#[0-9]+ ]]; then
            ALLOWLIST_INVALID+=("$item_path — note: '$note'")
            continue
        fi
        ALLOWLIST_SET["$item_path"]=1
        ALLOWLIST_NOTE["$item_path"]="$note"
    done < <(normalise_allowlist "$ALLOWLIST")
fi

# === Walk artifact, check coverage per item =============================

mapfile -t ROWS < <(jq -r '
    .by_crate[]?
    | .crate_name as $c
    | .items[]?
    | [$c, .item_path, .bdd_covered_lines] | @tsv
' "$ARTIFACT")

MISSING_COVERAGE=()
ALLOWLIST_DEAD=()

# Per-crate baseline lookup: read each baseline file once into the
# corresponding associative array.
declare -A BASELINE_LOADED=()

baseline_contains() {
    local crate="$1" item="$2" path
    path="$BASELINE_DIR/${crate}.txt"
    if [[ -z "${BASELINE_LOADED[$crate]:-}" ]]; then
        BASELINE_LOADED["$crate"]=1
        if [[ -f "$path" ]]; then
            while IFS= read -r line; do
                line="${line%$'\r'}"
                line="${line#"${line%%[![:space:]]*}"}"
                line="${line%"${line##*[![:space:]]}"}"
                [[ -z "$line" || "$line" == \#* ]] && continue
                line="${line%%#*}"
                line="${line%"${line##*[![:space:]]}"}"
                [[ -z "$line" ]] && continue
                eval "_BL_${crate//[^a-zA-Z0-9_]/_}_HAS[\"\$line\"]=1"
            done < "$path"
        fi
    fi
    local var
    var="_BL_${crate//[^a-zA-Z0-9_]/_}_HAS[\"\$item\"]"
    eval "[[ -n \"\${${var}:-}\" ]]"
}

# Each crate gets its own associative array of baselined items, declared
# lazily on first access. Bash's lack of namespaced hashes leads to the
# `eval` dance — the keys come from a JSON producer we trust.
for c in $(jq -r '.by_crate[]?.crate_name' "$ARTIFACT"); do
    var_name="_BL_${c//[^a-zA-Z0-9_]/_}_HAS"
    declare -A "$var_name=()"
done

for row in "${ROWS[@]}"; do
    [[ -z "$row" ]] && continue
    IFS=$'\t' read -r crate item covered <<<"$row"
    if (( covered > 0 )); then
        # Covered — but if it's also allowlisted, surface dead entry.
        if [[ -n "${ALLOWLIST_SET[$item]:-}" ]]; then
            ALLOWLIST_DEAD+=("$item — covered, drop from allowlist")
        fi
        continue
    fi
    if baseline_contains "$crate" "$item"; then
        if [[ -n "${ALLOWLIST_SET[$item]:-}" ]]; then
            ALLOWLIST_DEAD+=("$item (baselined + allowlisted)")
        fi
        continue
    fi
    if [[ -n "${ALLOWLIST_SET[$item]:-}" ]]; then
        continue
    fi
    MISSING_COVERAGE+=("$item")
done

# === Producer diagnostics ===============================================
DIAG_PARSE=$(jq '.diagnostics.parse_errors | length' "$ARTIFACT")
DIAG_LCOV=$(jq '.diagnostics.lcov_errors | length' "$ARTIFACT")
ITEMS_WALKED=$(jq '.diagnostics.items_walked' "$ARTIFACT")
LCOV_FILES=$(jq '.diagnostics.lcov_files_consumed' "$ARTIFACT")

# === Decide ==============================================================
fail=0

if (( ITEMS_WALKED == 0 )); then
    echo "::warning::pub-api-spec-audit: 0 pub items walked — workspace layout may have changed." >&2
fi

if (( LCOV_FILES == 0 )); then
    echo "::warning::pub-api-spec-audit: 0 lcov files consumed — BDD coverage capture may not have run. The gate will treat ALL items as uncovered." >&2
fi

if (( ${#ALLOWLIST_INVALID[@]} > 0 )); then
    echo "::error::pub-api-spec-audit: allowlist entries missing 'tracked: <repo>#<n> — <reason>' annotation:" >&2
    printf '  - %s\n' "${ALLOWLIST_INVALID[@]}" >&2
    fail=1
fi

if (( ${#MISSING_COVERAGE[@]} > 0 )); then
    echo "::error::pub-api-spec-audit: ${#MISSING_COVERAGE[@]} new pub item(s) without BDD coverage." >&2
    printf '  - %s\n' "${MISSING_COVERAGE[@]}" | head -50 >&2
    if (( ${#MISSING_COVERAGE[@]} > 50 )); then
        echo "  ... and $(( ${#MISSING_COVERAGE[@]} - 50 )) more." >&2
    fi
    cat >&2 <<EOF

Remediation options:
  1. Add a BDD scenario that exercises the new pub item, then re-run:
       moon run shop:test-bdd-api  &&  cargo run -p docs-gen --bin pub-api-spec-audit -- ...
  2. If the gap is intentional (item under construction, can't be reached
     from BDD without infra Mokumo doesn't have yet), add an entry to
     ${ALLOWLIST} of the form
       <crate>::<item_path>  # tracked: <repo>#<n> — <reason>
     where <repo>#<n> is an open issue tracking restoration.

Posture: existing 0%-covered items are frozen per-crate in
${BASELINE_DIR}/<crate>.txt until the work to cover them is filed
and scheduled.
EOF
    fail=2
fi

if (( DIAG_PARSE > 0 || DIAG_LCOV > 0 )); then
    echo "::warning::pub-api-spec-audit: producer diagnostics non-empty (\
parse_errors=$DIAG_PARSE, lcov_errors=$DIAG_LCOV). See $ARTIFACT for details." >&2
    if (( fail == 0 )); then
        fail=3
    fi
fi

if (( ${#ALLOWLIST_DEAD[@]} > 0 )); then
    echo "::warning::pub-api-spec-audit: allowlist entries with restored coverage — drop them from $ALLOWLIST:" >&2
    printf '  - %s\n' "${ALLOWLIST_DEAD[@]}" >&2
fi

if (( fail == 0 )); then
    echo "pub-api-spec-audit: ok ($ITEMS_WALKED pub item(s), $LCOV_FILES lcov file(s) consumed)"
fi

exit "$fail"
