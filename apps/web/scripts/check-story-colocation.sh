#!/usr/bin/env bash
# Enforces story colocation: every component directory under src/lib/components/
# must have at least one .stories.svelte file. Runs as a lefthook pre-commit hook.

set -euo pipefail

missing=()

# Get staged .svelte files under the components directory, skip story files
while IFS= read -r file; do
  [[ -z "$file" ]] && continue
  [[ "$file" == *.stories.svelte ]] && continue

  # Check if the directory has at least one story file
  dir=$(dirname "$file")
  if ! ls "${dir}"/*.stories.svelte &>/dev/null; then
    # Only report each directory once
    if [[ ! " ${missing[*]+"${missing[*]}"} " =~ " ${dir} " ]]; then
      missing+=("$dir")
    fi
  fi
done < <(git diff --cached --name-only --diff-filter=ACM -- 'apps/web/src/lib/components/**/*.svelte')

if [[ ${#missing[@]} -gt 0 ]]; then
  echo "Story co-location check failed:"
  echo ""
  for dir in "${missing[@]}"; do
    echo "  No .stories.svelte file in ${dir}/"
    echo ""
  done
  exit 1
fi
