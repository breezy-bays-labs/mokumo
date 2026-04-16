#!/usr/bin/env bash
# I5 — Feature gate ownership.
#
# kikan/Cargo.toml must not reference Tauri at all — no direct dep, no feature
# pulling Tauri transitively. See:
#   - adr-workspace-split-kikan (I5)
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/rg-check.sh
source "${HERE}/lib/rg-check.sh"

TARGET="${1:-crates/kikan/Cargo.toml}"

# Plain word `tauri` — covers `tauri = ...`, `tauri-plugin-...`, and
# `["tauri/something"]` feature activations.
PATTERN='\btauri\b'

rg_no_match_or_die "I5" "$PATTERN" "$TARGET"
echo "I5 ok: ${TARGET} has no Tauri references"
