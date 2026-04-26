use std::sync::Arc;

use cucumber::World;
use kikan::actor::Actor;
use kikan::error::DomainError;
use mokumo_shop::customer::adapter::SqliteCustomerRepository;
use mokumo_shop::customer::domain::{Customer, CustomerId};
use mokumo_shop::sequence::FormattedSequence;
use sea_orm::DatabaseConnection;

pub mod capturing_writer;
mod customer_atomicity_steps;
mod sequence_steps;

use capturing_writer::CapturingActivityWriter;

#[derive(World)]
#[world(init = Self::new)]
pub struct MokumoShopWorld {
    pub db: Option<DatabaseConnection>,
    pub _tmp: Option<tempfile::TempDir>,
    pub writer: Option<Arc<CapturingActivityWriter>>,
    pub repo: Option<Arc<SqliteCustomerRepository>>,
    pub actor: Actor,
    pub last_customer: Option<Customer>,
    pub last_error: Option<String>,
    pub abort_after_inserts: bool,
    pub named_customers: std::collections::BTreeMap<String, CustomerId>,
    pub alice_user_id: Option<i64>,
    // sequence feature state
    pub seq_prefix: String,
    pub seq_padding: u32,
    pub seq_format_result: String,
    pub seq_last_seeded_name: Option<String>,
    pub seq_result: Option<Result<FormattedSequence, DomainError>>,
    pub seq_results: Vec<Result<FormattedSequence, DomainError>>,
}

impl std::fmt::Debug for MokumoShopWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MokumoShopWorld")
            .field("has_db", &self.db.is_some())
            .field("has_writer", &self.writer.is_some())
            .field("has_repo", &self.repo.is_some())
            .field(
                "last_customer_id",
                &self.last_customer.as_ref().map(|c| c.id),
            )
            .field("last_error", &self.last_error)
            .finish()
    }
}

impl MokumoShopWorld {
    async fn new() -> Self {
        Self {
            db: None,
            _tmp: None,
            writer: None,
            repo: None,
            actor: Actor::system(),
            last_customer: None,
            last_error: None,
            abort_after_inserts: false,
            named_customers: std::collections::BTreeMap::new(),
            alice_user_id: None,
            seq_prefix: String::new(),
            seq_padding: 0,
            seq_format_result: String::new(),
            seq_last_seeded_name: None,
            seq_result: None,
            seq_results: Vec::new(),
        }
    }
}
