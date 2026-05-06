#!/usr/bin/env bash
# mokumo#654 — regenerate `.config/pub-api-spec-audit/<crate>.txt` from
# the most recent producer artifact.
#
# Reads `pub-api-spec-audit.json` (default in workspace root), emits one
# baseline file per crate listing the pub items that lacked BDD coverage
# AND are not already in the allowlist. Use this when:
#   * Initially seeding baselines at gate-live (one-time).
#   * Backfilling after deliberately retiring an allowlist entry that
#     turned out to need a longer-term gap (rare).
#
# Per-crate files: 1029+ items would make a single workspace baseline
# unreviewable. One file per crate keeps diffs scoped to the changed
# crate. Empty crates (no uncovered items) get an empty file (or are
# left absent — both are accepted by the gate).

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${HERE}/.." && pwd)"
cd "$ROOT"

ARTIFACT="${ARTIFACT:-pub-api-spec-audit.json}"
BASELINE_DIR="${BASELINE_DIR:-.config/pub-api-spec-audit}"

if [[ ! -f "$ARTIFACT" ]]; then
    echo "::error::regen-baseline: $ARTIFACT not found." >&2
    echo "Run \`cargo run -p docs-gen --bin pub-api-spec-audit -- ...\` first." >&2
    exit 1
fi

if ! command -v jq >/dev/null 2>&1; then
    echo "::error::regen-baseline: jq required" >&2
    exit 1
fi

GIT_SHA=$(git rev-parse --short HEAD 2>/dev/null || echo "unknown")

mkdir -p "$BASELINE_DIR"

mapfile -t CRATES < <(jq -r '.by_crate[]?.crate_name' "$ARTIFACT" | sort -u)

written=0
for crate in "${CRATES[@]}"; do
    [[ -z "$crate" ]] && continue
    out_path="${BASELINE_DIR}/${crate}.txt"
    {
        cat <<HEADER
# Public-API spec audit baseline (mokumo#654) — ${crate}
#
# Frozen list of pub items that lacked BDD coverage at gate-live.
# Regenerate via \`scripts/regen-pub-api-spec-audit-baseline.sh\`
# — see ops/standards/testing.md §"Public-API spec audit".
#
# Last regenerated against commit ${GIT_SHA}.

HEADER
        jq -r --arg c "$crate" '
            .by_crate[]
            | select(.crate_name == $c)
            | .items[]
            | select(.bdd_covered_lines == 0)
            | .item_path
        ' "$ARTIFACT" \
            | sort -u \
            | awk '{ printf "%s  # baselined: '"${GIT_SHA}"'\n", $1 }'
    } > "${out_path}.tmp"
    mv "${out_path}.tmp" "$out_path"
    n=$(grep -cv '^#\|^$' "$out_path" || true)
    echo "regen-baseline: wrote $out_path ($n entries)"
    written=$(( written + 1 ))
done

echo "regen-baseline: regenerated baselines for $written crate(s)"
