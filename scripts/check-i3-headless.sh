#!/usr/bin/env bash
# I3a — Headless server has zero Tauri / webview deps.
#
# mokumo-server must compile and link without dragging in Tauri, webkit2gtk,
# or wry — they would block container/musl deployment. See:
#   - adr-workspace-split-kikan (I3)
#   - adr-kikan-binary-topology
#
# I3b (musl cross-compile) is a separate CI job; see kikan-musl-build.
set -euo pipefail

PKG="${1:-mokumo-server}"

# Capture cargo tree separately so `set -e` doesn't abort on grep no-match.
tree="$(env -u RUSTC_WRAPPER cargo tree -p "$PKG" --edges normal,build --prefix none 2>/dev/null || true)"

if [[ -z "$tree" ]]; then
    echo "::error::I3 script error: cargo tree -p ${PKG} produced no output" >&2
    exit 2
fi

if echo "$tree" | grep -iE '\b(tauri|tauri-build|webkit2gtk|wry)\b' >/tmp/i3-hits.$$; then
    echo "::error::I3 violated: ${PKG} transitively depends on Tauri/webview crates" >&2
    echo "Offending entries:" >&2
    sort -u /tmp/i3-hits.$$ >&2
    rm -f /tmp/i3-hits.$$
    exit 1
fi
rm -f /tmp/i3-hits.$$

echo "I3 ok: ${PKG} has no Tauri/webview transitive deps"
