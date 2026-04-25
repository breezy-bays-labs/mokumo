# kikan-admin-ui

Sister crate to `kikan-spa-sveltekit`. Holds the rust-embed asset bundle
for the platform admin UI served at `/admin/*` on the composed Tauri
origin.

## Scope

This crate intentionally holds **only** the asset bundle and the
factory that constructs a `SpaSource` for it. All composition logic
(prefix-stripped mounting, dispatch) lives upstream in
`kikan::data_plane::spa::CompositeSpaSource`. Reusing
`kikan_spa_sveltekit::SvelteKitSpa<A: RustEmbed>` preserves the
sister-crate pattern: no new `SpaSource` implementation, no rust-embed
coupling into `kikan` proper.

## Frontend bundle

The `frontend/build/` directory holds a minimal hand-written stub: an
`index.html` shell plus a placeholder asset under
`_app/immutable/chunks/app.js`. `rust-embed` requires the directory to
exist at compile time. The stub is sufficient to verify
`CompositeSpaSource` dispatch end-to-end — the composed app serves it
from `/admin/` and `/admin/_app/...` assets resolve with
prefix-stripping. The SvelteKit source tree, build pipeline, and Moon
tasks are not part of this crate yet.

## Invariants

- **I6 (admin boundary)**: no `mokumo-*` dependency; no shop-domain
  nouns (`customer`, `garment`, `order`, `quote`, `workflow`) appear
  except through `BrandingConfig`.
