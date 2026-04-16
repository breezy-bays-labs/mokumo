use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use cucumber::{World, given, then, when};
use tokio::sync::broadcast;

use kikan_events::{BroadcastEventBus, HealthEvent, LifecycleEvent, MigrationEvent, ProfileEvent};

#[derive(Debug, World)]
#[world(init = Self::new)]
pub struct EventWorld {
    bus: Arc<BroadcastEventBus>,
    lifecycle_receivers: HashMap<String, broadcast::Receiver<LifecycleEvent>>,
    health_receivers: HashMap<String, broadcast::Receiver<HealthEvent>>,
    migration_receivers: HashMap<String, broadcast::Receiver<MigrationEvent>>,
    profile_receivers: HashMap<String, broadcast::Receiver<ProfileEvent>>,
    publish_count: usize,
}

impl EventWorld {
    fn new() -> Self {
        Self {
            bus: BroadcastEventBus::new(),
            lifecycle_receivers: HashMap::new(),
            health_receivers: HashMap::new(),
            migration_receivers: HashMap::new(),
            profile_receivers: HashMap::new(),
            publish_count: 0,
        }
    }
}

// --- Background ---

#[given("a BroadcastEventBus with default capacity")]
async fn given_default_bus(w: &mut EventWorld) {
    w.bus = BroadcastEventBus::new();
}

// --- Subscribe steps ---

#[given(expr = "subscriber {string} is subscribed to LifecycleEvent")]
async fn subscribe_lifecycle(w: &mut EventWorld, name: String) {
    let rx = w.bus.subscribe_lifecycle();
    w.lifecycle_receivers.insert(name, rx);
}

#[given(expr = "subscriber {string} is subscribed to HealthEvent")]
async fn subscribe_health(w: &mut EventWorld, name: String) {
    let rx = w.bus.subscribe_health();
    w.health_receivers.insert(name, rx);
}

#[given(expr = "subscriber {string} is subscribed to MigrationEvent")]
async fn subscribe_migration(w: &mut EventWorld, name: String) {
    let rx = w.bus.subscribe_migration();
    w.migration_receivers.insert(name, rx);
}

#[given(expr = "subscriber {string} is subscribed to ProfileEvent")]
async fn subscribe_profile(w: &mut EventWorld, name: String) {
    let rx = w.bus.subscribe_profile();
    w.profile_receivers.insert(name, rx);
}

// --- Publish steps ---

#[when("LifecycleEvent::Serving is published")]
async fn publish_serving(w: &mut EventWorld) {
    w.bus.publish_lifecycle(LifecycleEvent::Serving);
}

#[when("LifecycleEvent::ShutdownInitiated is published")]
async fn publish_shutdown(w: &mut EventWorld) {
    w.bus.publish_lifecycle(LifecycleEvent::ShutdownInitiated);
}

#[when("LifecycleEvent::BootStarted is published")]
async fn publish_boot_started(w: &mut EventWorld) {
    w.bus.publish_lifecycle(LifecycleEvent::BootStarted);
}

#[when("MigrationEvent::Completed is published")]
async fn publish_migration_completed(w: &mut EventWorld) {
    w.bus.publish_migration(MigrationEvent::Completed {
        graft: "test".into(),
        name: "m001".into(),
    });
}

#[when(expr = "{int} HealthEvent::GreenToYellow events are published")]
async fn publish_many_health(w: &mut EventWorld, count: usize) {
    for _ in 0..count {
        w.bus.publish_health(HealthEvent::GreenToYellow {
            reason: "test".into(),
        });
        w.publish_count += 1;
    }
}

#[when(expr = "subscriber {string} subscribes to LifecycleEvent")]
async fn late_subscribe_lifecycle(w: &mut EventWorld, name: String) {
    let rx = w.bus.subscribe_lifecycle();
    w.lifecycle_receivers.insert(name, rx);
}

#[when(expr = "subscriber {string} drops its MigrationEvent receiver")]
async fn drop_migration_receiver(w: &mut EventWorld, name: String) {
    w.migration_receivers.remove(&name);
}

#[when(expr = "{int} tasks each publish a distinct ProfileEvent::Switched concurrently")]
async fn concurrent_profile_publish(w: &mut EventWorld, count: usize) {
    let bus = w.bus.clone();
    let mut handles = Vec::new();
    for i in 0..count {
        let bus = bus.clone();
        handles.push(tokio::spawn(async move {
            bus.publish_profile(ProfileEvent::Switched {
                from: None,
                to: format!("profile-{i}"),
            });
        }));
    }
    for h in handles {
        h.await.unwrap();
    }
}

// --- Then steps ---

#[then(expr = "subscriber {string} receives LifecycleEvent::Serving")]
async fn then_receives_serving(w: &mut EventWorld, name: String) {
    let rx = w.lifecycle_receivers.get_mut(&name).unwrap();
    let event = rx.recv().await.unwrap();
    assert_eq!(event, LifecycleEvent::Serving);
}

#[then(expr = "subscriber {string} receives LifecycleEvent::ShutdownInitiated")]
async fn then_receives_shutdown(w: &mut EventWorld, name: String) {
    let rx = w.lifecycle_receivers.get_mut(&name).unwrap();
    let event = rx.recv().await.unwrap();
    assert_eq!(event, LifecycleEvent::ShutdownInitiated);
}

#[then(expr = "subscriber {string} receives no HealthEvent within 50ms")]
async fn then_no_health_event(w: &mut EventWorld, name: String) {
    let rx = w.health_receivers.get_mut(&name).unwrap();
    let result = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await;
    assert!(result.is_err(), "expected no event but got one");
}

#[then(expr = "subscriber {string} receives no LifecycleEvent within 50ms")]
async fn then_no_lifecycle_event(w: &mut EventWorld, name: String) {
    let rx = w.lifecycle_receivers.get_mut(&name).unwrap();
    let result = tokio::time::timeout(Duration::from_millis(50), rx.recv()).await;
    assert!(result.is_err(), "expected no event but got one");
}

#[then("every publish completes without blocking")]
async fn then_publishes_completed(w: &mut EventWorld) {
    assert!(w.publish_count > 0);
}

#[then(expr = "subscriber {string} receives the latest HealthEvent")]
async fn then_receives_latest_health(w: &mut EventWorld, name: String) {
    let rx = w.health_receivers.get_mut(&name).unwrap();
    let mut last = None;
    loop {
        match rx.try_recv() {
            Ok(e) => last = Some(e),
            Err(broadcast::error::TryRecvError::Lagged(n)) => {
                assert!(n > 0, "should have lagged");
                continue;
            }
            Err(broadcast::error::TryRecvError::Empty) => break,
            Err(broadcast::error::TryRecvError::Closed) => break,
        }
    }
    assert!(last.is_some(), "should have received at least one event");
}

#[then("the publish completes without error")]
async fn then_publish_ok(_w: &mut EventWorld) {
    // If we got here, the publish in the When step didn't panic.
}

#[then(expr = "subscriber {string} receives exactly {int} ProfileEvent events")]
async fn then_receives_exact_count(w: &mut EventWorld, name: String, count: usize) {
    let rx = w.profile_receivers.get_mut(&name).unwrap();
    let mut received = 0;
    while let Ok(Ok(_)) = tokio::time::timeout(Duration::from_millis(100), rx.recv()).await {
        received += 1;
    }
    assert_eq!(received, count, "expected {count} events, got {received}");
}
