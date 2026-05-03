#!/usr/bin/env bash
# Guard: paired-files Synchronized-Docs rule (AGENTS.md §B rules 2 + 3).
#
# Every PR that adds public crate surface (`pub fn`, `pub struct`, `pub trait`,
# `pub enum`, `pub mod`, `pub static`, `pub const`, `pub type`, `pub use`, plus
# the modifier-prefixed forms `pub async`, `pub unsafe`, `pub extern`) under the
# listed source paths must also touch the matching prose glossary in the same
# PR. The wider-scope path map (mokumo-shop + 9 kikan satellites) tracks
# AGENTS.md §B verbatim.
#
# Path → required doc:
#   crates/mokumo-shop/src/**            → LANGUAGE.md            (vertical)
#   crates/kikan/src/**                  → crates/kikan/LANGUAGE.md (platform)
#   crates/kikan-{events,mail,scheduler,
#                  socket,spa-sveltekit,
#                  tauri,cli,types}/src/** → crates/kikan/LANGUAGE.md
#
# Out of scope (deferred to mokumo#781):
#   - Rule 1 (trust-boundary code → SECURITY.md): semantic, no diff signal
#   - Rule 4 (architectural change → CONTEXT.md + ARCHITECTURE.md): ditto
#
# Detection: syntax-walk over `+`-prefixed diff lines. `pub(crate)`, `pub(super)`,
# and `pub(in …)` are excluded by the regex anchor — they're not crate surface.
# `cargo public-api` semantic diff was considered (mokumo#776 discovery) and
# rejected: it requires a nightly toolchain on every Rust-touching PR and only
# resolves the "rename-with-re-export" case, which the opt-out label already
# covers.
#
# Opt-out: PR_LABELS env var (space- or comma-separated). The label
# `docs-not-applicable` skips the gate — for genuinely internal `pub` items
# kept public for module-graph reasons. The label is part of the contract and
# its presence MUST be visible in the failure message so authors can find it.
#
# Diff base: BASE_REF (default origin/main). Local hooks tolerate a stale ref;
# CI sets STRICT_BASE_REF=1 to fail loudly.
#
# Self-test injection points:
#   DIFF_OVERRIDE  — file containing pre-recorded `git diff` output
#   NAME_OVERRIDE  — file containing pre-recorded `git diff --name-only`
#   PR_LABELS      — env var read directly; fixtures set it inline

set -euo pipefail

HERE="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${HERE}/.." && pwd)"
cd "$ROOT"

BASE_REF="${BASE_REF:-origin/main}"
PR_LABELS="${PR_LABELS:-}"
DIFF_OVERRIDE="${DIFF_OVERRIDE:-}"
NAME_OVERRIDE="${NAME_OVERRIDE:-}"

# === Opt-out short-circuit ================================================
# Must come BEFORE diff acquisition so the gate is a true no-op when the
# author has accepted the rule's escape hatch.

if printf '%s' "$PR_LABELS" | tr ' ,' '\n' | grep -Fxq 'docs-not-applicable'; then
    echo "docs-paired-files ok: docs-not-applicable label set; gate skipped"
    exit 0
fi

# === Path → required-doc map ==============================================

SHOP_DOC="LANGUAGE.md"
KIKAN_DOC="crates/kikan/LANGUAGE.md"

# In-scope diff paths (kept as a single argv array so the git invocation
# matches the awk path-classifier byte-for-byte).
SCOPE_PATHS=(
    'crates/mokumo-shop/src/'
    'crates/kikan/src/'
    'crates/kikan-events/src/'
    'crates/kikan-mail/src/'
    'crates/kikan-scheduler/src/'
    'crates/kikan-socket/src/'
    'crates/kikan-spa-sveltekit/src/'
    'crates/kikan-tauri/src/'
    'crates/kikan-cli/src/'
    'crates/kikan-types/src/'
)

# === Diff acquisition =====================================================
# Stream the diff straight into awk rather than slurping it into a bash var —
# very large PRs would otherwise risk the bash variable copy becoming the
# memory bottleneck.

if [[ -n "$DIFF_OVERRIDE" ]]; then
    DIFF_CMD=(cat -- "$DIFF_OVERRIDE")
else
    if ! git rev-parse --verify --quiet "$BASE_REF" >/dev/null; then
        if [[ "${STRICT_BASE_REF:-0}" == "1" ]]; then
            echo "::error::docs-paired-files: base ref '$BASE_REF' not found (STRICT_BASE_REF=1)" >&2
            exit 2
        fi
        echo "::warning::docs-paired-files: base ref '$BASE_REF' not found locally; skipping (run 'git fetch origin main' to enable)" >&2
        exit 0
    fi
    DIFF_CMD=(git diff "${BASE_REF}...HEAD" -- "${SCOPE_PATHS[@]}")
fi

# === Phase A: extract added pub items, group by required doc =============
# Output: tab-separated "doc-path<TAB>file<TAB>line".

ADDED_BY_DOC=$("${DIFF_CMD[@]}" | awk -v shop="$SHOP_DOC" -v kikan="$KIKAN_DOC" '
    /^diff --git / { file=""; doc=""; in_scope=0 }
    /^\+\+\+ b\// {
        file = substr($0, 7)
        in_scope = 0
        if (file ~ /^crates\/mokumo-shop\/src\//) {
            doc = shop; in_scope = 1
        } else if (file ~ /^crates\/(kikan|kikan-events|kikan-mail|kikan-scheduler|kikan-socket|kikan-spa-sveltekit|kikan-tauri|kikan-cli|kikan-types)\/src\//) {
            doc = kikan; in_scope = 1
        }
    }
    in_scope && /^\+[^+]/ {
        if ($0 ~ /^\+[[:space:]]*pub[[:space:]]+(fn|struct|trait|enum|mod|static|const|type|use|async|unsafe|extern)[[:space:]]/) {
            printf("%s\t%s\t%s\n", doc, file, substr($0, 2))
        }
    }
')

if [[ -z "$ADDED_BY_DOC" ]]; then
    echo "docs-paired-files ok: no public-surface additions in scope vs ${BASE_REF}"
    exit 0
fi

# === Phase B: identify changed file names =================================

if [[ -n "$NAME_OVERRIDE" ]]; then
    CHANGED_FILES=$(cat "$NAME_OVERRIDE")
else
    CHANGED_FILES=$(git diff --name-only "${BASE_REF}...HEAD")
fi

doc_was_changed() {
    printf '%s' "$CHANGED_FILES" | grep -Fxq "$1"
}

# === Phase C: per-doc check ===============================================
# Each doc that has at least one paired pub-item must be touched in the diff.

REQUIRED_DOCS=$(printf '%s\n' "$ADDED_BY_DOC" | cut -f1 | sort -u)
FAIL=0

while IFS= read -r doc; do
    [[ -z "$doc" ]] && continue
    if doc_was_changed "$doc"; then
        echo "docs-paired-files ok: ${doc} touched, paired with public-surface changes"
        continue
    fi

    cat <<EOF >&2
::error::docs-paired-files violation: public surface added without paired ${doc} change

This PR introduces new public crate surface (pub fn / struct / trait / …) but
does not modify ${doc}. AGENTS.md §Synchronized-Docs §B requires every new
public surface to land with a matching glossary entry in the same PR.

Added pub items requiring an entry in ${doc}:
EOF
    printf '%s\n' "$ADDED_BY_DOC" \
        | awk -F'\t' -v d="$doc" '$1==d { printf("  - %s: %s\n", $2, $3) }' >&2

    cat <<EOF >&2

To resolve, choose ONE:
  1. Add an entry for each new pub item to ${doc} in this PR.
  2. Mark items as crate-private if they are NOT part of the public API:
     change \`pub fn foo\` to \`pub(crate) fn foo\`.
  3. If the items are genuinely internal pub-for-mod-graph reasons (no
     consumer surface), add the \`docs-not-applicable\` PR label.

See https://github.com/breezy-bays-labs/mokumo/blob/main/AGENTS.md#synchronized-docs
for the full rule and rationale. Semantic rules 1 (trust-boundary) and 4
(architectural change) are not yet enforced — see mokumo#781.
EOF
    FAIL=1
done <<< "$REQUIRED_DOCS"

[[ "$FAIL" -ne 0 ]] && exit 1
exit 0
