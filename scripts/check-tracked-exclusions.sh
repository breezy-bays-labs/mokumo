#!/usr/bin/env bash
# Enforces the `tracked:` / `adr:` annotation contract from
# ~/.claude/rules/exclusions.md across the four exclusion mechanisms with
# low false-positive risk:
#
#   1. Rust  `#[ignore]` / `#[ignore = "..."]`
#   2. Rust  `#[cfg(skip_in_ci)]` and similar `#[cfg(.*skip.*)]` gates
#   3. JS/TS `it.skip(...)` / `describe.skip(...)` / `test.skip(...)` /
#            `xtest(...)` / `xit(...)`
#   4. Config-file `exclude` / `excluded_files` / `skip` arrays in
#      .toml/.yml/.yaml/.cjs/.js/.ts files
#
# A site passes if its line OR the two lines above it contain either
#   - `tracked: <repo>#<n>`              (deferred work with tracking issue)
#   - `adr: <path>.md`                   (permanent design choice)
#
# Modes:
#   default     — fail closed on any violation (exit 1, list sites)
#   --soft      — print violations and exit 0 (Option B1 landing mode)
#
# The regex floor is intentionally narrower than the full eight-mechanism
# rule. Higher-FP mechanisms (workflow `if:` conditions, commented-out
# test bodies, TODO/FIXME markers, `--exclude <crate>` in CI cargo
# invocations) are deferred to a T2 AST-aware audit; tools-pins handles
# `--exclude` already (see scripts/check-tools-pins.sh).

set -euo pipefail

soft=0
case "${1:-}" in
  --soft) soft=1 ;;
  "")     ;;
  *)      printf 'Usage: %s [--soft]\n' "$0" >&2; exit 2 ;;
esac

cd "$(git rev-parse --show-toplevel)"

ANNOTATION_RE='(tracked:[[:space:]]+[A-Za-z0-9_/-]+#[0-9]+|adr:[[:space:]]+[^[:space:]]+\.md)'

violations=()

# Append a violation if neither the candidate line nor the two preceding
# lines contain a tracking annotation.
check_site() {
  local file="$1" lineno="$2" mechanism="$3"
  local start=$(( lineno - 2 ))
  (( start < 1 )) && start=1
  if sed -n "${start},${lineno}p" "$file" | grep -qE "$ANNOTATION_RE"; then
    return 0
  fi
  local content
  content=$(sed -n "${lineno}p" "$file")
  # Trim leading whitespace for readable output.
  content="${content#"${content%%[![:space:]]*}"}"
  violations+=("$file:$lineno: $mechanism — ${content}")
}

# Walk a stream of `<file>:<lineno>:<content>` candidates and feed each
# `(file, lineno)` pair through check_site. The caller passes the
# producer command via a process substitution so `violations+=` executes
# in the parent shell (a `cmd | while …` pipe forks a subshell whose
# array mutations are lost).
walk_grep_output() {
  local mechanism="$1"
  local file lineno
  while IFS=: read -r file lineno _; do
    [[ -z "$file" ]] && continue
    check_site "$file" "$lineno" "$mechanism"
  done
}

# 1. Rust #[ignore] / #[ignore = "..."]
walk_grep_output 'Rust #[ignore]' \
  < <(git grep -nE '#\[ignore(\]|[[:space:]]*=)' -- '*.rs' 2>/dev/null || true)

# 2. Rust #[cfg(...skip...)] gates
walk_grep_output 'Rust #[cfg(skip_*)]' \
  < <(git grep -nE '#\[cfg\([^)]*skip[^)]*\)\]' -- '*.rs' 2>/dev/null || true)

# 3. JS/TS test-runner skip APIs
walk_grep_output 'JS/TS test skip' \
  < <(git grep -nE '\b(it|describe|test)\.skip\(|\bxtest\(|\bxit\(' \
       -- '*.ts' '*.tsx' '*.js' '*.cjs' '*.svelte' 2>/dev/null || true)

# 4. Config-file exclude/skip arrays.
#
#   - Multi-line arrays: every quoted entry inside the array is a candidate.
#   - Single-line arrays: the opener line is the candidate.
#
# We do NOT recurse into the array structure — annotations may sit above
# the array opener (covering the whole block) or above each entry. The
# 2-line-context check handles both placements.
collect_config_candidates() {
  local config_globs=( '*.toml' '*.yml' '*.yaml' '*.cjs' '*.js' '*.ts' '*.tsx' )
  local file
  while IFS= read -r file; do
    [[ -z "$file" ]] && continue
    awk -v F="$file" '
      # Single-line array: opener and at least one quoted entry on same line.
      /^[[:space:]]*(exclude|excluded_files|skip)[[:space:]]*[=:][[:space:]]*\[[^]]*"[^"]+"[^]]*\]/ {
        print F ":" NR ":" $0
        next
      }
      # Multi-line opener: enter array tracking state.
      /^[[:space:]]*(exclude|excluded_files|skip)[[:space:]]*[=:][[:space:]]*\[[[:space:]]*$/ {
        in_excl=1; next
      }
      # Closing bracket on its own line ends array tracking.
      in_excl && /^[[:space:]]*\][[:space:]]*,?[[:space:]]*$/ { in_excl=0; next }
      # Entry line inside a tracked array: quoted string with optional comma + comment.
      in_excl && /^[[:space:]]*("[^"]+"|'\''[^'\'']+'\'')[[:space:]]*,?[[:space:]]*(#.*|\/\/.*)?$/ {
        print F ":" NR ":" $0
      }
    ' "$file"
  done < <(git ls-files -- "${config_globs[@]}")
}

walk_grep_output 'Config exclude/skip array' < <(collect_config_candidates)

if (( ${#violations[@]} == 0 )); then
  echo "check-tracked-exclusions: no untracked exclusions found."
  exit 0
fi

printf "Found %d exclusion site(s) without \`tracked:\` or \`adr:\` annotation:\n\n" "${#violations[@]}"
printf '  %s\n' "${violations[@]}"
printf "\nAdd \`tracked: <repo>#<n> — <reason>\` for deferred work or\n"
printf "\`adr: <path>.md\` for permanent design decisions, on the same line\n"
printf "or within the two lines above. See ~/.claude/rules/exclusions.md.\n"

if (( soft == 1 )); then
  printf '\nRunning in --soft mode; not failing.\n'
  exit 0
fi
exit 1
