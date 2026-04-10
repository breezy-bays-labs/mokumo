use std::sync::Arc;

use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;

#[derive(Debug, Clone)]
pub struct MdnsStatus {
    pub active: bool,
    pub hostname: Option<String>,
    pub port: u16,
    pub bind_host: String,
}

impl Default for MdnsStatus {
    fn default() -> Self {
        Self {
            active: false,
            hostname: None,
            port: 0,
            bind_host: "127.0.0.1".into(),
        }
    }
}

pub type SharedMdnsStatus = Arc<RwLock<MdnsStatus>>;

impl MdnsStatus {
    pub fn shared() -> SharedMdnsStatus {
        Arc::new(RwLock::new(Self::default()))
    }
}

pub struct MdnsHandle {
    daemon: Option<mdns_sd::ServiceDaemon>,
    fullname: String,
}

/// Infrastructure test seam for mDNS registration, not a domain abstraction.
pub trait DiscoveryService: Send + Sync {
    fn register(&self, hostname: &str, port: u16) -> Result<MdnsHandle, String>;
}

pub struct RealDiscovery;

impl DiscoveryService for RealDiscovery {
    fn register(&self, hostname: &str, port: u16) -> Result<MdnsHandle, String> {
        let service_type = "_http._tcp.local.";

        let daemon =
            mdns_sd::ServiceDaemon::new().map_err(|e| format!("could not create daemon: {e}"))?;

        let host_fqdn = format!("{hostname}.local.");
        let service_info = mdns_sd::ServiceInfo::new(
            service_type,
            hostname,
            &host_fqdn,
            "",
            port,
            [("app", "mokumo"), ("version", env!("CARGO_PKG_VERSION"))].as_slice(),
        )
        .map_err(|e| format!("could not create service info: {e}"))?
        .enable_addr_auto();

        let fullname = service_info.get_fullname().to_string();

        daemon
            .register(service_info)
            .map_err(|e| format!("registration failed: {e}"))?;

        Ok(MdnsHandle {
            daemon: Some(daemon),
            fullname,
        })
    }
}

pub struct RecordingDiscovery {
    pub calls: parking_lot::Mutex<Vec<(String, u16)>>,
}

impl Default for RecordingDiscovery {
    fn default() -> Self {
        Self {
            calls: parking_lot::Mutex::new(Vec::new()),
        }
    }
}

impl RecordingDiscovery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn call_count(&self) -> usize {
        self.calls.lock().len()
    }
}

impl DiscoveryService for RecordingDiscovery {
    fn register(&self, hostname: &str, port: u16) -> Result<MdnsHandle, String> {
        self.calls.lock().push((hostname.to_string(), port));
        Ok(MdnsHandle {
            daemon: None,
            fullname: format!("{hostname}._http._tcp.local."),
        })
    }
}

pub struct NoOpDiscovery;

impl DiscoveryService for NoOpDiscovery {
    fn register(&self, _hostname: &str, _port: u16) -> Result<MdnsHandle, String> {
        Ok(MdnsHandle {
            daemon: None,
            fullname: String::new(),
        })
    }
}

pub struct FailingDiscovery;

impl DiscoveryService for FailingDiscovery {
    fn register(&self, _hostname: &str, _port: u16) -> Result<MdnsHandle, String> {
        Err("simulated mDNS failure".to_string())
    }
}

pub fn is_loopback(host: &str) -> bool {
    host == "127.0.0.1" || host == "localhost" || host == "::1"
}

pub fn register_mdns(
    host: &str,
    port: u16,
    status: &SharedMdnsStatus,
    discovery: &dyn DiscoveryService,
) -> Option<MdnsHandle> {
    if is_loopback(host) {
        tracing::info!("mDNS registration skipped: bound to loopback address");
        return None;
    }

    let hostname = "mokumo";
    match discovery.register(hostname, port) {
        Ok(handle) => {
            {
                let mut s = status.write();
                s.active = true;
                s.hostname = Some(format!("{hostname}.local"));
                s.port = port;
            }
            // Start collision monitor if a real daemon is available
            if let Some(ref daemon) = handle.daemon
                && let Ok(receiver) = daemon.monitor()
            {
                spawn_collision_monitor(receiver, status.clone());
            }
            tracing::info!("mDNS registered: {hostname}.local:{port} (_http._tcp)");
            Some(handle)
        }
        Err(e) => {
            tracing::warn!("mDNS registration failed: {e}");
            None
        }
    }
}

pub fn spawn_collision_monitor(
    receiver: mdns_sd::Receiver<mdns_sd::DaemonEvent>,
    status: SharedMdnsStatus,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            match receiver.recv_async().await {
                Ok(mdns_sd::DaemonEvent::NameChange(change)) => {
                    if matches!(change.rr_type, mdns_sd::RRType::A | mdns_sd::RRType::AAAA) {
                        tracing::warn!(
                            "mDNS name collision: '{}' changed to '{}'",
                            change.original,
                            change.new_name
                        );
                        let hostname = change.new_name.trim_end_matches('.').to_string();
                        let mut s = status.write();
                        s.hostname = Some(hostname);
                    } else {
                        tracing::debug!(
                            "mDNS service-instance rename ({}): '{}' → '{}'",
                            change.rr_type,
                            change.original,
                            change.new_name
                        );
                    }
                }
                Ok(_) => {} // Ignore other events
                Err(_) => {
                    tracing::debug!("mDNS monitor stopped");
                    break;
                }
            }
        }
    })
}

pub fn deregister_mdns(handle: MdnsHandle, status: &SharedMdnsStatus) {
    if let Some(daemon) = handle.daemon {
        if let Err(e) = daemon.unregister(&handle.fullname) {
            tracing::warn!("Failed to unregister mDNS: {e}");
        }
        if let Err(e) = daemon.shutdown() {
            tracing::warn!("Failed to shut down mDNS daemon: {e}");
        }
    }
    {
        let mut s = status.write();
        s.active = false;
        s.hostname = None;
    }
    tracing::info!("mDNS service deregistered");
}

// ---------------------------------------------------------------------------
// mDNS retry with backoff
// ---------------------------------------------------------------------------

/// Capped backoff schedule: 60s, 120s, then 300s indefinitely.
pub const BACKOFF_SCHEDULE: &[u64] = &[60, 120, 300];
const _: () = assert!(!BACKOFF_SCHEDULE.is_empty());

/// Returns the backoff delay for a given retry attempt (0-indexed).
pub fn backoff_delay(attempt: usize) -> std::time::Duration {
    let secs = BACKOFF_SCHEDULE
        .get(attempt)
        .copied()
        .unwrap_or(*BACKOFF_SCHEDULE.last().unwrap());
    std::time::Duration::from_secs(secs)
}

/// Handle for a running mDNS retry task.
pub struct MdnsRetryHandle {
    cancel: CancellationToken,
    task: tokio::task::JoinHandle<Option<MdnsHandle>>,
}

impl MdnsRetryHandle {
    /// Cancel the retry loop and return any successfully-obtained mDNS handle.
    ///
    /// The caller is responsible for deregistering the returned handle (if any).
    pub async fn cancel(self) -> Option<MdnsHandle> {
        self.cancel.cancel();
        match self.task.await {
            Ok(handle) => handle,
            Err(e) => {
                if e.is_panic() {
                    tracing::error!("mDNS retry task panicked: {e}");
                } else {
                    tracing::debug!("mDNS retry task cancelled: {e}");
                }
                None
            }
        }
    }

    /// Check if the retry task has finished.
    pub fn is_finished(&self) -> bool {
        self.task.is_finished()
    }
}

/// Spawn a background task that retries mDNS registration with capped backoff.
///
/// The retry loop sleeps for `backoff_delay(attempt)` then calls `register_mdns`.
/// On success it returns the handle; on failure it increments the attempt counter.
/// Cancels on either `shutdown` or the internal cancel token.
pub fn spawn_mdns_retry(
    host: String,
    port: u16,
    status: SharedMdnsStatus,
    discovery: Arc<dyn DiscoveryService>,
    shutdown: CancellationToken,
) -> MdnsRetryHandle {
    let cancel = CancellationToken::new();
    let cancel_clone = cancel.clone();

    let task = tokio::spawn(async move {
        let mut attempt = 0;
        loop {
            let delay = backoff_delay(attempt);
            tracing::info!(
                attempt = attempt + 1,
                delay_secs = delay.as_secs(),
                "Scheduling mDNS retry"
            );

            tokio::select! {
                () = tokio::time::sleep(delay) => {}
                () = shutdown.cancelled() => {
                    tracing::info!("mDNS retry cancelled: server shutting down");
                    return None;
                }
                () = cancel_clone.cancelled() => {
                    tracing::info!("mDNS retry cancelled");
                    return None;
                }
            }

            match register_mdns(&host, port, &status, discovery.as_ref()) {
                Some(handle) => {
                    tracing::info!(
                        "mDNS registration succeeded on retry attempt {}",
                        attempt + 1
                    );
                    return Some(handle);
                }
                None => {
                    attempt += 1;
                    if attempt >= 10 {
                        tracing::error!(
                            "mDNS retry attempt {attempt} failed — retries will continue at 5-min intervals"
                        );
                    } else {
                        tracing::warn!("mDNS retry attempt {attempt} failed");
                    }
                }
            }
        }
    });

    MdnsRetryHandle { cancel, task }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn backoff_schedule_values() {
        assert_eq!(backoff_delay(0), std::time::Duration::from_secs(60));
        assert_eq!(backoff_delay(1), std::time::Duration::from_secs(120));
        assert_eq!(backoff_delay(2), std::time::Duration::from_secs(300));
    }

    #[test]
    fn backoff_caps_at_300() {
        assert_eq!(backoff_delay(3), std::time::Duration::from_secs(300));
        assert_eq!(backoff_delay(10), std::time::Duration::from_secs(300));
        assert_eq!(backoff_delay(100), std::time::Duration::from_secs(300));
    }

    /// A configurable discovery service for testing retry behavior.
    struct ConfigurableDiscovery {
        fail_count: std::sync::atomic::AtomicUsize,
        calls: std::sync::Mutex<Vec<(String, u16)>>,
    }

    impl ConfigurableDiscovery {
        fn new(fail_first_n: usize) -> Self {
            Self {
                fail_count: std::sync::atomic::AtomicUsize::new(fail_first_n),
                calls: std::sync::Mutex::new(Vec::new()),
            }
        }

        fn call_count(&self) -> usize {
            self.calls.lock().unwrap().len()
        }
    }

    impl DiscoveryService for ConfigurableDiscovery {
        fn register(&self, hostname: &str, port: u16) -> Result<MdnsHandle, String> {
            self.calls
                .lock()
                .unwrap()
                .push((hostname.to_string(), port));
            let remaining = self.fail_count.load(std::sync::atomic::Ordering::SeqCst);
            if remaining > 0 {
                self.fail_count
                    .fetch_sub(1, std::sync::atomic::Ordering::SeqCst);
                Err("simulated mDNS failure".to_string())
            } else {
                Ok(MdnsHandle {
                    daemon: None,
                    fullname: format!("{hostname}._http._tcp.local."),
                })
            }
        }
    }

    #[tokio::test(start_paused = true)]
    async fn retry_succeeds_after_failures() {
        let status = MdnsStatus::shared();
        let shutdown = CancellationToken::new();
        let discovery = Arc::new(ConfigurableDiscovery::new(2)); // fail 2 times, succeed on 3rd

        let _handle = spawn_mdns_retry(
            "0.0.0.0".to_string(),
            6565,
            status.clone(),
            discovery.clone(),
            shutdown,
        );

        // Advance past first retry delay (60s) — attempt 1 fails
        tokio::time::sleep(std::time::Duration::from_secs(61)).await;
        assert_eq!(discovery.call_count(), 1);

        // Advance past second retry delay (120s) — attempt 2 fails
        tokio::time::sleep(std::time::Duration::from_secs(121)).await;
        assert_eq!(discovery.call_count(), 2);

        // Advance past third retry delay (300s) — attempt 3 succeeds
        tokio::time::sleep(std::time::Duration::from_secs(301)).await;

        assert_eq!(discovery.call_count(), 3);
        let s = status.read().unwrap();
        assert!(s.active, "mDNS should be active after successful retry");
    }

    #[tokio::test(start_paused = true)]
    async fn retry_cancelled_on_shutdown() {
        let status = MdnsStatus::shared();
        let shutdown = CancellationToken::new();
        let discovery = Arc::new(ConfigurableDiscovery::new(100)); // always fail

        let handle = spawn_mdns_retry(
            "0.0.0.0".to_string(),
            6565,
            status.clone(),
            discovery.clone(),
            shutdown.clone(),
        );

        // Cancel via shutdown before first retry (60s delay)
        tokio::time::sleep(std::time::Duration::from_secs(30)).await;
        shutdown.cancel();
        tokio::time::sleep(std::time::Duration::from_secs(1)).await;

        let result = handle.cancel().await;
        assert!(result.is_none());
        assert_eq!(discovery.call_count(), 0); // cancelled before first retry
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_returns_handle_after_success() {
        let status = MdnsStatus::shared();
        let shutdown = CancellationToken::new();
        let discovery = Arc::new(ConfigurableDiscovery::new(0)); // succeed on first try

        let handle = spawn_mdns_retry(
            "0.0.0.0".to_string(),
            6565,
            status.clone(),
            discovery.clone(),
            shutdown,
        );

        // Advance past first retry delay (60s) — succeeds
        tokio::time::sleep(std::time::Duration::from_secs(61)).await;

        let result = handle.cancel().await;
        assert!(
            result.is_some(),
            "cancel() should return the MdnsHandle from a successful retry"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn cancel_returns_none_when_not_yet_succeeded() {
        let status = MdnsStatus::shared();
        let shutdown = CancellationToken::new();
        let discovery = Arc::new(ConfigurableDiscovery::new(100)); // always fail

        let handle = spawn_mdns_retry(
            "0.0.0.0".to_string(),
            6565,
            status.clone(),
            discovery.clone(),
            shutdown,
        );

        // Cancel before any attempt succeeds
        tokio::time::sleep(std::time::Duration::from_secs(10)).await;
        let result = handle.cancel().await;
        assert!(
            result.is_none(),
            "cancel() should return None when retry hasn't succeeded"
        );
    }

    #[tokio::test(start_paused = true)]
    async fn retry_stops_after_success() {
        let status = MdnsStatus::shared();
        let shutdown = CancellationToken::new();
        let discovery = Arc::new(ConfigurableDiscovery::new(0)); // succeed on first try

        let handle = spawn_mdns_retry(
            "0.0.0.0".to_string(),
            6565,
            status.clone(),
            discovery.clone(),
            shutdown,
        );

        // Advance past first retry delay (60s)
        tokio::time::sleep(std::time::Duration::from_secs(61)).await;

        assert_eq!(discovery.call_count(), 1);
        assert!(handle.is_finished());
        let s = status.read().unwrap();
        assert!(s.active);
    }
}
