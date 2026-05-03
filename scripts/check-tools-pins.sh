#!/usr/bin/env bash
#
# Asserts every cargo bin governed by `tools.toml` is pinned to the matching
# version on every `bins:` line in `.github/workflows/*.yml`. Tools not in
# `tools.toml` (e.g. `tauri-driver`) pass through.
#
# Failure modes:
#   - workflow names a governed tool without a version (`bins: cargo-nextest`)
#   - workflow names a governed tool with the wrong version
#   - `tools.toml` pins a tool that no workflow references
#
# Pairs with the `tools-pins` CI job and the `tools-pins` pre-push hook in
# `lefthook.yml`. See `tools.toml` and AGENTS.md §"Dep-graph and verdict
# assertions" for the convention.
set -euo pipefail

cd "$(dirname "$0")/.."

if [ ! -f tools.toml ]; then
  echo "::error::tools.toml not found at repo root"
  exit 1
fi

# Parse `tools.toml` -> `pinned[<name>] = <version>` (flat-section TOML only).
declare -A pinned
section=""
while IFS= read -r line; do
  if [[ "$line" =~ ^\[tools\.([a-zA-Z0-9_-]+)\]$ ]]; then
    section="${BASH_REMATCH[1]}"
  elif [[ "$line" =~ ^version[[:space:]]*=[[:space:]]*\"([^\"]+)\"[[:space:]]*$ ]] && [ -n "$section" ]; then
    pinned[$section]="${BASH_REMATCH[1]}"
    section=""
  elif [[ "$line" =~ ^\[ ]]; then
    section=""
  fi
done < tools.toml

if [ "${#pinned[@]}" -eq 0 ]; then
  echo "::error::tools.toml parsed but contains no [tools.*] entries"
  exit 1
fi

errors=()
seen_in_workflow=()

# Scan every `bins:` line. The `(@version)?` group captures the optional pin.
shopt -s nullglob
workflow_files=(.github/workflows/*.yml .github/workflows/*.yaml)
shopt -u nullglob
if [ "${#workflow_files[@]}" -eq 0 ]; then
  echo "::error::no workflow files found under .github/workflows/"
  exit 1
fi

while IFS= read -r match; do
  file="${match%%:*}"
  rest="${match#*:}"
  lineno="${rest%%:*}"
  spec_raw="${rest#*:}"
  # Strip everything up to and including `bins:`, then trim whitespace and quotes.
  spec="${spec_raw#*bins:}"
  spec="${spec// /}"
  spec="${spec//\"/}"
  spec="${spec//\'/}"
  IFS=',' read -ra entries <<< "$spec"
  for entry in "${entries[@]}"; do
    [ -z "$entry" ] && continue
    if [[ "$entry" == *"@"* ]]; then
      name="${entry%@*}"
      version="${entry#*@}"
    else
      name="$entry"
      version=""
    fi
    if [ -n "${pinned[$name]+set}" ]; then
      seen_in_workflow+=("$name")
      pin="${pinned[$name]}"
      if [ -z "$version" ]; then
        errors+=("$file:$lineno: governed tool '$name' missing pin (expected '$name@$pin')")
      elif [ "$version" != "$pin" ]; then
        errors+=("$file:$lineno: '$name@$version' disagrees with tools.toml ('$name@$pin')")
      fi
    fi
  done
done < <(grep -nE '^[[:space:]]+bins:' "${workflow_files[@]}" 2>/dev/null || true)

# Reverse direction: every governed tool must be referenced somewhere.
for tool in "${!pinned[@]}"; do
  found=0
  for seen in "${seen_in_workflow[@]:-}"; do
    if [ "$seen" = "$tool" ]; then
      found=1
      break
    fi
  done
  if [ "$found" -eq 0 ]; then
    errors+=("tools.toml: '$tool' pinned at @${pinned[$tool]} but referenced by no workflow")
  fi
done

if [ "${#errors[@]}" -gt 0 ]; then
  echo "::error::tools.toml drift detected:" >&2
  for e in "${errors[@]}"; do echo "  - $e" >&2; done
  exit 1
fi

count="${#pinned[@]}"
echo "tools-pins ok: $count governed tool(s) consistent with workflow bins"
