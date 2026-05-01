#!/usr/bin/env bash
# Regenerate the vendored ajv bundle used by .github/scripts/scorecard/.
#
# The renderer in `.github/workflows/scorecard-comment.yml` validates the
# scorecard artifact via ajv. Vendoring the bundled `ajv.js` (esbuild
# `--platform=node --format=cjs`) keeps the renderer step network-free at
# CI runtime and pins the validator to an exact version reviewed at
# regen-time. Quarterly cadence per ADR `decisions/mokumo/adr-scorecard-crate-shape.md`.
#
# Usage:
#   tools/update-vendored-ajv.sh
#
# Outputs (committed verbatim):
#   .github/scripts/scorecard/ajv-bundle.js  # CJS, single file, ~80 KB
#   .github/scripts/scorecard/.VERSIONS      # ajv@VERSION + bundle date + bundler tag
set -euo pipefail

AJV_VERSION="8.16.0"  # pin exact, never `^8`
OUT_DIR=".github/scripts/scorecard"
OUT_BUNDLE="${OUT_DIR}/ajv-bundle.js"
OUT_VERSIONS="${OUT_DIR}/.VERSIONS"

# Resolve to repo root regardless of where the user invoked the script
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${REPO_ROOT}"

mkdir -p "${OUT_DIR}"

tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT

echo "Installing ajv@${AJV_VERSION} into ${tmp}..."
pnpm --prefix "${tmp}" add "ajv@${AJV_VERSION}"

echo "Bundling with esbuild (cjs, node)..."
# `--banner:js='// @ts-nocheck'` keeps `tsc --noEmit` (used by the
# `scorecard-drift` job to type-check the JSDoc-annotated renderer JS)
# from descending into the vendored bundle. The bundle is generated
# code we don't author, so type-checking it is noise.
pnpm dlx esbuild "${tmp}/node_modules/ajv/dist/ajv.js" \
  --bundle --platform=node --format=cjs \
  --banner:js='// @ts-nocheck' \
  --outfile="${OUT_BUNDLE}"

{
  echo "ajv@${AJV_VERSION}"
  echo "bundled-at: $(date -u +%Y-%m-%dT%H:%M:%SZ)"
  echo "bundler: esbuild (cjs, node)"
} > "${OUT_VERSIONS}"

echo "Wrote:"
echo "  ${OUT_BUNDLE}"
echo "  ${OUT_VERSIONS}"
