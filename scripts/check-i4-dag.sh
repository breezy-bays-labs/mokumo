#!/usr/bin/env bash
# I4 — Dependency direction (DAG enforcement).
#
# Forbidden edges per ADR adr-workspace-split-kikan §I4:
#   - kikan must not depend on any of its consumers, the mokumo-shop vertical,
#     or any adapter shell.
#   - mokumo-shop must not depend on any adapter or binary.
#
# This is enforced via `cargo tree` — kikan's dep tree must contain none of
# the forbidden downstream crate names, and mokumo-shop's must contain no
# adapter/binary names.
#
# Why not cargo-deny `bans.deny`? `wrappers` semantics is "ban this crate
# UNLESS the parent is in the wrapper list" — the inverse of "ban this edge
# only when consumer is X". cargo-deny also silently no-ops `wrappers` for
# crates listed in `[graph] exclude` (kikan-tauri, mokumo-desktop).
#
# Note: I4.a's check that kikan does not depend on mokumo-shop is also the
# dependency-side enforcement of I1 (domain purity) — kikan cannot acquire
# shop-vertical vocabulary by linking the vertical crate.
set -euo pipefail

# Returns 1 (violation) if any forbidden crate appears in the dep tree of $1.
check_no_forbidden_deps() {
    local pkg="$1"
    shift
    local forbidden_pattern
    forbidden_pattern="\\b($(IFS='|'; echo "$*"))\\b"

    local tree
    tree="$(env -u RUSTC_WRAPPER cargo tree -p "$pkg" --edges normal,build --prefix none 2>/dev/null || true)"
    if [[ -z "$tree" ]]; then
        echo "::error::I4 script error: cargo tree -p ${pkg} produced no output" >&2
        exit 2
    fi

    if echo "$tree" | grep -E "$forbidden_pattern" >/tmp/i4-hits.$$; then
        echo "::error::I4 violated: ${pkg} transitively depends on a forbidden downstream crate" >&2
        echo "Offending entries:" >&2
        sort -u /tmp/i4-hits.$$ >&2
        rm -f /tmp/i4-hits.$$
        return 1
    fi
    rm -f /tmp/i4-hits.$$
    return 0
}

fail=0

# I4.a — kikan must not depend on its consumers, the mokumo-shop vertical,
# any adapter shell, or any SubGraft satellite.
check_no_forbidden_deps kikan \
    mokumo-shop \
    mokumo-server \
    mokumo-desktop \
    kikan-tauri \
    kikan-socket \
    kikan-cli \
    kikan-events \
    kikan-mail \
    kikan-scheduler \
    || fail=1

# I4.b — mokumo-shop must not depend on adapters or binaries.
check_no_forbidden_deps mokumo-shop \
    kikan-tauri \
    mokumo-desktop \
    mokumo-server \
    || fail=1

if [[ $fail -ne 0 ]]; then
    exit 1
fi

echo "I4 ok: dependency DAG holds (kikan, mokumo-shop have no forbidden downstream deps)"
