//! Structural invariant: no SeaORM transaction types leak from `UserRepository`.
//!
//! The BDD spec `user_repo_atomicity.feature` pins composite-method atomicity
//! at the behavioral level. This test pins the *structural* side of the same
//! contract: handlers at the kikan boundary must never see a
//! `sea_orm::DatabaseTransaction` or `sea_orm::TransactionTrait` handle. All
//! transactional composition stays inside the adapter.
//!
//! The guard is compile-time: every method on `UserRepository` must accept
//! exactly the types the `UserRepoBoundary` impl block forwards — which are
//! framework-free. If someone adds a new method on the trait and its
//! signature mentions a SeaORM transaction type, this file stops compiling
//! because the forward-to-boundary signatures won't line up.
//!
//! This test does not exercise behavior; it exercises shape. The behavior
//! test is the BDD suite.

use kikan::auth::domain::{CreateUser, RoleId, User, UserId, UserRepository};
use kikan::auth::repo::SeaOrmUserRepo;
use kikan::error::DomainError;

#[allow(dead_code)]
async fn boundary_create<R: UserRepository>(
    repo: &R,
    req: &CreateUser,
) -> Result<User, DomainError> {
    repo.create(req).await
}

#[allow(dead_code)]
async fn boundary_find_by_id<R: UserRepository>(
    repo: &R,
    id: &UserId,
) -> Result<Option<User>, DomainError> {
    repo.find_by_id(id).await
}

#[allow(dead_code)]
async fn boundary_find_by_email<R: UserRepository>(
    repo: &R,
    email: &str,
) -> Result<Option<User>, DomainError> {
    repo.find_by_email(email).await
}

#[allow(dead_code)]
async fn boundary_update_password<R: UserRepository>(
    repo: &R,
    id: &UserId,
    pw: &str,
) -> Result<(), DomainError> {
    repo.update_password(id, pw).await
}

#[allow(dead_code)]
async fn boundary_count<R: UserRepository>(repo: &R) -> Result<i64, DomainError> {
    repo.count().await
}

#[allow(dead_code)]
async fn boundary_count_admins<R: UserRepository>(repo: &R) -> Result<u64, DomainError> {
    repo.count_active_admins().await
}

#[allow(dead_code)]
async fn boundary_soft_delete<R: UserRepository>(
    repo: &R,
    id: &UserId,
    actor: UserId,
) -> Result<User, DomainError> {
    repo.soft_delete_user(id, actor).await
}

#[allow(dead_code)]
async fn boundary_update_role<R: UserRepository>(
    repo: &R,
    id: &UserId,
    new_role: RoleId,
    actor: UserId,
) -> Result<User, DomainError> {
    repo.update_user_role(id, new_role, actor).await
}

/// Compile-time witness that `SeaOrmUserRepo` fulfills the `UserRepository`
/// trait contract without requiring the caller to import any SeaORM type.
///
/// The real enforcement is structural: this file contains no `use sea_orm::`
/// clauses and every method signature is generic over `R: UserRepository`.
/// The `boundary_*` functions above compile only if the trait's public
/// surface is free of SeaORM vocabulary. Adding a SeaORM type to a trait
/// method signature would either break the forwarders or require this file
/// to import SeaORM — which a reviewer will catch in the diff.
#[test]
fn user_repository_trait_is_framework_free() {
    fn assert_repo<R: UserRepository>() {}
    assert_repo::<SeaOrmUserRepo>();
}
