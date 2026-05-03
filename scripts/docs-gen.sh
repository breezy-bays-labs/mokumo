#!/usr/bin/env bash
# Regenerates all <!-- AUTO-GEN:* --> sections in repository docs.
# Safe to re-run: output is purely a function of source files (no timestamps,
# no ordering jitter). Run via `moon run docs:gen` or directly.
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR/.."

# --- AUTO-GEN:msrv — MSRV badge in README.md ---------------------------------
MSRV=$(grep '^channel = ' rust-toolchain.toml | sed 's/channel = "\(.*\)"/\1/')
BADGE="[![MSRV](https://img.shields.io/badge/MSRV-${MSRV}-blue.svg)](rust-toolchain.toml)"

awk -v badge="$BADGE" '
  /<!-- AUTO-GEN:msrv -->/ { print; print badge; skip=1; next }
  /<!-- \/AUTO-GEN:msrv -->/ { skip=0 }
  !skip { print }
' README.md > README.md.tmp && mv README.md.tmp README.md
# -----------------------------------------------------------------------------

echo "docs-gen: MSRV=${MSRV}"
