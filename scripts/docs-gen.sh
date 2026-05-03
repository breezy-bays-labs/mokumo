#!/usr/bin/env bash
# Regenerates all <!-- AUTO-GEN:* --> sections in repository docs.
# Safe to re-run: output is purely a function of source files (no timestamps,
# no ordering jitter). Run via `moon run docs:gen` or directly.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# Clean up temp file on any exit (success or failure)
trap 'rm -f README.md.tmp' EXIT

# --- AUTO-GEN:msrv — MSRV badge in README.md ---------------------------------
MSRV=$(sed -n 's/^channel = "\(.*\)"/\1/p' rust-toolchain.toml)
[[ -n "$MSRV" ]] || { echo "docs-gen: ERROR — could not extract MSRV from rust-toolchain.toml" >&2; exit 1; }

# Verify markers exist before rewriting — awk silently no-ops if they're absent
grep -q '<!-- AUTO-GEN:msrv -->'  README.md || { echo "docs-gen: ERROR — opening marker <!-- AUTO-GEN:msrv --> missing from README.md" >&2; exit 1; }
grep -q '<!-- /AUTO-GEN:msrv -->' README.md || { echo "docs-gen: ERROR — closing marker <!-- /AUTO-GEN:msrv --> missing from README.md" >&2; exit 1; }

# Escape hyphens for shields.io URL format (nightly-YYYY-MM-DD needs -- per segment)
MSRV_ENCODED="${MSRV//-/--}"
BADGE="[![MSRV](https://img.shields.io/badge/MSRV-${MSRV_ENCODED}-blue.svg)](rust-toolchain.toml)"

awk -v badge="$BADGE" '
  /<!-- AUTO-GEN:msrv -->/ { print; print badge; skip=1; next }
  /<!-- \/AUTO-GEN:msrv -->/ { skip=0 }
  !skip { print }
' README.md > README.md.tmp && mv README.md.tmp README.md
# -----------------------------------------------------------------------------

echo "docs-gen: MSRV=${MSRV}"
