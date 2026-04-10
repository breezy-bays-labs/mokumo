use serial_test::serial;

use mokumo_api::try_bind;

#[tokio::test]
#[serial]
async fn try_bind_finds_requested_port_when_available() {
    let (_, actual_port) = try_bind("127.0.0.1", 16565).await.unwrap();
    assert_eq!(actual_port, 16565);
}

#[tokio::test]
#[serial]
async fn try_bind_skips_occupied_port_and_finds_next() {
    // Occupy the first port with a std::net listener
    let _blocker = std::net::TcpListener::bind("127.0.0.1:16600").unwrap();

    let (_, actual_port) = try_bind("127.0.0.1", 16600).await.unwrap();
    assert_eq!(
        actual_port, 16601,
        "should bind to the exact next port after the occupied one"
    );
}

#[tokio::test]
#[serial]
async fn try_bind_returns_error_when_all_ports_exhausted() {
    // Occupy all 11 ports in the range 16700..=16710
    let _blockers: Vec<_> = (16700..=16710)
        .map(|p| std::net::TcpListener::bind(format!("127.0.0.1:{p}")).unwrap())
        .collect();

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
    assert!(
        err_msg.contains("--port"),
        "error should suggest --port flag, got: {err_msg}"
    );
    assert!(
        err_msg.contains("close conflicting"),
        "error should suggest closing apps, got: {err_msg}"
    );
}

#[tokio::test]
#[serial]
async fn try_bind_handles_port_near_u16_max() {
    // With saturating_add(10), port 65535 should try exactly one port (65535)
    // and either succeed or fail without panicking
    let result = try_bind("127.0.0.1", 65530).await;
    // We just verify it doesn't panic — the port may or may not be available
    match result {
        Ok((_, port)) => assert!(
            port >= 65530,
            "should bind to a port in the saturated range"
        ),
        Err(e) => assert!(
            e.to_string().contains("65530"),
            "error should mention the starting port"
        ),
    }
}
