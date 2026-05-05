#!/usr/bin/env bash
# Print the date-pinned nightly channel from `rust-nightly-coverage-toolchain.toml`.
#
# Single source of truth for two consumers:
#   - `crates/mokumo-shop/moon.yml` task `coverage-branches` (sets
#     `RUSTUP_TOOLCHAIN` for one invocation of `cargo llvm-cov --branch`).
#   - `.github/workflows/quality.yml` job `coverage-handlers` (passes the
#     channel to `setup-rust@v1` / `rustup toolchain install`).
#
# Centralising the parse in one script means a pin bump is a single-file edit
# (the `.toml`) — no scattered `nightly-YYYY-MM-DD` strings to keep in sync.
#
# Removable in one step when branch coverage stabilizes on rustc stable:
# delete this script and `rust-nightly-coverage-toolchain.toml` together with
# the `coverage-branches` moon task and the `coverage-handlers` CI job.

set -euo pipefail

repo_root="$(git rev-parse --show-toplevel 2>/dev/null || pwd)"
file="${repo_root}/rust-nightly-coverage-toolchain.toml"

if [[ ! -f "${file}" ]]; then
  echo "nightly-coverage-channel: ${file} not found" >&2
  exit 1
fi

# Match `channel = "nightly-YYYY-MM-DD"` under [toolchain]; tolerate single or
# double quotes and surrounding whitespace. Fail loudly on any deviation rather
# than silently emitting an empty string.
channel="$(awk -F"[='\"[:space:]]+" '
  /^\[toolchain\]/ { in_toolchain = 1; next }
  /^\[/             { in_toolchain = 0 }
  in_toolchain && /^channel/ {
    # Field 2 is the value; awk has already split on single/double
    # quotes / `=` / whitespace.
    for (i = 2; i <= NF; i++) {
      if ($i != "") { print $i; exit }
    }
  }
' "${file}")"

if [[ -z "${channel}" ]]; then
  echo "nightly-coverage-channel: no [toolchain] channel = ... entry in ${file}" >&2
  exit 1
fi

printf '%s\n' "${channel}"
