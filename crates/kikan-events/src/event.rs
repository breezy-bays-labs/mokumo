pub trait Event: Clone + Send + Sync + 'static {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum LifecycleEvent {
    BootStarted,
    MigrationsRunning,
    Serving,
    ShutdownInitiated,
    ShutdownComplete,
}
impl Event for LifecycleEvent {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum HealthEvent {
    GreenToYellow { reason: String },
    YellowToGreen,
    YellowToRed { reason: String },
    RedToYellow,
}
impl Event for HealthEvent {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MigrationEvent {
    Started {
        graft: String,
        name: String,
    },
    Completed {
        graft: String,
        name: String,
    },
    Failed {
        graft: String,
        name: String,
        error: String,
    },
}
impl Event for MigrationEvent {}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ProfileEvent {
    Enabled { profile_id: String },
    Disabled { profile_id: String },
    Switched { from: Option<String>, to: String },
}
impl Event for ProfileEvent {}
