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

echo
echo "self-tests: ${pass} passed, ${fail} failed"
[[ "$fail" -eq 0 ]]
