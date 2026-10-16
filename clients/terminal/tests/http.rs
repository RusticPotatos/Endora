use endora_terminal::{client::Client, storage::Config};
use std::{
    io::{Read, Write},
    net::TcpListener,
    sync::mpsc,
};

fn server(
    status: &str,
    body: &str,
    headers: &str,
) -> (Client, mpsc::Receiver<String>, std::thread::JoinHandle<()>) {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let config = Config {
        url: format!("http://{address}"),
        allow_http: true,
        ca_file: None,
    };
    let client = Client::new(&config, Some("fictional-session".into())).unwrap();
    let response = format!(
        "HTTP/1.1 {status}\r\nContent-Length: {}\r\n{headers}\r\n{body}",
        body.len()
    );
    let (tx, rx) = mpsc::channel();
    let thread = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        socket
            .set_read_timeout(Some(std::time::Duration::from_secs(3)))
            .unwrap();
        let mut data = vec![0; 8192];
        let n = socket.read(&mut data).unwrap();
        let _ = tx.send(String::from_utf8_lossy(&data[..n]).to_string());
        socket.write_all(response.as_bytes()).unwrap();
    });
    (client, rx, thread)
}

#[tokio::test]
async fn bearer_auth_and_expiry_are_safe() {
    let (client, rx, thread) = server("401 Unauthorized", "secret upstream details", "");
    let error = client.get("/v1/chat").await.unwrap_err();
    assert!(error.contains("Session expired"));
    assert!(!error.contains("secret"));
    assert!(
        rx.recv()
            .unwrap()
            .to_lowercase()
            .contains("authorization: bearer fictional-session")
    );
    thread.join().unwrap();
}

#[tokio::test]
async fn setup_errors_are_readable_and_unknown_upstream_details_stay_private() {
    let (client, _, thread) = server("400 Bad Request", r#"{"error":"unconfigured"}"#, "");
    assert!(
        client
            .get("/v1/terminal/home")
            .await
            .unwrap_err()
            .contains("private server setup")
    );
    thread.join().unwrap();
    let (client, _, thread) = server(
        "400 Bad Request",
        r#"{"error":"fictional-private-upstream-detail"}"#,
        "",
    );
    assert_eq!(
        client.get("/v1/terminal/home").await.unwrap_err(),
        "Node rejected request (400)"
    );
    thread.join().unwrap();
}

#[tokio::test]
async fn redirects_are_not_followed() {
    let (client, _, thread) = server("302 Found", "", "Location: http://192.168.1.10/\r\n");
    assert_eq!(
        client.get("/health").await.unwrap_err(),
        "Node rejected request (302)"
    );
    thread.join().unwrap();
}

#[tokio::test]
async fn interrupted_chat_is_uncertain_and_not_retried() {
    let (client, rx, thread) = server(
        "200 OK",
        "data: {\"type\":\"token\",\"text\":\"partial\"}\n\n",
        "Content-Type: text/event-stream\r\n",
    );
    let (tx, mut events) = tokio::sync::mpsc::channel(4);
    assert!(
        client
            .chat("Hello".into(), tx)
            .await
            .unwrap_err()
            .contains("uncertain")
    );
    assert_eq!(events.recv().await.unwrap()["text"], "partial");
    let request = rx.recv().unwrap();
    assert!(request.starts_with("POST /v1/chat/stream"));
    assert!(rx.try_recv().is_err());
    thread.join().unwrap();
}

#[tokio::test]
async fn done_and_unknown_events_work_and_signin_returns_session() {
    let (client, _, thread) = server(
        "200 OK",
        "data: {\"type\":\"future\"}\n\ndata: {\"type\":\"done\",\"reply\":{\"text\":\"Hello\"}}\n\n",
        "Content-Type: text/event-stream\r\n",
    );
    let (tx, mut events) = tokio::sync::mpsc::channel(4);
    client.chat("Hello".into(), tx).await.unwrap();
    assert_eq!(events.recv().await.unwrap()["type"], "future");
    assert_eq!(events.recv().await.unwrap()["type"], "done");
    thread.join().unwrap();
    let (client, rx, thread) = server(
        "200 OK",
        "{\"token\":\"fictional-new-session\"}",
        "Content-Type: application/json\r\n",
    );
    assert_eq!(
        client
            .sign_in("fictional-password".into(), "000000".into())
            .await
            .unwrap(),
        "fictional-new-session"
    );
    assert!(rx.recv().unwrap().starts_with("POST /v1/session"));
    thread.join().unwrap();
}

#[tokio::test]
async fn real_server_error_shape_is_understood() {
    for (status, body, expected) in [
        (
            "400 Bad Request",
            r#"{"error":"Home console: unconfigured","code":null}"#,
            "private server setup",
        ),
        (
            "400 Bad Request",
            r#"{"error":"Home console: upstream","code":null}"#,
            "Home service",
        ),
        (
            "409 Conflict",
            r#"{"error":"Home console: request_conflict","code":null}"#,
            "already used",
        ),
        (
            "500 Internal Server Error",
            r#"{"error":"internal error","code":null}"#,
            "(500)",
        ),
    ] {
        let (client, _, thread) = server(status, body, "");
        let error = client.get("/v1/terminal/containers").await.unwrap_err();
        assert!(error.contains(expected), "{status}: {error}");
        thread.join().unwrap();
    }
}
