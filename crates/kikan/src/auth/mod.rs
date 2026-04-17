//! Platform auth: users, roles, password hashing, the SeaORM repository,
//! and the `axum-login` backend (`Backend`) + session user
//! (`AuthenticatedUser` / `ProfileUserId`).
//!
//! Stage 3 (#507) lifted the domain types (from `crates/core`), entities + repo
//! (from `crates/db`), and the axum-login backend + session user wrappers
//! (from `services/api/src/auth/`) into kikan.
//!
//! S2.1c (V5c) intentionally scopes the move to the decoupled pieces
//! (`Backend`, `AuthenticatedUser`, `ProfileUserId`). The HTTP handler layer
//! (`login`/`logout`/`me`/`recover`/`reset`/`setup`/`regen`, plus the
//! `ProfileDb` middleware and the activity list handler) still lives in
//! `services/api/src/{auth,activity,profile_db}` because it's welded to
//! `SharedState` and `AppError`. It migrates in S3.1 when `MokumoAppState`
//! materialises and `FromRef<EngineContext>` becomes real — at which point
//! the `platform_routes<S>() -> Router<S> where EngineContext: FromRef<S>`
//! seam can carry the handlers into `Engine::build_router` properly.

pub mod backend;
pub mod domain;
pub mod entity_role;
pub mod entity_user;
pub mod password;
pub mod repo;
pub mod service;
pub mod user;

pub use backend::{Backend, Credentials};
pub use domain::{CreateUser, Role, RoleId, User, UserId, UserRepository};
pub use repo::SeaOrmUserRepo;
pub use service::UserService;
pub use user::{AuthenticatedUser, ProfileUserId};
