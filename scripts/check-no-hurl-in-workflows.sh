#!/usr/bin/env bash
# Guard: workflow YAML must not name *.hurl files (mokumo#383).
#
# `crates/mokumo-shop/moon.yml` is the single source of truth for the api-smoke
# suite — what hurl files run, what env they get, how the server is bootstrapped
# and seeded. CI workflow YAML exists to set up the environment and invoke
# `moon run shop:smoke`; it must not maintain its own list of hurl paths.
#
# Background: between 2026-04-05 (when the smoke task shipped) and 2026-04-26,
# CI's `api-smoke` job inlined a 2-file `hurl` invocation while the moon task
# grew to an 11-file list. The drift hid 8 broken tests because nothing was
# actually executing them. This guard prevents that re-occurring.
set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
# shellcheck source=lib/rg-check.sh
source "${HERE}/lib/rg-check.sh"

TARGET="${1:-.github/workflows}"

# Match any reference to a `.hurl` file path in workflow YAML.
PATTERN='\.hurl\b'
rg_no_match_or_die "G1/no-hurl-in-workflows" "$PATTERN" "$TARGET"
echo "G1/no-hurl-in-workflows ok: ${TARGET} contains no *.hurl path references"
