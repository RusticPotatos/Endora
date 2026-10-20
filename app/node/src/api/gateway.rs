//! The team gateway (ADR 0110): HQ's two doors for callers on the house's network, passed on
//! by the node's team listener. HQ stays unpublished; a sandbox on the Pi reaches the team protocol here (`/team-gateway/mcp`, opened by its turn's
//! token), and a runner on the Pi asks for turns here (`/team-gateway/runner/...`, signed with
//! the runner's token). The node checks neither token: it passes the request, its method, body
//! and `Authorization` untouched, to HQ's own URL and nowhere else, and HQ decides. The node's
//! own token for HQ is never added.

use super::*;
use axum::body::Body;
use endora_infrastructure::{HqError, Passing};

/// The largest request passed on: a turn's MCP call, or the runner's report, is far smaller.
const LARGEST_REQUEST: usize = 4 * 1024 * 1024;

/// The largest answer passed back.
const LARGEST_ANSWER: u64 = 8 * 1024 * 1024;

fn refuse(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

/// `/team-gateway/mcp`, passed on to HQ's `/v1/team/mcp`.
pub(super) async fn mcp(State(state): State<AppState>, request: Request) -> Response {
    pass(state, "/v1/team/mcp".to_owned(), request).await
}

/// `/team-gateway/runner/<route>`, passed on to HQ's `/v1/runner/<route>`. Only a plain route
/// passes: segments of letters, digits, `-` and `_`, so nothing can climb out of
/// `/v1/runner/`.
pub(super) async fn runner(State(state): State<AppState>, request: Request) -> Response {
    let route = request
        .uri()
        .path()
        .strip_prefix("/team-gateway/runner/")
        .unwrap_or("");
    let plain = !route.is_empty()
        && route.split('/').all(|segment| {
            !segment.is_empty()
                && segment
                    .bytes()
                    .all(|b| b.is_ascii_alphanumeric() || b == b'-' || b == b'_')
        });
    if !plain {
        return refuse(StatusCode::NOT_FOUND, "no such route");
    }
    let path = format!("/v1/runner/{route}");
    pass(state, path, request).await
}

/// Passes `request` on to `path` on HQ, and HQ's answer back.
async fn pass(state: AppState, path: String, request: Request) -> Response {
    let method = request.method().clone();
    if method != Method::GET && method != Method::POST {
        return refuse(
            StatusCode::METHOD_NOT_ALLOWED,
            "the team gateway passes on GET and POST",
        );
    }
    let Some(hq) = state.hq.clone() else {
        return refuse(StatusCode::SERVICE_UNAVAILABLE, "this node has no HQ");
    };
    // In a block of its own: a body is not `Sync`, so nothing may borrow the request across an
    // `.await`.
    let headers = {
        let header = |name: &str| {
            request
                .headers()
                .get(name)
                .and_then(|v| v.to_str().ok())
                .map(str::to_owned)
        };
        Passing {
            authorization: header("authorization"),
            content_type: header("content-type"),
            accept: header("accept"),
        }
    };
    let Ok(body) = axum::body::to_bytes(request.into_body(), LARGEST_REQUEST).await else {
        return refuse(
            StatusCode::PAYLOAD_TOO_LARGE,
            "the request is larger than the team gateway takes",
        );
    };
    let passed = tokio::task::spawn_blocking(move || {
        hq.pass(method.as_str(), &path, &headers, &body, LARGEST_ANSWER)
    })
    .await;
    match passed {
        Ok(Ok(answer)) => {
            let status = StatusCode::from_u16(answer.status).unwrap_or(StatusCode::BAD_GATEWAY);
            let mut response = Response::builder().status(status);
            if let Some(kind) = answer.content_type {
                response = response.header("content-type", kind);
            }
            response
                .body(Body::from(answer.body))
                .unwrap_or_else(|_| StatusCode::BAD_GATEWAY.into_response())
        }
        Ok(Err(HqError::Answered { status, message })) => refuse(
            StatusCode::from_u16(status).unwrap_or(StatusCode::BAD_GATEWAY),
            &message,
        ),
        Ok(Err(HqError::Unreachable(_))) | Err(_) => {
            refuse(StatusCode::BAD_GATEWAY, "HQ did not answer")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use endora_infrastructure::{RandomIdSource, SqliteStore};
    use http_body_util::BodyExt as _;
    use std::io::{Read as _, Write as _};
    use std::sync::Mutex;
    use tower::ServiceExt as _;

    /// What the fake HQ was sent: the request line, the headers (lower-cased) and the body.
    type Seen = Arc<Mutex<Vec<(String, Vec<(String, String)>, Vec<u8>)>>>;

    /// The node's own token for HQ, which nothing passed on may carry.
    const NODES_TOKEN: &str = "the-nodes-own-token-for-hq-0123456789";

    /// HQ, pretending: reads the whole of each request, its headers and then the body its
    /// `Content-Length` promises, before it answers `status` with `reply`, so a body that
    /// arrives in pieces never meets a closed socket.
    fn fake_hq(status: u16, reply: &'static str) -> (String, Seen) {
        fake_hq_saying(status, "", reply)
    }

    /// [`fake_hq`], with `extra` headers (each ending `\r\n`) in its answer.
    fn fake_hq_saying(status: u16, extra: &'static str, reply: &'static str) -> (String, Seen) {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        let seen: Seen = Arc::new(Mutex::new(Vec::new()));
        let kept = seen.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming() {
                let Ok(mut stream) = stream else { continue };
                let mut head = Vec::new();
                let mut byte = [0u8];
                while !head.ends_with(b"\r\n\r\n") {
                    if stream.read_exact(&mut byte).is_err() {
                        break;
                    }
                    head.push(byte[0]);
                }
                let head = String::from_utf8_lossy(&head).into_owned();
                let mut lines = head.lines();
                let asked = lines
                    .next()
                    .unwrap_or("")
                    .rsplit_once(' ')
                    .map_or(String::new(), |(a, _)| a.to_owned());
                let headers: Vec<(String, String)> = lines
                    .filter_map(|l| l.split_once(": "))
                    .map(|(k, v)| (k.to_ascii_lowercase(), v.to_owned()))
                    .collect();
                let length: usize = headers
                    .iter()
                    .find(|(k, _)| k == "content-length")
                    .and_then(|(_, v)| v.parse().ok())
                    .unwrap_or(0);
                let mut body = vec![0u8; length];
                let _ = stream.read_exact(&mut body);
                kept.lock().unwrap().push((asked, headers, body));
                let _ = write!(
                    stream,
                    "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\n{extra}Content-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                    reply.len()
                );
            }
        });
        (url, seen)
    }

    fn node(hq_url: Option<&str>) -> AppState {
        let state = AppState::new(
            Arc::new(SqliteStore::open_in_memory().unwrap()),
            Arc::new(RandomIdSource),
            Arc::new(SystemClock),
        );
        match hq_url {
            Some(url) => state.with_hq(endora_infrastructure::HqClient::new(url, NODES_TOKEN)),
            None => state,
        }
    }

    fn call(method: &str, uri: &str, token: &str, body: Vec<u8>) -> axum::http::Request<Body> {
        axum::http::Request::builder()
            .method(method)
            .uri(uri)
            .header("content-type", "application/json")
            .header("accept", "application/json, text/event-stream")
            .header("authorization", format!("Bearer {token}"))
            .header("cookie", "a=b")
            .body(Body::from(body))
            .unwrap()
    }

    async fn text(res: Response) -> String {
        String::from_utf8(res.into_body().collect().await.unwrap().to_bytes().to_vec()).unwrap()
    }

    fn header<'a>(headers: &'a [(String, String)], name: &str) -> Option<&'a str> {
        headers
            .iter()
            .find(|(k, _)| k == name)
            .map(|(_, v)| v.as_str())
    }

    #[tokio::test]
    async fn a_turns_mcp_call_reaches_hq_whole_with_its_own_token_and_hqs_answer_comes_back() {
        let (url, seen) = fake_hq(200, r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);
        let app = team_relay(node(Some(&url)));
        // Large enough to arrive in pieces.
        let big = json!({ "jsonrpc": "2.0", "id": 1, "method": "tools/call",
                          "params": { "name": "comment", "arguments": { "text": "a".repeat(600_000) } } })
        .to_string()
        .into_bytes();
        let res = app
            .oneshot(call(
                "POST",
                "/team-gateway/mcp",
                "the-turns-token",
                big.clone(),
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::OK);
        assert_eq!(
            res.headers()["content-type"],
            "application/json",
            "HQ's own content type"
        );
        assert_eq!(text(res).await, r#"{"jsonrpc":"2.0","id":1,"result":{}}"#);

        let seen = seen.lock().unwrap();
        assert_eq!(seen.len(), 1);
        let (asked, headers, body) = &seen[0];
        assert_eq!(asked, "POST /v1/team/mcp");
        assert_eq!(body, &big, "the body untouched, all of it");
        assert_eq!(
            header(headers, "authorization"),
            Some("Bearer the-turns-token")
        );
        assert_eq!(header(headers, "content-type"), Some("application/json"));
        assert_eq!(
            header(headers, "accept"),
            Some("application/json, text/event-stream")
        );
        assert_eq!(
            header(headers, "cookie"),
            None,
            "nothing else of the caller's"
        );
        assert!(
            headers.iter().all(|(_, v)| !v.contains(NODES_TOKEN)),
            "never the node's own token"
        );
    }

    #[tokio::test]
    async fn hqs_refusal_comes_back_as_it_said_it() {
        let (url, _) = fake_hq(401, r#"{"error":"no running turn has that token"}"#);
        let app = team_relay(node(Some(&url)));
        let res = app
            .oneshot(call("POST", "/team-gateway/mcp", "stale", b"{}".to_vec()))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::UNAUTHORIZED);
        assert_eq!(
            text(res).await,
            r#"{"error":"no running turn has that token"}"#
        );
    }

    #[tokio::test]
    async fn a_remote_runners_calls_reach_hqs_runner_routes_with_their_method() {
        let (url, seen) = fake_hq(200, r#"{"runs":[]}"#);
        let app = team_relay(node(Some(&url)));
        for (method, uri, body) in [
            ("GET", "/team-gateway/runner/stops", ""),
            ("POST", "/team-gateway/runner/next", r#"{"free":2}"#),
            (
                "POST",
                "/team-gateway/runner/12/ended",
                r#"{"succeeded":true,"lease":3}"#,
            ),
        ] {
            let res = app
                .clone()
                .oneshot(call(method, uri, "the-runners-token", body.into()))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::OK, "{uri}");
        }
        let seen = seen.lock().unwrap();
        let asked: Vec<(&str, &[u8])> = seen
            .iter()
            .map(|(a, _, b)| (a.as_str(), b.as_slice()))
            .collect();
        assert_eq!(
            asked,
            [
                ("GET /v1/runner/stops", &b""[..]),
                ("POST /v1/runner/next", br#"{"free":2}"#),
                (
                    "POST /v1/runner/12/ended",
                    br#"{"succeeded":true,"lease":3}"#
                ),
            ]
        );
        assert!(
            seen.iter()
                .all(|(_, h, _)| header(h, "authorization") == Some("Bearer the-runners-token"))
        );
    }

    #[tokio::test]
    async fn nothing_climbs_out_of_the_runners_routes_or_reaches_hq_otherwise() {
        let (url, seen) = fake_hq(200, "{}");
        let app = team_relay(node(Some(&url)));
        for (method, uri) in [
            ("POST", "/team-gateway/runner/../team/turn"),
            ("POST", "/team-gateway/runner/%2e%2e/team/turn"),
            ("POST", "/team-gateway/runner/a%2Fb"),
            ("POST", "/team-gateway/runner/"),
            ("POST", "/team-gateway/runner//next"),
            ("POST", "/team-gateway/team/turn"),
            ("POST", "/team-gateway/v1/team/turn"),
            ("POST", "/team-gateway/http://192.168.1.10/"),
        ] {
            let res = app
                .clone()
                .oneshot(call(method, uri, "t", b"{}".to_vec()))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::NOT_FOUND, "{uri}");
        }
        for method in ["PUT", "DELETE", "PATCH"] {
            let res = app
                .clone()
                .oneshot(call(method, "/team-gateway/mcp", "t", b"{}".to_vec()))
                .await
                .unwrap();
            assert_eq!(res.status(), StatusCode::METHOD_NOT_ALLOWED, "{method}");
        }
        let res = app
            .clone()
            .oneshot(call(
                "POST",
                "/team-gateway/mcp",
                "t",
                vec![b'a'; LARGEST_REQUEST + 1],
            ))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::PAYLOAD_TOO_LARGE);
        assert!(seen.lock().unwrap().is_empty(), "HQ was asked nothing");
    }

    #[tokio::test]
    async fn a_redirect_from_hq_is_passed_back_never_followed() {
        let (elsewhere, followed) = fake_hq(200, "{}");
        let location: &'static str =
            Box::leak(format!("Location: {elsewhere}/v1/team/mcp\r\n").into_boxed_str());
        let (url, _) = fake_hq_saying(302, location, "{}");
        let res = team_relay(node(Some(&url)))
            .oneshot(call("POST", "/team-gateway/mcp", "t", b"{}".to_vec()))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::FOUND);
        assert!(followed.lock().unwrap().is_empty(), "nothing but HQ's URL");
    }

    #[tokio::test]
    async fn without_hq_the_gateway_says_so_and_hq_unheard_is_a_bad_gateway() {
        let res = team_relay(node(None))
            .oneshot(call("POST", "/team-gateway/mcp", "t", b"{}".to_vec()))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::SERVICE_UNAVAILABLE);

        // A port nothing listens on.
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let url = format!("http://{}", listener.local_addr().unwrap());
        drop(listener);
        let res = team_relay(node(Some(&url)))
            .oneshot(call("GET", "/team-gateway/runner/stops", "t", Vec::new()))
            .await
            .unwrap();
        assert_eq!(res.status(), StatusCode::BAD_GATEWAY);
    }

    #[tokio::test]
    async fn the_gateway_is_served_only_on_the_team_listener() {
        let (url, seen) = fake_hq(200, "{}");
        let main = super::super::app(node(Some(&url)));
        for (method, uri) in [
            ("POST", "/team-gateway/mcp"),
            ("GET", "/team-gateway/runner/stops"),
        ] {
            let res = main
                .clone()
                .oneshot(call(method, uri, "t", Vec::new()))
                .await
                .unwrap();
            assert_eq!(
                res.status(),
                StatusCode::NOT_FOUND,
                "{uri} on the main port"
            );
        }
        assert!(seen.lock().unwrap().is_empty());
    }
}
