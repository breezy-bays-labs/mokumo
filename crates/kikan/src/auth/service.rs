use mokumo_core::error::DomainError;

use super::domain::{RoleId, User, UserId, UserRepository};

pub struct UserService<R> {
    repo: R,
}

impl<R: UserRepository> UserService<R> {
    pub fn new(repo: R) -> Self {
        Self { repo }
    }

    pub async fn soft_delete_user(
        &self,
        id: &UserId,
        actor_id: UserId,
    ) -> Result<User, DomainError> {
        let target = self
            .repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound {
                entity: "user",
                id: id.to_string(),
            })?;

        if target.role_id == RoleId::ADMIN {
            let count = self.repo.count_active_admins().await?;
            if count <= 1 {
                return Err(DomainError::Conflict {
                    message: "Cannot delete the last admin account. Assign another admin first."
                        .into(),
                });
            }
        }

        self.repo.soft_delete_user(id, actor_id).await
    }

    pub async fn update_user_role(
        &self,
        id: &UserId,
        new_role: RoleId,
        actor_id: UserId,
    ) -> Result<User, DomainError> {
        let target = self
            .repo
            .find_by_id(id)
            .await?
            .ok_or_else(|| DomainError::NotFound {
                entity: "user",
                id: id.to_string(),
            })?;

        if target.role_id == RoleId::ADMIN && new_role != RoleId::ADMIN {
            let count = self.repo.count_active_admins().await?;
            if count <= 1 {
                return Err(DomainError::Conflict {
                    message: "Cannot demote the last admin account. Assign another admin first."
                        .into(),
                });
            }
        }

        self.repo.update_user_role(id, new_role, actor_id).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicBool, Ordering};

    fn make_user(role_id: RoleId) -> User {
        User {
            id: UserId::new(1),
            email: "test@shop.local".into(),
            name: "Test".into(),
            role_id,
            is_active: true,
            last_login_at: None,
            created_at: "2026-01-01T00:00:00Z".into(),
            updated_at: "2026-01-01T00:00:00Z".into(),
            deleted_at: None,
        }
    }

    struct MockRepo {
        target_role: RoleId,
        active_admin_count: u64,
        /// Tracks whether soft_delete_user or update_user_role was called.
        mutation_called: AtomicBool,
    }

    impl MockRepo {
        fn new(target_role: RoleId, active_admin_count: u64) -> Self {
            Self {
                target_role,
                active_admin_count,
                mutation_called: AtomicBool::new(false),
            }
        }
    }

    impl UserRepository for MockRepo {
        async fn create(
            &self,
            _req: &super::super::domain::CreateUser,
        ) -> Result<User, DomainError> {
            unimplemented!("not needed for guard tests")
        }

        async fn find_by_id(&self, _id: &UserId) -> Result<Option<User>, DomainError> {
            Ok(Some(make_user(self.target_role)))
        }

        async fn find_by_email(&self, _email: &str) -> Result<Option<User>, DomainError> {
            unimplemented!()
        }

        async fn update_password(
            &self,
            _id: &UserId,
            _new_password: &str,
        ) -> Result<(), DomainError> {
            unimplemented!()
        }

        async fn count(&self) -> Result<i64, DomainError> {
            unimplemented!()
        }

        async fn soft_delete_user(
            &self,
            _id: &UserId,
            _actor_id: UserId,
        ) -> Result<User, DomainError> {
            self.mutation_called.store(true, Ordering::SeqCst);
            Ok(make_user(self.target_role))
        }

        async fn update_user_role(
            &self,
            _id: &UserId,
            new_role: RoleId,
            _actor_id: UserId,
        ) -> Result<User, DomainError> {
            self.mutation_called.store(true, Ordering::SeqCst);
            Ok(make_user(new_role))
        }

        async fn count_active_admins(&self) -> Result<u64, DomainError> {
            Ok(self.active_admin_count)
        }
    }

    fn actor() -> UserId {
        UserId::new(99)
    }

    fn target_id() -> UserId {
        UserId::new(1)
    }

    // Scenario 1: soft-delete a non-admin user — succeeds, guard not checked
    #[tokio::test]
    async fn soft_delete_user_non_admin_succeeds() {
        let repo = MockRepo::new(RoleId::STAFF, 0); // count irrelevant
        let svc = UserService::new(repo);
        let result = svc.soft_delete_user(&target_id(), actor()).await;
        assert!(result.is_ok());
        assert!(svc.repo.mutation_called.load(Ordering::SeqCst));
    }

    // Scenario 2: soft-delete one of two admins — succeeds (count = 2)
    #[tokio::test]
    async fn soft_delete_user_one_of_two_admins_succeeds() {
        let repo = MockRepo::new(RoleId::ADMIN, 2);
        let svc = UserService::new(repo);
        let result = svc.soft_delete_user(&target_id(), actor()).await;
        assert!(result.is_ok());
        assert!(svc.repo.mutation_called.load(Ordering::SeqCst));
    }

    // Scenario 3: soft-delete the last active admin — rejected (count = 1)
    #[tokio::test]
    async fn soft_delete_user_last_admin_rejected() {
        let repo = MockRepo::new(RoleId::ADMIN, 1);
        let svc = UserService::new(repo);
        let result = svc.soft_delete_user(&target_id(), actor()).await;
        match result {
            Err(DomainError::Conflict { message }) => {
                assert_eq!(
                    message,
                    "Cannot delete the last admin account. Assign another admin first."
                );
            }
            other => panic!("expected Conflict, got {other:?}"),
        }
        assert!(!svc.repo.mutation_called.load(Ordering::SeqCst));
    }

    // Scenario 4: promote staff to admin — succeeds, guard not checked
    #[tokio::test]
    async fn update_user_role_promote_succeeds() {
        let repo = MockRepo::new(RoleId::STAFF, 0); // count irrelevant for non-ADMIN target
        let svc = UserService::new(repo);
        let result = svc
            .update_user_role(&target_id(), RoleId::ADMIN, actor())
            .await;
        assert!(result.is_ok());
        assert!(svc.repo.mutation_called.load(Ordering::SeqCst));
    }

    // Scenario 5: demote one of two admins — succeeds (count = 2)
    #[tokio::test]
    async fn update_user_role_demote_one_of_two_admins_succeeds() {
        let repo = MockRepo::new(RoleId::ADMIN, 2);
        let svc = UserService::new(repo);
        let result = svc
            .update_user_role(&target_id(), RoleId::STAFF, actor())
            .await;
        assert!(result.is_ok());
        assert!(svc.repo.mutation_called.load(Ordering::SeqCst));
    }

    // Scenario 6: demote the last active admin — rejected (count = 1)
    #[tokio::test]
    async fn update_user_role_demote_last_admin_rejected() {
        let repo = MockRepo::new(RoleId::ADMIN, 1);
        let svc = UserService::new(repo);
        let result = svc
            .update_user_role(&target_id(), RoleId::STAFF, actor())
            .await;
        match result {
            Err(DomainError::Conflict { message }) => {
                assert_eq!(
                    message,
                    "Cannot demote the last admin account. Assign another admin first."
                );
            }
            other => panic!("expected Conflict, got {other:?}"),
        }
        assert!(!svc.repo.mutation_called.load(Ordering::SeqCst));
    }

    // Scenario 7: count returns 1 (ghost admin excluded by adapter) — demote rejected
    // This verifies that count=1 fires the guard regardless of why it's 1.
    #[tokio::test]
    async fn update_user_role_ghost_admin_count_one_rejected() {
        // The mock returns count=1, simulating the adapter correctly excluding deleted admins.
        // Guard fires because count ≤ 1.
        let repo = MockRepo::new(RoleId::ADMIN, 1);
        let svc = UserService::new(repo);
        let result = svc
            .update_user_role(&target_id(), RoleId::STAFF, actor())
            .await;
        assert!(matches!(result, Err(DomainError::Conflict { .. })));
        assert!(!svc.repo.mutation_called.load(Ordering::SeqCst));
    }
}
