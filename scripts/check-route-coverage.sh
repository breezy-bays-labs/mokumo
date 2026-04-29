#!/usr/bin/env bash
# Guard: every newly-introduced `.route("/api/...")` or `.nest("/api/...")`
# in the diff requires a sibling `.hurl` file (or an entry in the moon.yml
# exclusion ledger).
#
# Scope (v1 — lenient by design):
#   - Detects added lines containing literal `/api/...` paths in
#     `.route("...")` / `.nest("...")` calls under `crates/**/src/**/*.rs`.
#   - Passes if the route's first /api/<domain> segment has any sibling
#     hurl coverage, or if a new hurl was added in the same diff, or if the
#     literal route is in the exclusion ledger.
#   - **Known false-negative:** sub-router internals using relative paths
#     (e.g. `.route("/{id}/restore")` inside a `.nest("/api/customers", ...)`
#     parent) are not detected in v1. Backfill those manually until the
#     parent prefix resolver lands in v2.
#
# Background: mokumo CLAUDE.md mandates per-endpoint hurl coverage:
#   "New API endpoints require a `.hurl` file — add
#   `tests/api/<domain>/<endpoint>.hurl` in the same PR."
# Until this script landed, only G1 (no hurl in workflows) was enforced;
# the per-endpoint mandate itself was honor-system. mokumo#727 / audit F4.
#
# Diff base: `origin/main`. Local hooks rely on the user having fetched
# recently; stale base is acceptable for local checks because CI is the
# source of truth.

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${HERE}/.." && pwd)"
cd "$ROOT"

BASE_REF="${BASE_REF:-origin/main}"
LEDGER_FILE="${LEDGER_FILE:-crates/mokumo-shop/moon.yml}"
HURL_TREE="${HURL_TREE:-tests/api}"

# DIFF_OVERRIDE lets self-tests inject a synthetic diff via a file path.
DIFF_OVERRIDE="${DIFF_OVERRIDE:-}"

if [[ -n "$DIFF_OVERRIDE" ]]; then
    DIFF=$(cat "$DIFF_OVERRIDE")
else
    if ! git rev-parse --verify --quiet "$BASE_REF" >/dev/null; then
        # CI sets STRICT_BASE_REF=1 — missing base is a hard failure there.
        # Locally (lefthook, ad-hoc runs) we warn-skip so a stale fetch
        # doesn't block the developer.
        if [[ "${STRICT_BASE_REF:-0}" == "1" ]]; then
            echo "::error::route-coverage: base ref '$BASE_REF' not found (STRICT_BASE_REF=1)" >&2
            exit 2
        fi
        echo "::warning::route-coverage: base ref '$BASE_REF' not found locally; skipping (run \`git fetch origin main\` to enable)" >&2
        exit 0
    fi
    DIFF=$(git diff --unified=0 "${BASE_REF}...HEAD" -- 'crates/**/src/**/*.rs' 2>/dev/null || true)
fi

if [[ -z "$DIFF" ]]; then
    echo "route-coverage ok: no Rust source changes vs ${BASE_REF}"
    exit 0
fi

# Extract added lines containing .route("/api/...") or .nest("/api/...").
# We only match literal `/api/` prefixes — relative paths in sub-routers
# are explicit non-goals for v1 (see header).
NEW_ROUTES=$(printf '%s\n' "$DIFF" \
    | grep -E '^\+[^+]' \
    | grep -oE '\.(route|nest)\("/api/[^"]+"' \
    | sed -E 's#^\.(route|nest)\("(/api/[^"]+)"$#\2#' \
    | sort -u || true)

if [[ -z "$NEW_ROUTES" ]]; then
    echo "route-coverage ok: no new /api/ routes introduced in diff"
    exit 0
fi

# Collect new hurl files added in same diff (for the in-PR coverage case).
if [[ -n "$DIFF_OVERRIDE" ]]; then
    NEW_HURL=""
else
    NEW_HURL=$(git diff --name-only --diff-filter=A "${BASE_REF}...HEAD" -- "${HURL_TREE}/**/*.hurl" 2>/dev/null || true)
fi

# Read the exclusion ledger once. Format: free-form Hurl-style comments
# in moon.yml mentioning routes that are intentionally not covered, e.g.
# "# Excluded: POST /api/demo/reset (restarts server; demo-smoke covers)".
# We match by exact literal route appearing anywhere in the ledger file.
if [[ -f "$LEDGER_FILE" ]]; then
    LEDGER=$(cat "$LEDGER_FILE")
else
    LEDGER=""
fi

FAIL=0

while IFS= read -r route; do
    [[ -z "$route" ]] && continue

    # domain = first path segment after /api/
    domain=$(printf '%s\n' "$route" | sed -E 's|^/api/([^/]+).*$|\1|')

    # (a) existing hurl tree for the domain
    if compgen -G "${HURL_TREE}/${domain}/*.hurl" > /dev/null 2>&1; then
        echo "route-coverage ok: $route (covered by ${HURL_TREE}/${domain}/)"
        continue
    fi
    if [[ -f "${HURL_TREE}/${domain}.hurl" ]]; then
        echo "route-coverage ok: $route (covered by ${HURL_TREE}/${domain}.hurl)"
        continue
    fi

    # (b) new hurl in this diff under the matching domain.
    # Fixed-string matching: `domain` may contain regex metacharacters
    # (e.g. `{id}` if a route is mounted at `/api/{id}/...`).
    if printf '%s\n' "$NEW_HURL" | grep -qFx "${HURL_TREE}/${domain}.hurl" \
        || printf '%s\n' "$NEW_HURL" | grep -qF "${HURL_TREE}/${domain}/"; then
        echo "route-coverage ok: $route (new hurl added in this diff for domain ${domain})"
        continue
    fi

    # (c) route literal in exclusion ledger.
    # Exact-equal match, not substring: prevents `/api/users` from being
    # falsely "covered" by a ledger entry that mentions `/api/users/roles`.
    # We extract every `/api/...`-shaped token from the ledger and require
    # one to equal the route under check.
    LEDGER_ROUTES=$(printf '%s\n' "$LEDGER" | grep -oE '/api/[A-Za-z0-9_/{}+-]+' || true)
    if printf '%s\n' "$LEDGER_ROUTES" | grep -qFx "$route"; then
        echo "route-coverage ok: $route (in exclusion ledger ${LEDGER_FILE})"
        continue
    fi

    cat <<EOF >&2
::error::route-coverage violation: ${route}

This PR introduces a new ${route} mount but no sibling \`.hurl\` smoke
file. mokumo CLAUDE.md mandates per-endpoint hurl coverage. Add one of:

  - ${HURL_TREE}/${domain}/<name>.hurl proving the wire shape, OR
  - an entry in ${LEDGER_FILE} explaining why this route can't be
    smoke-tested with thin assertions (e.g. WebSocket, restarts the
    server, debug-only).

See standards/testing/hurl-conventions.md (ops repo) and the existing
${HURL_TREE}/ tree for the file pattern.
EOF
    FAIL=1
done <<< "$NEW_ROUTES"

if [[ "$FAIL" -ne 0 ]]; then
    exit 1
fi

echo "route-coverage ok: all new /api/ routes have hurl or ledger coverage"
