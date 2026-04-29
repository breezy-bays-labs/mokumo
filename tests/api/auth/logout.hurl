# POST /api/auth/logout — legacy alias mounted at `routes.rs:135`,
# routing to the same canonical kikan handler as
# `/api/platform/v1/auth/logout`. The handler always returns 204 whether
# or not a session is attached (see
# `crates/kikan/src/platform/v1/auth/logout.rs`), so Phase 1 placement
# (anonymous, no cookie jar) exercises the alias mount without
# invalidating the shared cookie jar Phase 2 tests rely on.
POST http://{{host}}/api/auth/logout

HTTP 204
