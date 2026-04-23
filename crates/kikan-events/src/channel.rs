//! Domain-neutral broadcast fanout primitive.
//!
//! `FanoutChannel<T>` wraps [`tokio::sync::broadcast::Sender`] with a narrow,
//! stable surface (`publish`, `subscribe`, `receiver_count`, `with_capacity`).
//! It is the mechanism; typed buses ([`crate::bus::BroadcastEventBus`],
//! `mokumo-shop::ws::ConnectionManager`) compose it privately and own their
//! own taxonomies.
//!
//! Reach for `FanoutChannel<T>` when you need a typed broadcast and the
//! receiver count/default-capacity conventions that the rest of kikan uses.
//! Reach for raw `tokio::sync::broadcast` when you need capabilities the
//! primitive intentionally hides (e.g. split halves, explicit `SendError`).

use tokio::sync::broadcast;

/// Default buffered capacity per fanout channel.
pub const DEFAULT_CAPACITY: usize = 1024;

/// Single-producer-visible fanout over `tokio::sync::broadcast`.
///
/// Drops events when no receivers exist; `publish` returns `0` in that case.
/// Lagged receivers receive a `RecvError::Lagged` from their own `Receiver`
/// and keep progressing — the channel itself stays healthy.
pub struct FanoutChannel<T: Clone + Send + 'static> {
    tx: broadcast::Sender<T>,
}

impl<T: Clone + Send + 'static> FanoutChannel<T> {
    pub fn with_capacity(cap: usize) -> Self {
        let (tx, _) = broadcast::channel(cap);
        Self { tx }
    }

    /// Send to all current subscribers. Returns the number of receivers the
    /// event reached; `0` when there are no subscribers (normal operation).
    pub fn publish(&self, value: T) -> usize {
        self.tx.send(value).unwrap_or(0)
    }

    pub fn subscribe(&self) -> broadcast::Receiver<T> {
        self.tx.subscribe()
    }

    pub fn receiver_count(&self) -> usize {
        self.tx.receiver_count()
    }
}

impl<T: Clone + Send + 'static> std::fmt::Debug for FanoutChannel<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FanoutChannel")
            .field("receiver_count", &self.tx.receiver_count())
            .finish_non_exhaustive()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn fanout_channel_publish_reaches_subscriber() {
        let ch = FanoutChannel::<u32>::with_capacity(8);
        let mut rx = ch.subscribe();
        let delivered = ch.publish(42);
        assert_eq!(delivered, 1);
        assert_eq!(rx.recv().await.unwrap(), 42);
    }

    #[tokio::test]
    async fn fanout_channel_multiple_subscribers_each_receive() {
        let ch = FanoutChannel::<u32>::with_capacity(8);
        let mut rx_a = ch.subscribe();
        let mut rx_b = ch.subscribe();
        let delivered = ch.publish(7);
        assert_eq!(delivered, 2);
        assert_eq!(rx_a.recv().await.unwrap(), 7);
        assert_eq!(rx_b.recv().await.unwrap(), 7);
    }

    #[test]
    fn fanout_channel_with_no_subscribers_returns_zero() {
        let ch = FanoutChannel::<u32>::with_capacity(8);
        assert_eq!(ch.publish(1), 0);
        assert_eq!(ch.receiver_count(), 0);
    }

    #[tokio::test]
    async fn fanout_channel_lagged_receiver_still_gets_new_events() {
        let ch = FanoutChannel::<u32>::with_capacity(2);
        let mut rx = ch.subscribe();
        for i in 0..5 {
            ch.publish(i);
        }
        match rx.recv().await {
            Err(broadcast::error::RecvError::Lagged(_)) => {}
            other => panic!("expected Lagged, got {other:?}"),
        }
        while rx.try_recv().is_ok() {}
        ch.publish(99);
        assert_eq!(rx.recv().await.unwrap(), 99);
    }
}
