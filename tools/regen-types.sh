#!/usr/bin/env bash
# Regenerate the scorecard JSON Schemas and the renderer's `types.d.ts`.
#
# Three sequential steps:
#   1. `cargo run -p scorecard --bin emit-schema` writes BOTH committed
#      schemas from the Rust types via schemars:
#        - `.config/scorecard/schema.json` — wire contract for the
#          renderer.
#        - `.config/scorecard/quality.config.schema.json` — operator
#          contract for `quality.toml` (validated by ajv on the
#          `scorecard-drift` CI gate).
#      Default `--target both` means the binary needs no flags here.
#   2. `pnpm regen-types` (in `.github/scripts/scorecard/`) projects the
#      wire schema into `.github/scripts/scorecard/types.d.ts` so the
#      plain-JS renderer can JSDoc-reference the same shapes the
#      producer emits. The `json-schema-to-typescript` version is
#      pinned in that package's `package.json` — the single source of
#      truth.
#
# CI runs the same commands (`scorecard-drift` job in `quality.yml`)
# and fails on any uncommitted diff, so contributors who change the
# Rust scorecard types must regen + commit all three files. See the
# §Renderer types section in `crates/scorecard/README.md`.
#
# Usage:
#   tools/regen-types.sh
set -euo pipefail

WIRE_SCHEMA_OUT=".config/scorecard/schema.json"
QUALITY_SCHEMA_OUT=".config/scorecard/quality.config.schema.json"
TYPES_OUT=".github/scripts/scorecard/types.d.ts"

# Resolve to repo root regardless of where the user invoked the script.
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

echo "Regenerating wire + operator schemas from Rust source..."
cargo run --quiet -p scorecard --bin emit-schema

echo "Installing renderer dependencies (frozen lockfile)..."
pnpm install --filter @mokumo/scorecard-renderer... --frozen-lockfile

echo "Regenerating ${TYPES_OUT}..."
pnpm --filter @mokumo/scorecard-renderer regen-types

echo "Wrote:"
echo "  ${WIRE_SCHEMA_OUT}"
echo "  ${QUALITY_SCHEMA_OUT}"
echo "  ${TYPES_OUT}"
