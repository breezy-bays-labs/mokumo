#!/usr/bin/env bash
# Reject hard-coded "/admin/..." paths in admin-UI source.
#
# Internal links must prepend the SvelteKit base path (`$app/paths`) so
# the bundle works whether served at /admin (composed mount) or /
# (standalone preview). The CompositeSpaSource strips the /admin prefix
# at the server boundary; in-app links must NOT bake it in.

set -euo pipefail

cd "$(dirname "$0")/.."

ROOT="src"
fail=0

# Find string literals that hard-code the /admin prefix.
# Allowed: $app/paths usage, comments, and import.meta.env-style references.
if grep -RnE "['\"\`]/admin(/|['\"\`])" "$ROOT" \
     --include='*.svelte' --include='*.ts' --include='*.svelte.ts' \
     | grep -vE '^[[:space:]]*//' \
     | grep -vE '^[[:space:]]*\*' \
     | grep -vE 'import .* from' ; then
  echo "❌ check-app-paths-base: hard-coded '/admin/...' literal in source."
  echo "   Use \$app/paths (import { base } from '\$app/paths'; href={\`\${base}/foo\`}) instead."
  fail=1
fi

if [[ $fail -eq 0 ]]; then
  echo "✓ app-paths-base gate: clean."
fi

exit $fail
