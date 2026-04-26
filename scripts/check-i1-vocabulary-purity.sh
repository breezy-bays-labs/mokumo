#!/usr/bin/env bash
# I1/strict — Vocabulary purity for kikan source.
#
# kikan is the platform crate; the vertical (Mokumo) owns its profile
# vocabulary (`demo`, `production`, ...). kikan must reach those names
# only through the `Graft` trait + `ProfileDirName` opaque key (per
# adr-kikan-engine-vocabulary).
#
# This script greps `crates/kikan/src/` for the literal strings `demo`,
# `production`, `Demo`, `Production` (whole-word) outside of:
#   - inline `#[cfg(test)]` modules (truncated per-file at first match)
#   - `//`-prefixed comment lines (rustdoc included)
#   - a small allow-list of files with known pre-existing structural
#     leaks scheduled for cleanup in PR B (slug RESERVED_SLUGS, legacy
#     folder name in boot_state, etc.) — see the ALLOWLIST below.
#
# Each allow-list entry SHOULD shrink and eventually disappear. New
# files MUST NOT be added to the allow-list without updating
# `proposed-adr-collapse-mokumo-core-and-vocabulary-neutral-kikan.md`
# (or its promoted form).

set -euo pipefail

TARGET="${1:-crates/kikan/src}"
PATTERN='\b(demo|production|Demo|Production)\b'

# Pre-existing structural leaks scheduled for cleanup. Each entry
# carries a short note pointing at the PR/issue that removes it.
ALLOWLIST=(
    "crates/kikan/src/slug.rs"                       # RESERVED_SLUGS contains literal — split via Graft hook in PR B
    "crates/kikan/src/meta/boot_state.rs"            # legacy folder name + test fixtures — Graft hook in PR B
    "crates/kikan/src/meta/upgrade.rs"               # legacy upgrade test fixtures — fully covered by inline #[cfg(test)] but Path literals leak in same file
    "crates/kikan/src/meta/profiles.rs"              # SQL fixture string in inline test
    "crates/kikan/src/data_plane/kikan_version.rs"   # test fixture profile names in inline tests
    "crates/kikan/src/control_plane/types.rs"        # PinId test fixture
    "crates/kikan/src/db/diagnostics.rs"             # admin@demo.local test fixture
    "crates/kikan/src/tenancy/profile_dir_name.rs"   # ProfileDirName::parse round-trip tests
    "crates/kikan/src/app_error.rs"                  # AppError::DemoSetupRequired variant name (CamelCase, regex-safe; included for inline-test fixtures)
)

is_allowed() {
    local f="$1"
    for entry in "${ALLOWLIST[@]}"; do
        [ "$f" = "$entry" ] && return 0
    done
    return 1
}

violations=0
while IFS= read -r f; do
    if is_allowed "$f"; then
        continue
    fi
    # Truncate at the first `#[cfg(test)]` line so inline test modules
    # don't false-positive. Keep original line numbers via `NR`.
    leaks=$(awk '/^[[:space:]]*#\[cfg\(test\)\]/{exit} {print NR":"$0}' "$f" \
        | grep -E "$PATTERN" \
        | grep -vE '^[0-9]+:[[:space:]]*//' \
        || true)
    if [ -n "$leaks" ]; then
        echo "::error::I1/strict vocab purity violation in $f:" >&2
        echo "$leaks" | sed 's|^|  |' >&2
        violations=$((violations + 1))
    fi
done < <(find "$TARGET" -name '*.rs' -not -name '*_tests.rs' -not -name 'tests.rs' | sort)

if [ "$violations" -gt 0 ]; then
    echo "::error::I1/strict vocab purity: $violations file(s) leak vertical vocabulary into kikan source" >&2
    exit 1
fi

echo "I1/strict vocab purity ok: ${TARGET} (excluding allow-list of ${#ALLOWLIST[@]} pre-existing files) contains no demo/production literals"
