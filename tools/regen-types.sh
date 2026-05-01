#!/usr/bin/env bash
# Regenerate the scorecard JSON Schema and the renderer's `types.d.ts`.
#
# Two sequential steps:
#   1. `cargo run -p scorecard --bin emit-schema` writes the canonical
#      `.config/scorecard/schema.json` from the Rust types via schemars.
#      The committed schema is the renderer's wire contract.
#   2. `json-schema-to-typescript` projects that schema into
#      `.github/scripts/scorecard/types.d.ts` so the plain-JS renderer can
#      JSDoc-reference the same shapes the producer emits.
#
# CI runs the same two commands (`scorecard-drift` job in `quality.yml`)
# and fails on any uncommitted diff, so contributors who change the Rust
# scorecard types must regen + commit both files. See the §Renderer types
# section in `crates/scorecard/README.md`.
#
# Usage:
#   tools/regen-types.sh
set -euo pipefail

JSTS_VERSION="15.0.4"  # pin exact, never `^15`
SCHEMA_OUT=".config/scorecard/schema.json"
TYPES_OUT=".github/scripts/scorecard/types.d.ts"

# Resolve to repo root regardless of where the user invoked the script.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

echo "Regenerating ${SCHEMA_OUT} from Rust source..."
cargo run --quiet -p scorecard --bin emit-schema -- --out "${SCHEMA_OUT}"

echo "Regenerating ${TYPES_OUT} via json-schema-to-typescript@${JSTS_VERSION}..."
pnpm dlx "json-schema-to-typescript@${JSTS_VERSION}" "${SCHEMA_OUT}" --output "${TYPES_OUT}"

echo "Wrote:"
echo "  ${SCHEMA_OUT}"
echo "  ${TYPES_OUT}"
