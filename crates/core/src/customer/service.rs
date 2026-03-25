use crate::activity::ActivityAction;
use crate::activity::traits::ActivityLogRepository;
use crate::customer::traits::CustomerRepository;
use crate::customer::{CreateCustomer, Customer, CustomerId, UpdateCustomer};
use crate::error::DomainError;
use crate::filter::IncludeDeleted;
use crate::pagination::PageParams;

pub struct CustomerService<R, A> {
    repo: R,
    activity: A,
}

impl<R: CustomerRepository, A: ActivityLogRepository> CustomerService<R, A> {
    pub fn new(repo: R, activity: A) -> Self {
        Self { repo, activity }
    }

    pub async fn find_by_id(
        &self,
        id: &CustomerId,
        filter: IncludeDeleted,
    ) -> Result<Option<Customer>, DomainError> {
        self.repo.find_by_id(id, filter).await
    }

    pub async fn list(
        &self,
        params: PageParams,
        filter: IncludeDeleted,
    ) -> Result<(Vec<Customer>, i64), DomainError> {
        self.repo.list(params, filter).await
    }

    // TODO(#59): wrap mutation + activity log in shared transaction (Option G)
    pub async fn create(&self, req: &CreateCustomer) -> Result<Customer, DomainError> {
        let customer = self.repo.create(req).await?;
        self.log_activity(&customer, ActivityAction::Created)
            .await?;
        Ok(customer)
    }

    // TODO(#59): wrap mutation + activity log in shared transaction (Option G)
    pub async fn update(
        &self,
        id: &CustomerId,
        req: &UpdateCustomer,
    ) -> Result<Customer, DomainError> {
        let customer = self.repo.update(id, req).await?;
        self.log_activity(&customer, ActivityAction::Updated)
            .await?;
        Ok(customer)
    }

    // TODO(#59): wrap mutation + activity log in shared transaction (Option G)
    pub async fn soft_delete(&self, id: &CustomerId) -> Result<Customer, DomainError> {
        let customer = self.repo.soft_delete(id).await?;
        self.log_activity(&customer, ActivityAction::SoftDeleted)
            .await?;
        Ok(customer)
    }

    async fn log_activity(
        &self,
        customer: &Customer,
        action: ActivityAction,
    ) -> Result<(), DomainError> {
        let payload = serde_json::to_value(customer).map_err(|e| DomainError::Internal {
            message: format!("failed to serialize customer for activity log: {e}"),
        })?;
        self.activity
            .log(
                "customer",
                &customer.id.to_string(),
                &action.to_string(),
                "system",
                "system",
                &payload,
            )
            .await?;
        Ok(())
    }
}
