#!/usr/bin/env bash
# I2 — Adapter boundary (source side).
#
# kikan platform must not reference Tauri types or commands. Tauri integration
# lives in kikan-tauri (the OS-capability adapter shell). See:
#   - adr-workspace-split-kikan (I2)
#   - adr-control-plane-data-plane-split
#
# Allows override of TARGET dir for fixture self-tests.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/rg-check.sh
source "${HERE}/lib/rg-check.sh"

TARGET="${1:-crates/kikan/src}"

PATTERN='\btauri::|#\[tauri::command\]'

rg_no_match_or_die "I2" "$PATTERN" "$TARGET"
echo "I2 ok: ${TARGET} contains no Tauri references"
