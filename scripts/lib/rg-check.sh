#!/usr/bin/env bash
# Shared rg "no match = success" idiom for invariant checks.
#
# rg exit codes: 0 = match found, 1 = no match, 2+ = real error.
# `set -euo pipefail` makes naive `if rg ...; then` swallow exit 2 silently.
# This wrapper propagates exit 2+ as a script error, treats 0 as the violation,
# and treats 1 as success.
#
# Usage:
#   rg_no_match_or_die "<invariant-id>" "<pattern>" <target...>

rg_no_match_or_die() {
    local invariant="$1"
    local pattern="$2"
    shift 2
    local rc=0

    set +e
    rg -n --color=never "$pattern" "$@"
    rc=$?
    set -e

    case "$rc" in
        0)
            echo "::error::${invariant} violated: pattern '${pattern}' matched in $* (see file:line above)" >&2
            exit 1
            ;;
        1)
            return 0
            ;;
        *)
            echo "::error::${invariant} script error: rg exited ${rc}" >&2
            exit "$rc"
            ;;
    esac
}
