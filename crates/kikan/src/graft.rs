use crate::engine::EngineContext;
use crate::error::EngineError;
use crate::migrations::bootstrap::BootstrapMigrations;
use crate::migrations::{GraftId, Migration};

#[trait_variant::make(Send)]
pub trait Graft: Sized + 'static {
    type AppState: Clone + Send + Sync + 'static;

    fn id() -> GraftId;
    fn migrations(&self) -> Vec<Box<dyn Migration>>;
    async fn build_state(&self, ctx: &EngineContext) -> Result<Self::AppState, EngineError>;
    fn data_plane_routes(state: &Self::AppState) -> axum::Router<Self::AppState>;
}

#[async_trait::async_trait]
pub trait SubGraft: Send + Sync + 'static {
    fn id(&self) -> GraftId;
    fn migrations(&self) -> Vec<Box<dyn Migration>>;

    async fn on_ignite(&self) -> Result<(), EngineError> {
        Ok(())
    }
    async fn on_liftoff(&self) -> Result<(), EngineError> {
        Ok(())
    }
    async fn on_shutdown(&self) -> Result<(), EngineError> {
        Ok(())
    }
    async fn check(&self) -> Result<(), EngineError> {
        Ok(())
    }
}

pub struct SelfGraft;

#[async_trait::async_trait]
impl SubGraft for SelfGraft {
    fn id(&self) -> GraftId {
        BootstrapMigrations::graft_id()
    }

    fn migrations(&self) -> Vec<Box<dyn Migration>> {
        BootstrapMigrations::migrations()
    }
}
