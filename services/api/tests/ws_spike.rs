/// Axum-test WebSocket API spike.
///
/// Verifies that the axum-test `ws` feature provides a usable WS testing API
/// before S2 depends on it. If this test fails, fallback is tokio-tungstenite.
use axum::{Router, extract::ws::WebSocketUpgrade, response::IntoResponse, routing::get};

async fn echo_ws(ws: WebSocketUpgrade) -> impl IntoResponse {
    ws.on_upgrade(|mut socket| async move {
        use axum::extract::ws::Message;
        while let Some(Ok(msg)) = socket.recv().await {
            if let Message::Text(text) = msg
                && socket.send(Message::Text(text)).await.is_err()
            {
                break;
            }
        }
    })
}

#[tokio::test]
async fn axum_test_ws_smoke() {
    let app = Router::new().route("/ws", get(echo_ws));
    let config = axum_test::TestServerConfig {
        transport: Some(axum_test::Transport::HttpRandomPort),
        ..Default::default()
    };
    let server = config.build(app).unwrap();

    let mut ws = server.get_websocket("/ws").await.into_websocket().await;

    ws.send_text("hello").await;
    let msg = ws.receive_text().await;
    assert_eq!(msg, "hello");
}
