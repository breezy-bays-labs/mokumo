#!/usr/bin/env bash
# Guard: every newly-introduced API route in the diff requires hurl coverage
# for each HTTP method, OR a matching entry in the moon.yml exclusion ledger.
#
# How it works:
#   - Detects added `.route("...", <method>(...))` calls under
#     `crates/**/src/**/*.rs`. Multi-line `.route(\n  "<path>",\n  ...,\n)`
#     blocks are recognised by joining consecutive `+`-prefixed diff lines
#     per file before applying the route-extraction regex.
#   - Resolves relative paths in sub-routers: a `.route("/{id}/restore", ...)`
#     added inside a function whose name appears as the second arg of a
#     `.nest("/api/<prefix>", <fn>())` call in `routes.rs` is treated as
#     `/api/<prefix>/{id}/restore`.
#   - Per-method coverage: each HTTP method on a route (`get(h).post(h)`
#     yields two endpoints) is checked independently. Coverage matches when
#     a hurl file has a request line `<METHOD> http(s)?://<host><path>`
#     with `{id}`-style segments treated as `[^/]+`.
#
# Background: mokumo CLAUDE.md mandates per-endpoint hurl coverage:
#   "New API endpoints require a `.hurl` file — add
#   `tests/api/<domain>/<endpoint>.hurl` in the same PR."
#
# Diff base: `origin/main`. Local hooks tolerate a stale base ref because
# CI is the source of truth (CI sets STRICT_BASE_REF=1 to fail loudly on
# missing ref).
#
# Known residual gaps (out of scope for this guard):
#   - Mounting a pre-existing router under a new `.nest("/api/X", ...)`
#     prefix without inline `.route(...)` changes is not flagged.
#   - Routes added via `MethodRouter::new().on(MethodFilter::*, ...)` form
#     (axum supports it; mokumo does not currently use it) are not detected.
#   - Cosmetic re-emissions of an unchanged `.route(...)` line are treated
#     as new endpoints; harmless when the existing hurl is unchanged.
#   - Adding a method to a multi-line `.route(\n  "<path>",\n  <chain>,\n)`
#     block where the path line is unchanged: the `--unified=0` diff buffer
#     captures only the chain delta, so the route's path can't be resolved
#     and the new method is missed. Single-line chain edits (where path +
#     chain share a line) and whole-route additions ARE caught. Closing
#     this case requires post-image vs pre-image parsing (mokumo#733).

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${HERE}/.." && pwd)"
cd "$ROOT"

BASE_REF="${BASE_REF:-origin/main}"
LEDGER_FILE="${LEDGER_FILE:-crates/mokumo-shop/moon.yml}"
HURL_TREE="${HURL_TREE:-tests/api}"
ROUTES_FILES="${ROUTES_FILES:-crates/mokumo-shop/src/routes.rs}"
ROUTER_FN_SEARCH_DIRS="${ROUTER_FN_SEARCH_DIRS:-crates}"

# Self-test injection points. DIFF_OVERRIDE replaces git diff output;
# ROUTER_FN_OVERRIDE replaces the full grep across crates so fixtures can pin
# a fixed set of fn → file mappings.
DIFF_OVERRIDE="${DIFF_OVERRIDE:-}"
ROUTER_FN_OVERRIDE="${ROUTER_FN_OVERRIDE:-}"

# === Diff acquisition =====================================================

if [[ -n "$DIFF_OVERRIDE" ]]; then
    DIFF=$(cat "$DIFF_OVERRIDE")
else
    if ! git rev-parse --verify --quiet "$BASE_REF" >/dev/null; then
        if [[ "${STRICT_BASE_REF:-0}" == "1" ]]; then
            echo "::error::route-coverage: base ref '$BASE_REF' not found (STRICT_BASE_REF=1)" >&2
            exit 2
        fi
        echo "::warning::route-coverage: base ref '$BASE_REF' not found locally; skipping (run \`git fetch origin main\` to enable)" >&2
        exit 0
    fi
    # Allow git diff to surface real failures (corrupt index, lock contention)
    # — only the missing-ref case is silenced above.
    DIFF=$(git diff --unified=0 "${BASE_REF}...HEAD" -- 'crates/**/src/**/*.rs')
fi

if [[ -z "$DIFF" ]]; then
    echo "route-coverage ok: no Rust source changes vs ${BASE_REF}"
    exit 0
fi

# === Phase A: file → /api/<prefix> map ====================================
# Walk routes.rs (or the configured ROUTES_FILES) for `.nest("/api/<prefix>",
# ... ::<fn>())` patterns to map function names to mount prefixes, then join
# with the workspace router-fn directory to get a single file → prefix map.
# Multi-line .nest() chains are handled by collapsing whitespace before
# matching.

build_fn_to_prefix_map() {
    local f
    for f in $ROUTES_FILES; do
        [[ -f "$f" ]] || continue
        tr '\n' ' ' < "$f" | sed -E 's/[[:space:]]+/ /g' \
            | sed 's|\.nest(|\n.nest(|g' \
            | sed -nE 's|^\.nest\( *"(/api/[^"]+)", *([a-zA-Z_][a-zA-Z0-9_]*::)*([a-zA-Z_][a-zA-Z0-9_]*)\(\).*$|\3 \1|p'
    done | sort -u
}

build_fn_to_file_map() {
    if [[ -n "$ROUTER_FN_OVERRIDE" ]]; then
        cat "$ROUTER_FN_OVERRIDE"
        return
    fi
    local search_dirs
    read -r -a search_dirs <<< "$ROUTER_FN_SEARCH_DIRS"
    grep -rE '^[[:space:]]*pub fn [a-zA-Z_][a-zA-Z0-9_]*\(.*\)[^{]+->[[:space:]]*Router' \
        --include='*.rs' \
        "${search_dirs[@]}" \
        | sed -nE 's|^([^:]+):[[:space:]]*pub fn ([a-zA-Z_][a-zA-Z0-9_]*)\(.*$|\2 \1|p' \
        | sort -u
}

# Emits "<file_path> <prefix>" lines by joining the two intermediate maps.
# Empty output when no nest mappings are found — that's fine; relative
# routes simply won't resolve and will be skipped (see Phase C).
build_file_to_prefix_map() {
    local fn_to_prefix
    fn_to_prefix=$(build_fn_to_prefix_map || true)
    local fn_to_file
    fn_to_file=$(build_fn_to_file_map || true)
    [[ -z "$fn_to_prefix" || -z "$fn_to_file" ]] && return 0
    awk 'NR==FNR { prefix[$1]=$2; next } $1 in prefix { print $2, prefix[$1] }' \
        <(printf '%s\n' "$fn_to_prefix") \
        <(printf '%s\n' "$fn_to_file") \
        | sort -u
}

FILE_TO_PREFIX_MAP=$(build_file_to_prefix_map)

# Look up the /api/<prefix> a router fn defined in `<file>` is mounted under.
# Echoes the prefix on hit (returns 0); returns 1 on miss.
resolve_prefix_for_file() {
    local file="$1"
    [[ -z "$file" || -z "$FILE_TO_PREFIX_MAP" ]] && return 1
    awk -v file="$file" '$1==file { print $2; found=1; exit } END { exit found?0:1 }' \
        <<< "$FILE_TO_PREFIX_MAP"
}

# === Phase B: walk diff for added .route(...) calls =======================
# Per-file buffer accumulation handles multi-line route blocks. Inside a
# buffer we split at each `.route(` so each segment carries one route's
# path + chained methods.

# Sentinel = ASCII Unit Separator (0x1F). Won't appear in Rust source.
SENTINEL=$'\x1f'

emit_endpoints_for_buffer() {
    local file="$1"
    local buffer="$2"
    [[ -z "$buffer" ]] && return 0

    # Split at each `.route(` so each line (after the first, which is
    # pre-`.route(` junk) starts with `.route(<args>...`.
    local split
    split=$(printf '%s' "$buffer" | sed "s|\\.route(|${SENTINEL}.route(|g" | tr "$SENTINEL" '\n')

    local seg
    while IFS= read -r seg; do
        # Match `.route( "<path>" , <body…>` — body runs until the next
        # `.route(` (i.e. end of segment) and contains the method chain.
        if [[ "$seg" =~ \.route\([[:space:]]*\"([^\"]+)\"[[:space:]]*,(.*) ]]; then
            local path="${BASH_REMATCH[1]}"
            local body="${BASH_REMATCH[2]}"

            local methods
            methods=$(printf '%s' "$body" \
                | grep -oE '\b(get|post|put|patch|delete|head|options)\(' \
                | sed 's/(//' \
                | sort -u)
            [[ -z "$methods" ]] && continue

            local full_path=""
            if [[ "$path" == /api/* ]]; then
                full_path="$path"
            else
                local prefix
                if prefix=$(resolve_prefix_for_file "$file"); then
                    if [[ "$path" == "/" ]]; then
                        full_path="$prefix"
                    else
                        full_path="${prefix}${path}"
                    fi
                else
                    # Out of scope: relative path with no resolvable parent
                    # mount. Skip — the route may be intentionally mounted
                    # at a non-`/api/` path, or the mounting site is added
                    # in the same PR and lives outside ROUTES_FILES.
                    continue
                fi
            fi

            local m
            for m in $methods; do
                printf '%s %s\n' \
                    "$(printf '%s' "$m" | tr '[:lower:]' '[:upper:]')" \
                    "$full_path"
            done
        fi
    done <<< "$split"
}

extract_added_routes() {
    local current_file=""
    local buffer=""
    local line
    while IFS= read -r line; do
        if [[ "$line" =~ ^\+\+\+\ b/(.+)$ ]]; then
            # Capture the regex group BEFORE calling helpers — bash may
            # reset BASH_REMATCH inside the call, and `set -u` then errors
            # on `${BASH_REMATCH[1]}` if the helper runs a non-matching
            # regex or no regex at all.
            local next_file="${BASH_REMATCH[1]}"
            emit_endpoints_for_buffer "$current_file" "$buffer"
            current_file="$next_file"
            buffer=""
            continue
        fi
        # Accumulate added lines into the current file's buffer (drop the
        # leading `+`). Removed and context lines are ignored.
        if [[ "$line" =~ ^\+[^+] ]]; then
            buffer+="${line:1} "
        fi
    done <<< "$DIFF"
    emit_endpoints_for_buffer "$current_file" "$buffer"
}

NEW_ENDPOINTS=$(extract_added_routes | sort -u)

if [[ -z "$NEW_ENDPOINTS" ]]; then
    echo "route-coverage ok: no new (method, /api/...) endpoints introduced in diff"
    exit 0
fi

# === Phase C: coverage check per (METHOD, PATH) ===========================

if [[ -z "$DIFF_OVERRIDE" ]]; then
    NEW_HURL=$(git diff --name-only --diff-filter=A "${BASE_REF}...HEAD" -- "${HURL_TREE}/**/*.hurl")
else
    NEW_HURL=""
fi

if [[ -f "$LEDGER_FILE" ]]; then
    LEDGER=$(cat "$LEDGER_FILE")
else
    LEDGER=""
fi

# Helper: extract /api/<domain> top-level segment from a path. Pure bash —
# no subshell.
domain_of() {
    local p="${1#/api/}"
    printf '%s' "${p%%/*}"
}

# Convert /api/users/{id}/role → /api/users/[^/]+/role for grep -E.
# Step 1 escapes regex metas in literal segments. The character class places
# `]` first (POSIX-portable trick — a literal `]` at the start of a class
# is part of the class, not its terminator); without that, GNU sed -E
# truncates the class at the first unescaped `]`, silently leaving the
# downstream metas (`.`, `*`, …) unescaped. Step 2 swaps `{<word>}` with
# `[^/]+`; the single-char classes `[{]`/`[}]` dodge ERE's quantifier-
# interval (`\{m,n\}`) parsing under strict EREs.
path_to_regex() {
    local p="$1"
    local escaped
    escaped=$(printf '%s' "$p" | sed -E 's|[].*+?^()[$\\]|\\&|g')
    printf '%s' "$escaped" | sed -E 's|[{][a-zA-Z_][a-zA-Z0-9_]*[}]|[^/]+|g'
}

# Returns 0 if the file contains a request line `<METHOD> http(s)?://<host><path>(?|space|EOL)`.
# `<host>` matches any non-space, non-slash run (`{{host}}` template included).
hurl_has_request() {
    local method="$1"
    local path_regex="$2"
    local hurl_file="$3"
    [[ -f "$hurl_file" ]] || return 1
    grep -qE "^${method}[[:space:]]+https?://[^[:space:]/]+${path_regex}(\\?|[[:space:]]|$)" "$hurl_file"
}

# Try every known coverage source for (method, path). Echoes a one-line
# "covered by …" message on success and returns 0; returns 1 otherwise.
method_covered() {
    local method="$1"
    local path="$2"
    local domain
    domain=$(domain_of "$path")
    local path_regex
    path_regex=$(path_to_regex "$path")

    # (a) any hurl file under <HURL_TREE>/<domain>/ or sibling <domain>.hurl
    local f
    while IFS= read -r f; do
        [[ -z "$f" ]] && continue
        if hurl_has_request "$method" "$path_regex" "$f"; then
            printf 'covered by %s' "$f"
            return 0
        fi
    done < <(
        find "${HURL_TREE}/${domain}" -type f -name '*.hurl' 2>/dev/null
        [[ -f "${HURL_TREE}/${domain}.hurl" ]] && printf '%s\n' "${HURL_TREE}/${domain}.hurl"
    )

    # (b) newly-added hurl files in the same diff
    if [[ -n "$NEW_HURL" ]]; then
        while IFS= read -r f; do
            [[ -z "$f" ]] && continue
            if hurl_has_request "$method" "$path_regex" "$f"; then
                printf 'covered by new file %s' "$f"
                return 0
            fi
        done <<< "$NEW_HURL"
    fi

    # (c) exclusion ledger: a "<METHOD> <path>" or "<METHOD> <path>(<note>)" line.
    # Match against the path with placeholders treated as `[^/]+` so a ledger
    # entry like "DELETE /api/users/{id}" covers the route literally.
    if printf '%s' "$LEDGER" | grep -qE "(^|[[:space:]])${method}[[:space:]]+${path_regex}([[:space:]]|$|\(|,)"; then
        printf 'in exclusion ledger %s' "$LEDGER_FILE"
        return 0
    fi

    return 1
}

FAIL=0
COVERAGE_MSG=""

while IFS=' ' read -r method path; do
    [[ -z "$method" || -z "$path" ]] && continue
    if COVERAGE_MSG=$(method_covered "$method" "$path"); then
        echo "route-coverage ok: ${method} ${path} (${COVERAGE_MSG})"
    else
        domain=$(domain_of "$path")
        cat <<EOF >&2
::error::route-coverage violation: ${method} ${path}

This PR introduces ${method} ${path} but no .hurl file has a matching
request line. mokumo CLAUDE.md mandates per-endpoint hurl coverage. Add:

  - ${HURL_TREE}/${domain}/<verb>-<slug>.hurl with a "${method} http://{{host}}${path}" line, OR
  - an entry in ${LEDGER_FILE} listing "${method} ${path}" with the reason.

See ~/Github/ops/standards/testing/hurl-conventions.md and
~/.claude/skills/hurl-test-author/ (Mokumo deltas) for the file pattern.
EOF
        FAIL=1
    fi
done <<< "$NEW_ENDPOINTS"

if [[ "$FAIL" -ne 0 ]]; then
    exit 1
fi

echo "route-coverage ok: all new (method, /api/...) endpoints have hurl or ledger coverage"
