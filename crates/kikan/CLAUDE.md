# kikan — boundary enforcement

`kikan` is the platform crate. It must remain garment-domain-agnostic and
adapter-shell-agnostic. CI enforces the boundary on every PR via
`scripts/check-i*.sh`; the same checks pass locally with `bash scripts/check-iN-*.sh`.

What is forbidden inside this crate (see `adr-workspace-split-kikan`):

- **I1 Domain purity** — no garment-vertical identifiers (`customer`, `garment`, `quote`, `invoice`, `print_job`, etc.) in `src/` or `Cargo.toml`. Garment language belongs in `mokumo-garment`.
- **I2 Adapter boundary** — no `tauri::` paths, no `#[tauri::command]` attributes. Tauri lives in `kikan-tauri`.
- **I4 DAG direction** — no dependency on `mokumo-garment`, `mokumo-server`, `mokumo-desktop`, `kikan-tauri`, `kikan-socket`, or `kikan-cli`. Dependencies flow toward kikan, never away from it.
- **I5 Feature gates** — no Cargo feature may pull a Tauri-tagged crate.

If you need to add code that triggers any of these checks, the answer is almost
always to put it in a different crate. If it's truly the boundary that needs to
move, update the ADR first, then the checks, then the code.
