#!/usr/bin/env bash
# I1 — Domain purity (source side).
#
# kikan platform crate must contain zero references to garment-vertical
# domain language (classic I1) AND no leaked shop-vertical wire artifacts
# (strict I1) — the `SetupMode` variant name and the `mokumo.db` DB
# filename literal. Enforces the boundary stated in:
#   - adr-workspace-split-kikan (I1)
#   - adr-kikan-engine-vocabulary (capability/vocabulary split)
#   - adr-workspace-ci-testing
#
# Classic I1 = "shop nouns" (customer, garment, quote, …) — kikan must not
# mention them even in negated doc-comment prose.
# Strict I1 = "leaked wire artifact" — kikan must not name the vertical's
# profile-kind enum or per-profile DB filename in production code. This
# catches the case where kikan has dropped the shop nouns but still
# depends on the vertical's `SetupMode` + `mokumo.db` wire shapes.
#
# Allows override of TARGET dir for fixture self-tests.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/rg-check.sh
source "${HERE}/lib/rg-check.sh"

TARGET="${1:-crates/kikan/src}"

# Classic I1: whole-word shop-vertical nouns.
PATTERN_CLASSIC='\b(customer|garment|print_job|quote|invoice|decorator|embroidery|dtf|screen.print|apparel)\b'
rg_no_match_or_die "I1/classic" "$PATTERN_CLASSIC" "$TARGET"
echo "I1/classic ok: ${TARGET} contains no garment-domain identifiers"

# Strict I1: leaked vertical wire artifacts. Excludes sibling *_tests.rs
# files (and **/tests.rs) — test code may reference the mokumo-specific
# fixtures without violating the boundary.
PATTERN_STRICT='\bSetupMode\b|"mokumo\.db"'
set +e
rg -n --color=never \
    -g '!**/tests.rs' \
    -g '!*_tests.rs' \
    "$PATTERN_STRICT" "$TARGET"
rc=$?
set -e
case "$rc" in
    0)
        echo "::error::I1/strict violated: pattern '${PATTERN_STRICT}' matched in ${TARGET} (see file:line above)" >&2
        exit 1
        ;;
    1) ;;
    *)
        echo "::error::I1/strict script error: rg exited ${rc}" >&2
        exit "$rc"
        ;;
esac
echo "I1/strict ok: ${TARGET} contains no SetupMode or \"mokumo.db\" in production code"
