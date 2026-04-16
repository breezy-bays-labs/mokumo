#!/usr/bin/env bash
# I1 — Domain purity (source side).
#
# kikan platform crate must contain zero references to garment-vertical
# domain language. Enforces the boundary stated in:
#   - adr-workspace-split-kikan (I1)
#   - adr-workspace-ci-testing
#
# Allows override of TARGET dir for fixture self-tests.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/rg-check.sh
source "${HERE}/lib/rg-check.sh"

TARGET="${1:-crates/kikan/src}"

# Whole-word match — narrow, low false-positive rate. If a future kikan use
# legitimately needs one of these words (e.g. "order_by"), tighten the regex
# rather than broaden the deny-list.
PATTERN='\b(customer|garment|print_job|quote|invoice|decorator|embroidery|dtf|screen.print|apparel)\b'

rg_no_match_or_die "I1" "$PATTERN" "$TARGET"
echo "I1 ok: ${TARGET} contains no garment-domain identifiers"
