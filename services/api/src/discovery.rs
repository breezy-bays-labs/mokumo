use std::sync::{Arc, RwLock};

#[derive(Debug, Default, Clone)]
pub struct MdnsStatus {
    pub active: bool,
    pub hostname: Option<String>,
    pub port: u16,
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
    pub calls: std::sync::Mutex<Vec<(String, u16)>>,
}

impl Default for RecordingDiscovery {
    fn default() -> Self {
        Self {
            calls: std::sync::Mutex::new(Vec::new()),
        }
    }
}

impl RecordingDiscovery {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn call_count(&self) -> usize {
        self.calls.lock().unwrap().len()
    }
}

impl DiscoveryService for RecordingDiscovery {
    fn register(&self, hostname: &str, port: u16) -> Result<MdnsHandle, String> {
        self.calls
            .lock()
            .unwrap()
            .push((hostname.to_string(), port));
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
                let mut s = status.write().expect("MdnsStatus lock poisoned");
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
                    tracing::warn!(
                        "mDNS name collision: '{}' changed to '{}'",
                        change.original,
                        change.new_name
                    );
                    let mut s = status.write().expect("MdnsStatus lock poisoned");
                    s.hostname = Some(change.new_name);
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
        let mut s = status.write().expect("MdnsStatus lock poisoned");
        s.active = false;
        s.hostname = None;
    }
    tracing::info!("mDNS service deregistered");
}
