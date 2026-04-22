#!/usr/bin/env bash
# Per-session CRAP delta gate — fails if any function's CRAP score increased
# between the given base ref and HEAD on a file that's changed on HEAD.
#
# Usage: scripts/check-crap-delta.sh [base-ref]
#   base-ref defaults to origin/main.
#
# Implements gh#569. For refactor-heavy sessions we want "no CRAP regression on
# changed code" even when scores stay under the crap4rs global threshold.
#
# Strategy: run crap4rs --format json on BOTH refs, limited to changed files on
# the HEAD side via --diff. Match functions by (file_path, qualified_name).
# Exit non-zero if any match shows a higher CRAP value on HEAD.
#
# Expensive (two coverage runs). Intended for CI / explicit local invocation,
# not pre-commit. See ops/standards/testing.md#crap-delta-gate.

set -euo pipefail

BASE="${1:-origin/main}"
REPO_ROOT="$(git rev-parse --show-toplevel)"
cd "$REPO_ROOT"

TMPDIR="$(mktemp -d)"
trap "rm -rf $TMPDIR" EXIT

echo "[crap-delta] Base ref: $BASE"
echo "[crap-delta] Repo:     $REPO_ROOT"

git fetch --no-tags --depth=1000 origin "${BASE#origin/}" 2>/dev/null || true
git rev-parse --verify "$BASE" >/dev/null

build_and_dump() {
  local outfile="$1"
  local diff_ref="${2-}"
  # shop:crap covers web:build + cargo llvm-cov; we re-run the cargo step so we
  # can capture --format json (shop:crap uses table output).
  moon run web:build >/dev/null
  cargo llvm-cov nextest \
    --workspace \
    --exclude mokumo-desktop \
    --exclude kikan-tauri \
    --lcov \
    --output-path "$TMPDIR/lcov.info" >/dev/null

  if [ -n "$diff_ref" ]; then
    crap4rs \
      --coverage "$TMPDIR/lcov.info" \
      --src . \
      --format json \
      --diff "$diff_ref" \
      > "$outfile"
  else
    crap4rs \
      --coverage "$TMPDIR/lcov.info" \
      --src . \
      --format json \
      > "$outfile"
  fi
}

echo "[crap-delta] Capturing HEAD CRAP snapshot (changed files only via --diff)..."
build_and_dump "$TMPDIR/head.json" "$BASE"

BASE_WORKTREE="$TMPDIR/base-worktree"
echo "[crap-delta] Creating base worktree at $BASE_WORKTREE..."
git worktree add --detach "$BASE_WORKTREE" "$BASE" >/dev/null

echo "[crap-delta] Capturing BASE CRAP snapshot (full) in worktree..."
(
  cd "$BASE_WORKTREE"
  # pnpm install in case lockfile shifted between base and head
  pnpm install --frozen-lockfile >/dev/null
  build_and_dump "$TMPDIR/base.json"
)

git worktree remove --force "$BASE_WORKTREE" >/dev/null

echo "[crap-delta] Comparing..."
REGRESSIONS=$(jq -s '
  .[0].result.functions as $base
  | .[1].result.functions as $head
  | [
      $head[]
      | . as $hf
      | ($base[]
          | select(
              .scored.identity.file_path == $hf.scored.identity.file_path
              and .scored.identity.qualified_name == $hf.scored.identity.qualified_name
            )
        ) as $bf
      | select($hf.scored.crap.value > $bf.scored.crap.value + 0.01)
      | {
          file: $hf.scored.identity.file_path,
          function: $hf.scored.identity.qualified_name,
          base_crap: $bf.scored.crap.value,
          head_crap: $hf.scored.crap.value,
          delta: ($hf.scored.crap.value - $bf.scored.crap.value)
        }
    ]
' "$TMPDIR/base.json" "$TMPDIR/head.json")

REGRESSION_COUNT=$(echo "$REGRESSIONS" | jq 'length')

if [ "$REGRESSION_COUNT" -gt 0 ]; then
  echo "[crap-delta] FAIL: $REGRESSION_COUNT function(s) regressed:"
  echo "$REGRESSIONS" | jq -r '.[] | "  \(.file)::\(.function)  \(.base_crap) -> \(.head_crap)  (+\(.delta))"'
  echo ""
  echo "Full JSON:"
  echo "$REGRESSIONS" | jq .
  exit 1
fi

echo "[crap-delta] PASS: no CRAP regressions on changed functions"
