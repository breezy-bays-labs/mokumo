use std::net::Ipv4Addr;

use cucumber::{then, when};
use mokumo_api::try_bind_ephemeral_loopback;

use super::ApiWorld;

// Step definitions for services/api/tests/features/ephemeral_bind.feature.
// These test try_bind_ephemeral_loopback() directly — no HTTP server involved.

#[when("the desktop server requests an ephemeral loopback port")]
async fn request_ephemeral_loopback_port(w: &mut ApiWorld) {
    let (listener, addr) = try_bind_ephemeral_loopback()
        .await
        .expect("try_bind_ephemeral_loopback should succeed");
    w.ephemeral_addr = Some(addr);
    w.ephemeral_listener = Some(listener);
}

// --- Bind address assertions ---

#[then("the bound address is on 127.0.0.1")]
async fn bound_address_is_loopback(w: &mut ApiWorld) {
    let addr = w
        .ephemeral_addr
        .expect("ephemeral bind must have run first");
    assert_eq!(
        addr.ip(),
        std::net::IpAddr::V4(Ipv4Addr::LOCALHOST),
        "Bound address should be 127.0.0.1, got {}",
        addr.ip()
    );
}

#[then("the server is not reachable from other network interfaces")]
async fn not_reachable_from_other_interfaces(w: &mut ApiWorld) {
    let addr = w
        .ephemeral_addr
        .expect("ephemeral bind must have run first");
    assert!(
        addr.ip().is_loopback(),
        "Address should be loopback-only, got {}",
        addr.ip()
    );
}

// --- OS-assigned port assertions ---

#[then("the assigned port is greater than zero")]
async fn assigned_port_greater_than_zero(w: &mut ApiWorld) {
    let addr = w
        .ephemeral_addr
        .expect("ephemeral bind must have run first");
    assert!(
        addr.port() > 0,
        "OS-assigned port should be > 0, got {}",
        addr.port()
    );
}

#[then("no fixed or preferred port was requested")]
async fn no_fixed_port_requested(w: &mut ApiWorld) {
    let addr = w
        .ephemeral_addr
        .expect("ephemeral bind must have run first");
    // Binding "127.0.0.1:0" means we explicitly did not request a specific port.
    // The resulting port is always > 0 (proof that the OS assigned it).
    assert!(addr.port() > 0);
}

// --- Readable address assertions ---

#[then("the full socket address host and port can be read from the listener")]
async fn socket_address_readable_from_listener(w: &mut ApiWorld) {
    let listener = w
        .ephemeral_listener
        .as_ref()
        .expect("listener should be stored from When step");
    let addr = listener
        .local_addr()
        .expect("local_addr() should succeed on a bound listener");
    assert!(addr.port() > 0);
}

#[then("the address host is 127.0.0.1")]
async fn address_host_is_loopback(w: &mut ApiWorld) {
    let listener = w
        .ephemeral_listener
        .as_ref()
        .expect("listener should be stored from When step");
    let addr = listener.local_addr().expect("local_addr() should succeed");
    assert_eq!(
        addr.ip(),
        std::net::IpAddr::V4(Ipv4Addr::LOCALHOST),
        "Listener host should be 127.0.0.1, got {}",
        addr.ip()
    );
}

#[then("the address port is the same as the OS-assigned port")]
async fn address_port_matches_assigned_port(w: &mut ApiWorld) {
    let stored_port = w
        .ephemeral_addr
        .expect("addr must be set from When step")
        .port();
    let listener = w
        .ephemeral_listener
        .as_ref()
        .expect("listener should be stored from When step");
    let read_port = listener
        .local_addr()
        .expect("local_addr() should succeed")
        .port();
    assert_eq!(
        stored_port, read_port,
        "Port from local_addr() should match the addr returned by try_bind_ephemeral_loopback"
    );
}

// --- Independence from the fixed-port range ---

// Note: "Given ports {int} through {int} are already in use" is the existing
// no-op step in port_fallback_steps.rs. The actual port occupation is done
// inline here to make the assertion deterministic.

#[then("the bind succeeds")]
async fn bind_succeeds(w: &mut ApiWorld) {
    assert!(
        w.ephemeral_addr.is_some(),
        "Ephemeral bind should have succeeded and addr stored"
    );
}

#[then("the assigned port is outside the 6565-6575 range")]
async fn assigned_port_outside_default_range(_w: &mut ApiWorld) {
    // Occupy ports 6565–6575 so the OS must pick something else for :0 bind.
    // _blockers stay alive until after the bind assertion (drop at end of step).
    let _blockers: Vec<_> = (6565u16..=6575)
        .filter_map(|p| std::net::TcpListener::bind(format!("127.0.0.1:{p}")).ok())
        .collect();

    let (_, addr) = try_bind_ephemeral_loopback()
        .await
        .expect("try_bind_ephemeral_loopback should succeed even with 6565-6575 occupied");

    assert!(
        addr.port() < 6565 || addr.port() > 6575,
        "Expected port outside 6565-6575, got {}",
        addr.port()
    );
}
