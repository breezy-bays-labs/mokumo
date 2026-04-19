#!/usr/bin/env bash
# I3a — Headless server has zero Tauri / webview deps.
#
# mokumo-server must compile and link without dragging in Tauri, webkit2gtk,
# or wry — they would block container/musl deployment. See:
#   - adr-workspace-split-kikan (I3)
#   - adr-kikan-binary-topology
#
# The check runs cargo tree under two feature configurations:
#   - default features: production path; what the release binary links
#   - no-default features: guards against a future default feature masking
#     a Tauri-bearing dep. If a feature becomes non-default but still pulls
#     Tauri, the default-only audit would miss it as the feature surface
#     grows (#554).
#
# I3b (musl cross-compile) is a separate CI job; see kikan-musl-build.
set -euo pipefail

PKG="${1:-mokumo-server}"
FORBIDDEN='\b(tauri|tauri-build|webkit2gtk|wry)\b'

check_tree() {
    local label="$1"
    shift
    local tree
    tree="$(env -u RUSTC_WRAPPER cargo tree -p "$PKG" "$@" --edges normal,build --prefix none 2>/dev/null || true)"

    if [[ -z "$tree" ]]; then
        echo "::error::I3 script error: cargo tree ${label} for ${PKG} produced no output" >&2
        return 2
    fi

    local hits
    hits="$(echo "$tree" | grep -iE "$FORBIDDEN" | sort -u || true)"
    if [[ -n "$hits" ]]; then
        echo "::error::I3 violated (${label}): ${PKG} transitively depends on Tauri/webview crates" >&2
        echo "Offending entries:" >&2
        echo "$hits" >&2
        return 1
    fi
    echo "I3 ok (${label}): ${PKG} has no Tauri/webview transitive deps"
    return 0
}

rc=0
check_tree "default features" || rc=$?
if [[ "$rc" -ne 0 ]]; then exit "$rc"; fi

check_tree "no-default features" --no-default-features || rc=$?
exit "$rc"
