//! Mokumo's concretion of kikan's generic auth surface.
//!
//! Kikan's auth types (`AuthenticatedUser<K>`, `ProfileUserId<K>`,
//! `Backend<K>`, `ActiveProfile<K>`) are generic over the vertical's
//! `Graft::ProfileKind`. Mokumo picks `K = SetupMode` — that choice
//! lives here so every mokumo-shop handler can spell the concrete
//! names without repeating the turbofish.
//!
//! This module is the mokumo-side of the capability/vocabulary split
//! (see `adr-kikan-engine-vocabulary.md`): kikan owns the mechanism,
//! mokumo owns the wire type.

pub mod handlers;
pub mod recover;
pub mod recovery_artifact;
pub mod reset;

pub use handlers::{
    AuthSessionType, DEMO_RESET_PATH, regenerate_recovery_codes, require_auth_with_demo_auto_login,
    reset_router, setup_router,
};
pub use kikan_types::SetupMode;

/// Session identity concretion. Corresponds to `axum_login::AuthUser::Id`.
pub type ProfileUserId = kikan::auth::ProfileUserId<SetupMode>;

/// Session-bound user with resolved password hash + profile kind.
pub type AuthenticatedUser = kikan::auth::AuthenticatedUser<SetupMode>;

/// Authentication backend for the `axum-login` stack.
pub type Backend = kikan::auth::Backend<SetupMode>;

/// Per-request active-profile extractor companion to `ProfileDb`.
pub type ActiveProfile = kikan::ActiveProfile<SetupMode>;

/// Profile identifier wrapper.
pub type ProfileId = kikan::ProfileId<SetupMode>;

/// `axum-login`'s auth-session extractor, pinned to Mokumo's backend.
pub type AuthSession = axum_login::AuthSession<Backend>;
