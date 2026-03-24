use dashmap::DashMap;
use mokumo_types::ws::BroadcastEvent;
use std::sync::Arc;
use tokio::sync::broadcast;
use uuid::Uuid;

/// Manages WebSocket connections and broadcasts pre-serialized events.
pub struct ConnectionManager {
    broadcast_tx: broadcast::Sender<Arc<str>>,
    connections: DashMap<Uuid, ()>,
}

impl ConnectionManager {
    pub fn new(capacity: usize) -> Self {
        let (broadcast_tx, _) = broadcast::channel(capacity);
        Self {
            broadcast_tx,
            connections: DashMap::new(),
        }
    }

    pub fn add(&self) -> (Uuid, broadcast::Receiver<Arc<str>>) {
        let id = Uuid::new_v4();
        let rx = self.broadcast_tx.subscribe();
        self.connections.insert(id, ());
        (id, rx)
    }

    /// No-op if the ID doesn't exist.
    pub fn remove(&self, id: Uuid) {
        self.connections.remove(&id);
    }

    /// Serialize once and broadcast to all subscribers.
    /// Returns the number of receivers that received it.
    pub fn broadcast(&self, event: BroadcastEvent) -> usize {
        let json: Arc<str> = serde_json::to_string(&event)
            .expect("BroadcastEvent serialization cannot fail")
            .into();
        self.broadcast_tx.send(json).unwrap_or(0)
    }

    pub fn connection_count(&self) -> usize {
        self.connections.len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn make_event(type_: &str) -> BroadcastEvent {
        BroadcastEvent::new(type_, serde_json::json!({"test": true}))
    }

    #[test]
    fn test_add_returns_receiver() {
        let mgr = ConnectionManager::new(16);
        let (id, _rx) = mgr.add();
        assert!(!id.is_nil());
    }

    #[test]
    fn test_remove_decrements_count() {
        let mgr = ConnectionManager::new(16);
        let (id, _rx) = mgr.add();
        assert_eq!(mgr.connection_count(), 1);
        mgr.remove(id);
        assert_eq!(mgr.connection_count(), 0);
    }

    #[test]
    fn test_connection_count() {
        let mgr = ConnectionManager::new(16);
        let _a = mgr.add();
        let _b = mgr.add();
        let _c = mgr.add();
        assert_eq!(mgr.connection_count(), 3);
    }

    #[tokio::test]
    async fn test_broadcast_reaches_receiver() {
        let mgr = ConnectionManager::new(16);
        let (_id, mut rx) = mgr.add();

        let event = make_event("customer.created");
        let sent = mgr.broadcast(event.clone());
        assert_eq!(sent, 1);

        let json_str = rx.recv().await.unwrap();
        let received: BroadcastEvent = serde_json::from_str(&json_str).unwrap();
        assert_eq!(received, event);
    }

    #[test]
    fn test_broadcast_no_receivers_does_not_panic() {
        let mgr = ConnectionManager::new(16);
        let event = make_event("customer.created");
        let sent = mgr.broadcast(event);
        assert_eq!(sent, 0);
    }

    #[test]
    fn test_remove_nonexistent_is_noop() {
        let mgr = ConnectionManager::new(16);
        let fake_id = Uuid::new_v4();
        mgr.remove(fake_id);
    }
}
