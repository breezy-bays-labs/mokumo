#!/usr/bin/env bash
# mokumo#655 — handler ↔ scenario map fail-closed gate.
#
# Reads the producer artifact `handler-scenario-coverage.json` and asserts
# that every walked handler row is either:
#   * baselined (frozen at gate-live),
#   * allowlisted with a `tracked: <repo>#<n> — <reason>` annotation, or
#   * covered by at least one **happy** AND one **error_4xx** scenario.
#
# Posture A (per the ADR): 5xx is informational; new handlers do NOT
# need a 5xx scenario. The artifact still tracks 5xx for completeness;
# upgrading the gate to require 5xx is a one-line edit (drop the `--no-5xx`
# default below).
#
# Inputs:
#   --artifact <PATH>        producer JSON (default ./handler-scenario-coverage.json)
#   --baseline <PATH>        committed baseline list (default
#                            .config/handler-scenario-coverage/baseline.txt)
#   --allowlist <PATH>       allowlist file (default
#                            .config/handler-scenario-coverage/allowlist.txt)
#
# Exit codes:
#   0 — all walked handlers covered or exempted; no producer diagnostics.
#   1 — CLI / I/O error.
#   2 — at least one handler missing required column without an
#       allowlist entry.
#   3 — producer diagnostics (unresolvable routes, orphan observations,
#       JSONL parse errors) — printed for action.
#
# Local-dev hint: run after `moon run shop:test-bdd-api && \
#   cargo run -p docs-gen --bin handler-scenario-coverage -- \
#     --workspace-root . --jsonl-dir target/bdd-coverage \
#     --output-json handler-scenario-coverage.json`.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${HERE}/.." && pwd)"
cd "$ROOT"

ARTIFACT="handler-scenario-coverage.json"
BASELINE=".config/handler-scenario-coverage/baseline.txt"
ALLOWLIST=".config/handler-scenario-coverage/allowlist.txt"

while [[ $# -gt 0 ]]; do
    case "$1" in
        --artifact) ARTIFACT="$2"; shift 2;;
        --baseline) BASELINE="$2"; shift 2;;
        --allowlist) ALLOWLIST="$2"; shift 2;;
        -h|--help)
            sed -n '2,/^$/p' "$0" | sed 's/^# \{0,1\}//'
            exit 0
            ;;
        *)
            echo "::error::handler-scenario-coverage: unknown arg $1" >&2
            exit 1
            ;;
    esac
done

if [[ ! -f "$ARTIFACT" ]]; then
    echo "::error::handler-scenario-coverage: artifact not found at $ARTIFACT" >&2
    echo "Run \`cargo run -p docs-gen --bin handler-scenario-coverage -- ...\` first." >&2
    exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "::error::handler-scenario-coverage: jq is required" >&2
    exit 1
fi

# === Read baseline + allowlist ============================================
# Each file is line-oriented:
#   <METHOD> <path>          [# tracked: <repo>#<n> — <reason>]
# Blank lines and full-line comments (`# ...`) are ignored.
# The annotation is required for allowlist entries; baseline entries can
# carry a `# baselined: <git-hash>` comment for provenance.

normalise() {
    # METHOD<space>path — uppercase method, strip trailing whitespace and
    # any inline comment so the comparison is structural, not textual.
    awk '
        # strip CR + leading/trailing space
        { gsub(/\r/, ""); sub(/^[ \t]+/, ""); sub(/[ \t]+$/, "") }
        # skip blank + full-line comments
        /^$/ || /^#/ { next }
        # split entry from inline comment (first unquoted "  #" or " # ")
        {
            entry = $0
            # consume the comment portion by splitting on the first " #"
            n = index(entry, "#")
            if (n > 1) {
                comment_part = substr(entry, n)
                entry = substr(entry, 1, n - 1)
                sub(/[ \t]+$/, "", entry)
            } else {
                comment_part = ""
            }
            # Uppercase the first whitespace-delimited token (the method).
            split(entry, parts, /[ \t]+/)
            if (parts[1] == "") next
            method = toupper(parts[1])
            path = ""
            for (i = 2; i <= length(parts); i++) {
                if (parts[i] == "") continue
                if (path != "") path = path " "
                path = path parts[i]
            }
            if (path == "") next
            print method "\t" path "\t" comment_part
        }
    ' "$1"
}

declare -A BASELINE_SET=() ALLOWLIST_SET=() ALLOWLIST_NOTE=()

if [[ -f "$BASELINE" ]]; then
    while IFS=$'\t' read -r method path note; do
        [[ -z "${method:-}" ]] && continue
        BASELINE_SET["$method $path"]=1
    done < <(normalise "$BASELINE")
fi

ALLOWLIST_INVALID=()
if [[ -f "$ALLOWLIST" ]]; then
    while IFS=$'\t' read -r method path note; do
        [[ -z "${method:-}" ]] && continue
        key="$method $path"
        # Allowlist entries MUST carry `tracked: <repo>#<n>` — otherwise the
        # drift gate has nothing to verify. Reject at gate time.
        if [[ ! "$note" =~ tracked:[[:space:]]*[A-Za-z0-9_/.-]+\#[0-9]+ ]]; then
            ALLOWLIST_INVALID+=("$key — note: '$note'")
            continue
        fi
        ALLOWLIST_SET["$key"]=1
        ALLOWLIST_NOTE["$key"]="$note"
    done < <(normalise "$ALLOWLIST")
fi

# === Walk the artifact ====================================================
# For each handler in the artifact, check its required columns (happy +
# error_4xx). Skip rows that are baselined or allowlisted; remember
# allowlist-baseline overlap (warns the operator about dead entries).

mapfile -t HANDLER_ROWS < <(jq -r '
    .by_crate[]?.handlers[]?
    | [.method, .path, (.happy|length), (.error_4xx|length), (.error_5xx|length)]
    | @tsv
' "$ARTIFACT")

MISSING_HAPPY=()
MISSING_4XX=()
ALLOWLIST_DEAD=()

for row in "${HANDLER_ROWS[@]}"; do
    [[ -z "$row" ]] && continue
    IFS=$'\t' read -r method path happy_n err4_n _err5_n <<<"$row"
    key="$method $path"
    if [[ -n "${BASELINE_SET[$key]:-}" ]]; then
        # If a baselined entry now has full coverage AND is also on the
        # allowlist, surface the overlap — operator should reconcile.
        if [[ -n "${ALLOWLIST_SET[$key]:-}" ]] && (( happy_n > 0 && err4_n > 0 )); then
            ALLOWLIST_DEAD+=("$key (baselined + allowlisted but now covered)")
        fi
        continue
    fi
    if [[ -n "${ALLOWLIST_SET[$key]:-}" ]]; then
        if (( happy_n > 0 && err4_n > 0 )); then
            ALLOWLIST_DEAD+=("$key — covered, drop from allowlist")
        fi
        continue
    fi
    if (( happy_n == 0 )); then
        MISSING_HAPPY+=("$key")
    fi
    if (( err4_n == 0 )); then
        MISSING_4XX+=("$key")
    fi
done

# === Producer diagnostics =================================================
DIAG_UNRESOLVABLE=$(jq '.diagnostics.unresolvable_routes | length' "$ARTIFACT")
DIAG_ORPHANS=$(jq '.diagnostics.orphan_observations | length' "$ARTIFACT")
DIAG_PARSE_ERRORS=$(jq '.diagnostics.jsonl_errors | length' "$ARTIFACT")
ROWS_CONSUMED=$(jq '.diagnostics.rows_consumed' "$ARTIFACT")

# === Decide ===============================================================
fail=0

if (( ROWS_CONSUMED == 0 )); then
    echo "::warning::handler-scenario-coverage: 0 rows consumed — capture middleware may not have been wired in. Check that \`moon run shop:test-bdd-api\` ran AND wrote to the JSONL dir before the producer." >&2
fi

if (( ${#ALLOWLIST_INVALID[@]} > 0 )); then
    echo "::error::handler-scenario-coverage: allowlist entries missing 'tracked: <repo>#<n> — <reason>' annotation:" >&2
    printf '  - %s\n' "${ALLOWLIST_INVALID[@]}" >&2
    fail=1
fi

if (( ${#MISSING_HAPPY[@]} > 0 || ${#MISSING_4XX[@]} > 0 )); then
    echo "::error::handler-scenario-coverage: new handlers missing required column(s)." >&2
    if (( ${#MISSING_HAPPY[@]} > 0 )); then
        echo "  Missing happy (2xx) coverage:" >&2
        printf '    - %s\n' "${MISSING_HAPPY[@]}" >&2
    fi
    if (( ${#MISSING_4XX[@]} > 0 )); then
        echo "  Missing error_4xx coverage:" >&2
        printf '    - %s\n' "${MISSING_4XX[@]}" >&2
    fi
    cat >&2 <<EOF

Remediation options:
  1. Add a BDD scenario in crates/mokumo-shop/tests/api_features/ that
     hits the handler with the missing status class, then re-run
     \`moon run shop:test-bdd-api\` and the producer.
  2. If the gap is intentional (handler under construction, can't be
     reached from BDD without infra Mokumo doesn't have yet), add an
     entry to ${ALLOWLIST} of the form
       <METHOD> <path>  # tracked: <repo>#<n> — <reason>
     where <repo>#<n> is an open issue tracking restoration.

Posture: gate requires happy + 4xx for new handlers (5xx is
informational). Existing 0%-covered handlers are frozen in
${BASELINE} until the work to cover them is filed and scheduled.
EOF
    fail=2
fi

if (( DIAG_UNRESOLVABLE > 0 || DIAG_ORPHANS > 0 || DIAG_PARSE_ERRORS > 0 )); then
    echo "::warning::handler-scenario-coverage: producer diagnostics non-empty (\
unresolvable=$DIAG_UNRESOLVABLE, orphans=$DIAG_ORPHANS, parse_errors=$DIAG_PARSE_ERRORS). \
See $ARTIFACT for details." >&2
    # Orphan-only diagnostics are tolerated during the mokumo#816 soft-window:
    # the walker emits relative paths for nested routers, so MatchedPath
    # absolute paths from capture register as orphans even when the handler
    # IS walked. Unresolvable routes and JSONL parse errors are real producer
    # failures and still escalate. Restore strict behavior (drop the special
    # case) once #816 lands and orphans naturally drop to 0.
    if (( DIAG_UNRESOLVABLE > 0 || DIAG_PARSE_ERRORS > 0 )); then
        if (( fail == 0 )); then
            fail=3
        fi
    fi
fi

if (( ${#ALLOWLIST_DEAD[@]} > 0 )); then
    echo "::warning::handler-scenario-coverage: allowlist entries with restored coverage — drop them from $ALLOWLIST:" >&2
    printf '  - %s\n' "${ALLOWLIST_DEAD[@]}" >&2
fi

if (( fail == 0 )); then
    walked=$(jq '[.by_crate[]?.handlers[]?] | length' "$ARTIFACT")
    baselined=${#BASELINE_SET[@]}
    allowlisted=${#ALLOWLIST_SET[@]}
    echo "handler-scenario-coverage: ok ($walked handler(s), $baselined baselined, $allowlisted allowlisted, $ROWS_CONSUMED row(s))"
fi

exit "$fail"
