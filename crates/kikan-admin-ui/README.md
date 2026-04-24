# kikan-admin-ui

Sister crate to `kikan-spa-sveltekit`. Holds the rust-embed asset bundle
for the M00 platform admin UI served at `/admin/*` on the composed Tauri
origin.

## Scope

This crate intentionally holds **only** the asset bundle and the
factory that constructs a `SpaSource` for it. All composition logic
(prefix-stripped mounting, dispatch) lives upstream in
`kikan::data_plane::spa::CompositeSpaSource`. Reusing
`kikan_spa_sveltekit::SvelteKitSpa<A: RustEmbed>` preserves the
sister-crate pattern: no new `SpaSource` implementation, no rust-embed
coupling into `kikan` proper.

## Frontend scaffold

The `frontend/` directory currently contains only a hand-written
placeholder bundle under `frontend/build/`. The full SvelteKit 5 +
`adapter-static` + `paths.base='/admin'` source tree, the Tailwind v4
and shadcn-svelte toolchain, the Moon `frontend-*` tasks, and the
pnpm-workspace registration land in subsequent PRs in the M00 kikan
admin UI pipeline.

The placeholder bundle is sufficient for:

- `cargo build --workspace` clean (rust-embed requires `frontend/build/`
  to exist at compile time).
- `CompositeSpaSource` dispatch verification — the composed app serves
  the placeholder from `/admin/` and `/admin/_app/...` assets resolve
  with prefix-stripping.

When the real frontend pipeline lands, `pnpm build` overwrites
`frontend/build/`; the directory will be added to `frontend/.gitignore`
at that point and the committed stub removed.

## Invariants

- **I6 (admin boundary)**: no `mokumo-*` dependency; no shop-domain
  nouns (`customer`, `garment`, `order`, `quote`, `workflow`) appear
  except through `BrandingConfig`. The check script arrives in a
  follow-up PR.
