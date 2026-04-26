# kikan — boundary enforcement

`kikan` is the platform crate. It must remain garment-domain-agnostic and
adapter-shell-agnostic. CI enforces the boundary on every PR via
`scripts/check-i*.sh`; the same checks pass locally with `bash scripts/check-iN-*.sh`.

What is forbidden inside this crate (see `adr-workspace-split-kikan` +
`adr-kikan-engine-vocabulary`):

- **I1/classic — Domain purity.** No shop-vertical identifiers (`customer`, `garment`, `quote`, `invoice`, `print_job`, etc.) in `src/` or `Cargo.toml`. Shop-vertical language belongs in `mokumo-shop`.
- **I1/strict — No leaked vertical wire artifacts.** No `SetupMode` variant name, no `"mokumo.db"` filename literal, no `\b(demo|production|Demo|Production)\b` literal strings or identifiers in production code under `src/` (excluding inline `#[cfg(test)]` modules and a small allow-list of files scheduled for vocab cleanup in PR B — see `scripts/check-i1-vocabulary-purity.sh`). The vertical's `ProfileKind` is reached only through the `Graft` trait hooks (`all_profile_kinds`, `default_profile_kind`, `requires_setup_wizard`, `auth_profile_kind`, `db_filename`) and through the `Display` + `FromStr` bounds on the associated type (which are the single source of truth for on-disk directory names). Kikan owns *capability*; the vertical owns *vocabulary*.
- **I2 Adapter boundary** — no `tauri::` paths, no `#[tauri::command]` attributes. Tauri lives in `kikan-tauri`.
- **I4 DAG direction** — no dependency on `mokumo-shop`, `mokumo-server`, `mokumo-desktop`, `kikan-tauri`, `kikan-socket`, or `kikan-cli`. Dependencies flow toward kikan, never away from it.
- **I5 Feature gates** — no Cargo feature may pull a Tauri-tagged crate.

If you need to add code that triggers any of these checks, the answer is almost
always to put it in a different crate. If it's truly the boundary that needs to
move, update the ADR first, then the checks, then the code.
