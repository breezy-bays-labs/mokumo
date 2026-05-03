#!/usr/bin/env bash
# Self-tests for invariant-check scripts.
#
# Validates each script's contract:
#   1. real-tree run → exit 0 (currently passes organically)
#   2. fixture run → exit 1 (planted violation)
#
# I3/I4 fixtures are not feasible (would require synthetic Cargo workspaces);
# they're covered by the in-PR plant-and-revert acceptance verification.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${HERE}/../.." && pwd)"
FIX="${HERE}/fixtures"

pass=0
fail=0

assert_exit() {
    local label="$1"
    local want="$2"
    shift 2
    set +e
    "$@" >/dev/null 2>&1
    local got=$?
    set -e
    if [[ "$got" -eq "$want" ]]; then
        echo "ok   ${label} (exit ${got})"
        pass=$((pass+1))
    else
        echo "FAIL ${label}: want exit ${want}, got ${got}"
        fail=$((fail+1))
    fi
}

cd "$ROOT"

# Real-tree must pass.
assert_exit "I1 real-tree pass" 0 bash scripts/check-i1-domain-purity.sh
assert_exit "I2 real-tree pass" 0 bash scripts/check-i2-adapter-boundary.sh
assert_exit "I2b real-tree pass" 0 bash scripts/check-i2b-tauri-type-ids.sh
# I3 covers both default and no-default features configurations internally;
# a single pass assertion validates mokumo-server is Tauri-free under each (#554).
assert_exit "I3 real-tree pass (default + no-default features)" 0 bash scripts/check-i3-headless.sh
assert_exit "I4 real-tree pass" 0 bash scripts/check-i4-dag.sh
assert_exit "I5 real-tree pass" 0 bash scripts/check-i5-features.sh
assert_exit "R13 real-tree pass" 0 bash scripts/check-r13-action-strings.sh
assert_exit "route-coverage real-tree pass" 0 bash scripts/check-route-coverage.sh
assert_exit "docs-paired-files real-tree pass" 0 bash scripts/check-docs-paired-files.sh

# R13 fixture: a file containing a forbidden prefixed literal must fail.
R13_FIX="$(mktemp)"
cat >"$R13_FIX" <<'EOF'
pub const fn as_str(&self) -> &'static str {
    match self {
        Self::Created => "customer_created",
        Self::Updated => "updated",
        Self::SoftDeleted => "soft_deleted",
        Self::Restored => "restored",
    }
}
EOF
assert_exit "R13 fixture fail"  1 env TARGET="$R13_FIX" bash scripts/check-r13-action-strings.sh
rm -f "$R13_FIX"

# Fixtures must fail.
assert_exit "I1 fixture fail"   1 bash scripts/check-i1-domain-purity.sh "${FIX}/i1-violation/src"
assert_exit "I2 fixture fail"   1 bash scripts/check-i2-adapter-boundary.sh "${FIX}/i2-violation/src"
assert_exit "I2b fixture fail"  1 bash scripts/check-i2b-tauri-type-ids.sh  "${FIX}/i2b-violation/src"
assert_exit "I5 fixture fail"   1 bash scripts/check-i5-features.sh        "${FIX}/i5-violation/Cargo.toml"

# route-coverage fixture: synthetic diff adding /api/widgets with no
# tests/api/widgets/ tree and no exclusion ledger entry must fail.
assert_exit "route-coverage fixture fail" 1 \
    env DIFF_OVERRIDE="${FIX}/route-coverage-violation/diff.txt" \
        HURL_TREE="${FIX}/route-coverage-violation/empty-hurl-tree" \
        LEDGER_FILE="${FIX}/route-coverage-violation/empty-ledger.yml" \
    bash scripts/check-route-coverage.sh

# route-coverage pass via existing-domain coverage.
assert_exit "route-coverage existing-domain pass" 0 \
    env DIFF_OVERRIDE="${FIX}/route-coverage-pass/diff.txt" \
        HURL_TREE="${FIX}/route-coverage-pass/api" \
        LEDGER_FILE="${FIX}/route-coverage-pass/empty-ledger.yml" \
    bash scripts/check-route-coverage.sh

# route-coverage pass via exclusion ledger entry.
assert_exit "route-coverage ledger pass" 0 \
    env DIFF_OVERRIDE="${FIX}/route-coverage-violation/diff-single.txt" \
        HURL_TREE="${FIX}/route-coverage-violation/empty-hurl-tree" \
        LEDGER_FILE="${FIX}/route-coverage-violation/ledger-with-entry.yml" \
    bash scripts/check-route-coverage.sh

# route-coverage substring-ledger guard: route /api/users must NOT be
# considered "covered" by a ledger entry that only mentions /api/users/roles.
assert_exit "route-coverage ledger substring rejected" 1 \
    env DIFF_OVERRIDE="${FIX}/route-coverage-ledger-substring/diff.txt" \
        HURL_TREE="${FIX}/route-coverage-violation/empty-hurl-tree" \
        LEDGER_FILE="${FIX}/route-coverage-ledger-substring/ledger-only-subpath.yml" \
    bash scripts/check-route-coverage.sh

# Nested-mount resolver. A relative route added inside `customer_router()`
# must be resolved to /api/customers/<rel> by walking routes.rs `.nest(...)`.
assert_exit "route-coverage nested-mount pass" 0 \
    env DIFF_OVERRIDE="${FIX}/route-coverage-nested-mount/diff.txt" \
        HURL_TREE="${FIX}/route-coverage-nested-mount/api" \
        LEDGER_FILE="${FIX}/route-coverage-nested-mount/empty-ledger.yml" \
        ROUTES_FILES="${FIX}/route-coverage-nested-mount/routes.rs" \
        ROUTER_FN_OVERRIDE="${FIX}/route-coverage-nested-mount/fn-overrides.txt" \
    bash scripts/check-route-coverage.sh

# Per-method gap. Adding POST to a route where only GET is hurl-covered must
# FAIL even though the domain `/api/customers/` has hurl coverage.
assert_exit "route-coverage per-method gap fails" 1 \
    env DIFF_OVERRIDE="${FIX}/route-coverage-per-method-gap/diff.txt" \
        HURL_TREE="${FIX}/route-coverage-per-method-gap/api" \
        LEDGER_FILE="${FIX}/route-coverage-per-method-gap/empty-ledger.yml" \
    bash scripts/check-route-coverage.sh

# Multi-line `.route(\n  "<path>",\n  ...,\n)` block — the dominant style in
# routes.rs. The script must detect the route by joining consecutive added
# lines per file before applying the route-extraction regex.
assert_exit "route-coverage multi-line route pass" 0 \
    env DIFF_OVERRIDE="${FIX}/route-coverage-multi-line/diff.txt" \
        HURL_TREE="${FIX}/route-coverage-multi-line/api" \
        LEDGER_FILE="${FIX}/route-coverage-multi-line/empty-ledger.yml" \
    bash scripts/check-route-coverage.sh

# Multi-nest routes.rs — every .nest("/api/<prefix>", ...) call must register
# in the prefix map. A pre-fix script (greedy `.*\.nest\(` matches only the
# LAST nest in a file) silently skips routes mounted under earlier nests.
# This fixture adds a route in BOTH nests (quotes + invoices) but only
# provides hurl coverage for the LAST one. A pre-fix script would consider
# only the invoice route, find it covered, and exit 0 spuriously. The fixed
# script enumerates both nests and reports the missing quote-route coverage.
assert_exit "route-coverage multi-nest first-of-many fails" 1 \
    env DIFF_OVERRIDE="${FIX}/route-coverage-multi-nest/diff.txt" \
        HURL_TREE="${FIX}/route-coverage-multi-nest/api" \
        LEDGER_FILE="${FIX}/route-coverage-multi-nest/empty-ledger.yml" \
        ROUTES_FILES="${FIX}/route-coverage-multi-nest/routes.rs" \
        ROUTER_FN_OVERRIDE="${FIX}/route-coverage-multi-nest/fn-overrides.txt" \
    bash scripts/check-route-coverage.sh

# Regex-meta in literal path segment (`/api/foo.bar`) must NOT be matched by
# a hurl request line that has any other character in the same position
# (`/api/fooXbar`). path_to_regex must escape `.` correctly.
assert_exit "route-coverage dot-escape rejects fuzzy match" 1 \
    env DIFF_OVERRIDE="${FIX}/route-coverage-dot-escape/diff.txt" \
        HURL_TREE="${FIX}/route-coverage-dot-escape/api" \
        LEDGER_FILE="${FIX}/route-coverage-dot-escape/empty-ledger.yml" \
    bash scripts/check-route-coverage.sh

# === docs-paired-files (mokumo#776) =======================================
# Each fixture pairs `diff.txt` (recorded `git diff` output) with `names.txt`
# (recorded `git diff --name-only` output) so the gate can be exercised
# without a live git tree.

# Public surface added in mokumo-shop with no LANGUAGE.md change → fail.
assert_exit "docs-paired-files violation fails" 1 \
    env DIFF_OVERRIDE="${FIX}/docs-paired-files-violation/diff.txt" \
        NAME_OVERRIDE="${FIX}/docs-paired-files-violation/names.txt" \
    bash scripts/check-docs-paired-files.sh

# Same public surface but the docs-not-applicable label is set → pass.
assert_exit "docs-paired-files opt-out label passes" 0 \
    env DIFF_OVERRIDE="${FIX}/docs-paired-files-violation/diff.txt" \
        NAME_OVERRIDE="${FIX}/docs-paired-files-violation/names.txt" \
        PR_LABELS="docs-not-applicable" \
    bash scripts/check-docs-paired-files.sh

# Public surface in mokumo-shop AND LANGUAGE.md touched → pass.
assert_exit "docs-paired-files shop pair passes" 0 \
    env DIFF_OVERRIDE="${FIX}/docs-paired-files-pass-shop/diff.txt" \
        NAME_OVERRIDE="${FIX}/docs-paired-files-pass-shop/names.txt" \
    bash scripts/check-docs-paired-files.sh

# Public surface in kikan-types (a §B kikan satellite) AND
# crates/kikan/LANGUAGE.md touched → pass. Validates the wider §B scope: any
# of the 9 kikan-* satellites maps to the platform glossary.
assert_exit "docs-paired-files kikan-types pair passes" 0 \
    env DIFF_OVERRIDE="${FIX}/docs-paired-files-pass-kikan/diff.txt" \
        NAME_OVERRIDE="${FIX}/docs-paired-files-pass-kikan/names.txt" \
    bash scripts/check-docs-paired-files.sh

# Spine case: pub surface in mokumo-shop, but only crates/kikan/LANGUAGE.md
# was touched (the WRONG glossary) → fail. The script must keep separate
# accounting per crate-prefix; "any doc touched" is not the contract.
assert_exit "docs-paired-files wrong-doc fails (spine)" 1 \
    env DIFF_OVERRIDE="${FIX}/docs-paired-files-wrong-doc/diff.txt" \
        NAME_OVERRIDE="${FIX}/docs-paired-files-wrong-doc/names.txt" \
    bash scripts/check-docs-paired-files.sh

# Restricted-pub additions only (`pub(crate)`, `pub(super)`, `pub(in …)`) do
# not contribute to crate surface and must not trigger the gate.
assert_exit "docs-paired-files pub(crate)-only passes" 0 \
    env DIFF_OVERRIDE="${FIX}/docs-paired-files-pubcrate/diff.txt" \
        NAME_OVERRIDE="${FIX}/docs-paired-files-pubcrate/names.txt" \
    bash scripts/check-docs-paired-files.sh

# `pub fn` outside any §B path (e.g. tools/docs-gen/src/) must not trigger.
assert_exit "docs-paired-files out-of-scope passes" 0 \
    env DIFF_OVERRIDE="${FIX}/docs-paired-files-out-of-scope/diff.txt" \
        NAME_OVERRIDE="${FIX}/docs-paired-files-out-of-scope/names.txt" \
    bash scripts/check-docs-paired-files.sh

# Pure file rename inside an in-scope path with no `+pub` lines must not
# trigger. Rename-with-content-edit is not testable from a fixture (git
# generates rename markers via similarity index, not literal `+pub` lines)
# so this fixture only covers the pure-rename case.
assert_exit "docs-paired-files rename-only passes" 0 \
    env DIFF_OVERRIDE="${FIX}/docs-paired-files-renamed-rs/diff.txt" \
        NAME_OVERRIDE="${FIX}/docs-paired-files-renamed-rs/names.txt" \
    bash scripts/check-docs-paired-files.sh

# Modifier-prefixed pub items (`pub async fn`, `pub unsafe trait`,
# `pub extern fn`) must register as public surface; Axum handlers are nearly
# all `pub async fn`, so missing this case would silently let the gate green
# on the most common shape it's meant to catch.
assert_exit "docs-paired-files pub async/unsafe fails without doc" 1 \
    env DIFF_OVERRIDE="${FIX}/docs-paired-files-async-fn/diff.txt" \
        NAME_OVERRIDE="${FIX}/docs-paired-files-async-fn/names.txt" \
    bash scripts/check-docs-paired-files.sh

echo
echo "self-tests: ${pass} passed, ${fail} failed"
[[ "$fail" -eq 0 ]]
