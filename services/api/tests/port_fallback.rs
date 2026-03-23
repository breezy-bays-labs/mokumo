use serial_test::serial;

use mokumo_api::try_bind;

#[tokio::test]
#[serial]
async fn try_bind_finds_requested_port_when_available() {
    let (listener, actual_port) = try_bind("127.0.0.1", 16565).await.unwrap();
    assert_eq!(actual_port, 16565);
    drop(listener);
}

#[tokio::test]
#[serial]
async fn try_bind_skips_occupied_port_and_finds_next() {
    // Occupy the first port with a std::net listener
    let blocker = std::net::TcpListener::bind("127.0.0.1:16600").unwrap();

    let (listener, actual_port) = try_bind("127.0.0.1", 16600).await.unwrap();
    assert_ne!(actual_port, 16600, "should not bind to the occupied port");
    assert!(
        actual_port > 16600 && actual_port <= 16610,
        "should find a port in the fallback range, got {actual_port}"
    );

    drop(listener);
    drop(blocker);
}

#[tokio::test]
#[serial]
async fn try_bind_returns_error_when_all_ports_exhausted() {
    // Occupy all 11 ports in the range 16700..=16710
    let mut blockers = Vec::new();
    for p in 16700..=16710 {
        let l = std::net::TcpListener::bind(format!("127.0.0.1:{p}")).unwrap();
        blockers.push(l);
    }

    let result = try_bind("127.0.0.1", 16700).await;
    assert!(
        result.is_err(),
        "should fail when all 11 ports are occupied"
    );

    let err_msg = result.unwrap_err().to_string();
    assert!(
        err_msg.contains("16700") && err_msg.contains("16710"),
        "error should mention the port range, got: {err_msg}"
    );

    drop(blockers);
}
