#!/usr/bin/env bash
# Reject Svelte 4 patterns in admin-UI source.
#
# Per /workspace/CLAUDE.md "Svelte 5 runes only" — `$state`, `$derived`,
# `$effect`, `$props`. Never Svelte 4 stores or `export let`.

set -euo pipefail

cd "$(dirname "$0")/.."

ROOT="src"
fail=0

# `export let` outside type aliases / interfaces — Svelte 4 prop syntax.
if grep -RnE '^\s*export let ' "$ROOT" --include='*.svelte' --include='*.svelte.ts' 2>/dev/null; then
  echo "❌ Svelte 5 purity: 'export let' is Svelte 4 props syntax. Use \$props() instead."
  fail=1
fi

# `$:` reactive blocks — Svelte 4 reactivity.
if grep -RnE '^\s*\$:\s' "$ROOT" --include='*.svelte' 2>/dev/null; then
  echo "❌ Svelte 5 purity: '\$:' is Svelte 4 reactivity. Use \$derived or \$effect instead."
  fail=1
fi

# `on:click` and similar — Svelte 4 event-directive syntax.
if grep -RnE 'on:[a-z]+(\||=)' "$ROOT" --include='*.svelte' 2>/dev/null; then
  echo "❌ Svelte 5 purity: 'on:event' directive is Svelte 4. Use onclick / onkeydown / etc. attributes."
  fail=1
fi

# `svelte/store` import — Svelte 4 stores. Use $state / runes.
if grep -RnE "from\s+['\"]svelte/store['\"]" "$ROOT" --include='*.svelte' --include='*.ts' --include='*.svelte.ts' 2>/dev/null; then
  echo "❌ Svelte 5 purity: imports from 'svelte/store' (writable/readable/derived). Use \$state / \$derived runes."
  fail=1
fi

if [[ $fail -eq 0 ]]; then
  echo "✓ Svelte 5 purity gate: clean."
fi

exit $fail
