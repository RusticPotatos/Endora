//! What a Claude Code turn's sandbox says its own tools did (ADR 0117): Claude Code's
//! `PostToolUse` hook, in the workbench, posts each call of Bash, Edit, Read and the rest here
//! with the turn's token, and the node tells HQ, which keeps it as a receipt marked
//! **sandbox-reported**. The node did not witness these calls (they ran in the sandbox), so a
//! receipt of one may be cited for what the sandbox did, never as a reading of the lab now: the
//! evidence rule holds a live criterion to a node-witnessed reading.
//!
//! Only a running turn HQ says Claude Code runs is heard; the team's own tools
//! (`mcp__endora__*`) are left out, since the node and HQ record those themselves as they
//! answer them. What is kept is short and scrubbed of secrets and of the turn's own token.
//!
//! Beside it, what every turn's worker says of the whole turn (ADR 0129): its conversation as
//! text, told at the turn's end with the turn's token. The node scrubs it with the same scrubber
//! as the receipts, holds it to a megabyte and tells HQ, which keeps it for a while.

use super::*;
use axum::body::Bytes;
use axum::http::HeaderMap;

use super::relay::{token_of, turn_of};

/// The tools that change the sandbox's files: a receipt of one acts. A command is counted as
/// a look, whatever it did: the sandbox is disposable, and only a branch submitted to the node
/// leaves it.
const CHANGES: [&str; 3] = ["Edit", "Write", "NotebookEdit"];

fn refuse(status: StatusCode, message: &str) -> Response {
    (status, Json(json!({ "error": message }))).into_response()
}

/// A value as one line of text: a string as it is, anything else as compact JSON.
fn text_of(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => String::new(),
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

/// What a call was asked, in short: a command, a file or a pattern, by its tool.
fn args_of(tool: &str, input: &serde_json::Value) -> String {
    let field = |k: &str| input[k].as_str().map(str::to_owned);
    let main = match tool {
        "Bash" => field("command"),
        "Read" | "Edit" | "Write" | "NotebookEdit" => field("file_path"),
        "Glob" | "Grep" => field("pattern"),
        _ => None,
    };
    main.unwrap_or_else(|| text_of(input))
}

/// What a call gave back, in short: a command's output, or the tool's answer.
fn excerpt_of(response: &serde_json::Value) -> String {
    let stdout = response["stdout"].as_str().unwrap_or("");
    let stderr = response["stderr"].as_str().unwrap_or("");
    if !stdout.is_empty() || !stderr.is_empty() {
        return format!("{stdout} {stderr}").trim().to_owned();
    }
    text_of(response)
}

/// `POST /team-relay/receipt`: one call of a Claude Code turn's own tools, as its hook said
/// it. Answered with the receipt's id, or `skipped` for the team's own tools.
pub(super) async fn sandbox_receipt(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(hq) = state.hq.clone() else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "HQ is not set up on this node",
        );
    };
    let token = token_of(&headers);
    let turn = match turn_of(hq.clone(), token.clone(), relay_wording).await {
        Ok(turn) => turn,
        Err(refused) => return refused,
    };
    if !turn.claude_code {
        return refuse(
            StatusCode::FORBIDDEN,
            "only a Claude Code turn's sandbox reports its own tools",
        );
    }
    let Ok(said) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return refuse(StatusCode::BAD_REQUEST, "the body is not JSON");
    };
    let tool = said["tool_name"].as_str().unwrap_or("").trim().to_owned();
    if tool.is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "a report names its tool_name");
    }
    if tool.starts_with("mcp__") {
        return Json(json!({ "skipped": "the team's own tools are recorded as they answer" }))
            .into_response();
    }
    let hidden = |text: String| -> String {
        let text = endora_capabilities::scrub_secrets(&text);
        if token.is_empty() {
            return text;
        }
        text.replace(&token, "[the turn's token]")
    };
    let failed = said["hook_event_name"] == "PostToolUseFailure"
        || said["tool_response"]["interrupted"] == true
        || said["tool_response"]["is_error"] == true;
    let excerpt = if said["error"].is_null() {
        excerpt_of(&said["tool_response"])
    } else {
        text_of(&said["error"])
    };
    let reported = json!({
        "token": token,
        "tool": tool,
        "args": hidden(args_of(&tool, &said["tool_input"])).chars().take(400).collect::<String>(),
        "excerpt": hidden(excerpt).chars().take(1_000).collect::<String>(),
        "ok": !failed,
        "act": CHANGES.contains(&tool.as_str()),
        "confidential": false,
        "sandbox": true,
    });
    let kept = tokio::task::spawn_blocking(move || hq.post("/v1/team/receipt", &reported)).await;
    match kept {
        Ok(Ok(answer)) => Json(json!({ "receipt": answer["receipt"] })).into_response(),
        Ok(Err(endora_infrastructure::HqError::Answered { status: 404, .. })) => {
            refuse(StatusCode::UNAUTHORIZED, "no running turn has that token")
        }
        _ => refuse(StatusCode::BAD_GATEWAY, "HQ kept no receipt"),
    }
}

/// The most of a turn's conversation the node passes on to HQ: a megabyte, its head and its
/// tail (ADR 0129). HQ holds the same line when it keeps it; the node holds it first so that
/// nothing larger is ever sent.
const CONVERSATION_MOST: usize = 1024 * 1024;

/// `text` held to `most` bytes: its head and its tail, with a line where the middle was cut.
fn held_to(text: &str, most: usize) -> String {
    if text.len() <= most {
        return text.to_owned();
    }
    let mark = |removed: usize| {
        format!(
            "\n[... cut by the node: {removed} bytes removed from the middle of this conversation ...]\n"
        )
    };
    let room = most.saturating_sub(mark(text.len()).len());
    let mut head = (room / 2).min(text.len());
    while !text.is_char_boundary(head) {
        head -= 1;
    }
    let mut tail = text.len().saturating_sub(room - room / 2);
    while !text.is_char_boundary(tail) {
        tail += 1;
    }
    format!("{}{}{}", &text[..head], mark(tail - head), &text[tail..])
}

/// `POST /team-relay/transcript`: what a turn said, told by its own worker at the end of the
/// turn, with the turn's token (ADR 0129). The node scrubs it of secrets with the scrubber the
/// receipts use, and of the turn's own token, holds it to a megabyte (head and tail, the cut
/// marked), and tells HQ, which keeps it for thirty days. Any running turn's worker may tell
/// its own; HQ keeps one a turn.
pub(super) async fn turn_transcript(
    State(state): State<AppState>,
    headers: HeaderMap,
    body: Bytes,
) -> Response {
    let Some(hq) = state.hq.clone() else {
        return refuse(
            StatusCode::SERVICE_UNAVAILABLE,
            "HQ is not set up on this node",
        );
    };
    let token = token_of(&headers);
    if let Err(refused) = turn_of(hq.clone(), token.clone(), relay_wording).await {
        return refused;
    }
    let Ok(said) = serde_json::from_slice::<serde_json::Value>(&body) else {
        return refuse(StatusCode::BAD_REQUEST, "the body is not JSON");
    };
    let text = said["text"].as_str().unwrap_or("");
    if text.trim().is_empty() {
        return refuse(StatusCode::BAD_REQUEST, "a conversation says something");
    }
    let mut text = endora_capabilities::scrub_secrets(text);
    if !token.is_empty() {
        text = text.replace(&token, "[the turn's token]");
    }
    let told = json!({
        "token": token,
        "runtime": said["runtime"].as_str().unwrap_or("worker"),
        "text": held_to(&text, CONVERSATION_MOST),
    });
    let kept = tokio::task::spawn_blocking(move || hq.post("/v1/team/transcript", &told)).await;
    match kept {
        Ok(Ok(answer)) => Json(answer).into_response(),
        Ok(Err(endora_infrastructure::HqError::Answered { status: 404, .. })) => {
            refuse(StatusCode::UNAUTHORIZED, "no running turn has that token")
        }
        Ok(Err(endora_infrastructure::HqError::Answered {
            status: 400,
            message,
        })) => refuse(StatusCode::CONFLICT, &message),
        _ => refuse(StatusCode::BAD_GATEWAY, "HQ kept no conversation"),
    }
}

/// How this route words a refusal of the token: as the rest of the team listener does.
fn relay_wording(status: StatusCode, _kind: &str, message: &str) -> Response {
    refuse(status, message)
}

#[cfg(test)]
mod tests {
    use super::super::relay::tests::{fake, node};
    use super::*;
    use axum::body::Body;
    use http_body_util::BodyExt as _;
    use tower::ServiceExt as _;

    fn report(token: Option<&str>, body: &serde_json::Value) -> axum::http::Request<Body> {
        let mut req = axum::http::Request::builder()
            .method("POST")
            .uri("/team-relay/receipt")
            .header("content-type", "application/json");
        if let Some(token) = token {
            req = req.header("authorization", format!("Bearer {token}"));
        }
        req.body(Body::from(body.to_string())).unwrap()
    }

    async fn answer(
        state: &AppState,
        req: axum::http::Request<Body>,
    ) -> (StatusCode, serde_json::Value) {
        let res = team_relay(state.clone()).oneshot(req).await.unwrap();
        let status = res.status();
        let body = res.into_body().collect().await.unwrap().to_bytes();
        (
            status,
            serde_json::from_slice(&body).unwrap_or(serde_json::Value::Null),
        )
    }

    fn tell_transcript(token: Option<&str>, body: &serde_json::Value) -> axum::http::Request<Body> {
        let mut req = axum::http::Request::builder()
            .method("POST")
            .uri("/team-relay/transcript")
            .header("content-type", "application/json");
        if let Some(token) = token {
            req = req.header("authorization", format!("Bearer {token}"));
        }
        req.body(Body::from(body.to_string())).unwrap()
    }

    #[test]
    fn a_long_conversation_is_held_to_its_head_and_tail_with_the_cut_marked() {
        assert_eq!(held_to("short", 100), "short");
        let long = format!("HEAD{}TAIL{}", "é".repeat(50_000), "x".repeat(10));
        let held = held_to(&long, 4_000);
        assert!(held.len() <= 4_000, "{}", held.len());
        assert!(held.starts_with("HEAD") && held.ends_with('x'));
        assert!(held.contains("cut by the node"), "{held}");
    }

    #[tokio::test]
    async fn a_turns_worker_tells_its_conversation_scrubbed_of_secrets_and_capped_and_hq_keeps_it()
    {
        let (hq_url, seen) = fake(Arc::new(|path, body| match path {
            "/v1/team/turn" if body.contains("\"tok\"") => (
                200,
                "application/json",
                json!({ "run": 7, "member": "Rae", "role": "developer", "left": 1000,
                        "cloud": false, "brain": "local", "runtime": "worker" })
                .to_string(),
            ),
            "/v1/team/turn" => (404, "application/json", "{}".to_owned()),
            "/v1/team/transcript" => (
                200,
                "application/json",
                r#"{"kept":true,"cut":false,"est_tokens":9,"tier":"internal"}"#.to_owned(),
            ),
            _ => (200, "application/json", "{}".to_owned()),
        }));
        let state = node(&hq_url, "http://127.0.0.1:9");
        let talk = json!({ "runtime": "worker", "text":
            "--- assistant ---\nThe key is password=hunter2 and my token is tok, ok." });
        for token in [None, Some("bad")] {
            let (got, _) = answer(&state, tell_transcript(token, &talk)).await;
            assert_eq!(got, StatusCode::UNAUTHORIZED, "{token:?}");
        }
        let (empty, _) = answer(
            &state,
            tell_transcript(Some("tok"), &json!({ "runtime": "worker", "text": "  " })),
        )
        .await;
        assert_eq!(empty, StatusCode::BAD_REQUEST);
        let (status, said) = answer(&state, tell_transcript(Some("tok"), &talk)).await;
        assert_eq!(status, StatusCode::OK, "{said}");
        assert_eq!(said["kept"], true);
        let told: Vec<serde_json::Value> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == "/v1/team/transcript")
            .map(|(_, _, b)| serde_json::from_str(b).unwrap())
            .collect();
        assert_eq!(told.len(), 1, "only a turn that runs has its words told");
        assert_eq!(
            told[0]["token"], "tok",
            "named by the turn's own token, to HQ"
        );
        let text = told[0]["text"].as_str().unwrap();
        assert!(!text.contains("hunter2"), "scrubbed: {text}");
        assert!(
            !text.contains("is tok,"),
            "the turn's own token is hidden: {text}"
        );
        assert!(text.contains("[the turn's token]"), "{text}");
        // A conversation over a megabyte reaches HQ held to it, head and tail.
        let huge = format!("START{}END", "line of talk\n".repeat(200_000));
        let (status, _) = answer(
            &state,
            tell_transcript(Some("tok"), &json!({ "runtime": "worker", "text": huge })),
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let sent: Vec<serde_json::Value> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == "/v1/team/transcript")
            .map(|(_, _, b)| serde_json::from_str(b).unwrap())
            .collect();
        let text = sent[1]["text"].as_str().unwrap();
        assert!(text.len() <= CONVERSATION_MOST, "{}", text.len());
        assert!(
            text.starts_with("START") && text.ends_with("END") && text.contains("cut by the node")
        );
    }

    #[tokio::test]
    async fn a_claude_code_turns_hook_reports_its_tools_as_sandbox_receipts_and_nothing_else_does()
    {
        let (hq_url, seen) = fake(Arc::new(|path, body| match path {
            "/v1/team/turn" if body.contains("\"cc\"") => (
                200,
                "application/json",
                json!({ "run": 7, "member": "Rae", "role": "developer", "left": 1000,
                        "cloud": true, "brain": "claude-haiku-5-5", "runtime": "claude-code" })
                .to_string(),
            ),
            "/v1/team/turn" if body.contains("\"worker\"") => (
                200,
                "application/json",
                json!({ "run": 8, "member": "Zed", "role": "developer", "left": 1000,
                        "cloud": true, "brain": "claude-haiku-5-5", "runtime": "worker" })
                .to_string(),
            ),
            "/v1/team/turn" => (404, "application/json", "{}".to_owned()),
            "/v1/team/receipt" => (200, "application/json", r#"{"receipt":52}"#.to_owned()),
            _ => (200, "application/json", "{}".to_owned()),
        }));
        let state = node(&hq_url, "http://127.0.0.1:9");
        let bash = json!({
            "session_id": "s", "hook_event_name": "PostToolUse", "tool_name": "Bash",
            "tool_input": { "command": "git log --oneline -1 # cc", "description": "look" },
            "tool_response": { "stdout": "abc123 tidy (token cc) password=hunter2", "stderr": "",
                               "interrupted": false },
            "tool_use_id": "toolu_1", "duration_ms": 40,
        });
        for (token, status) in [
            (None, StatusCode::UNAUTHORIZED),
            (Some("bad"), StatusCode::UNAUTHORIZED),
            (Some("worker"), StatusCode::FORBIDDEN),
        ] {
            let (got, _) = answer(&state, report(token, &bash)).await;
            assert_eq!(got, status, "{token:?}");
        }
        let (status, said) = answer(&state, report(Some("cc"), &bash)).await;
        assert_eq!(status, StatusCode::OK, "{said}");
        assert_eq!(said["receipt"], 52);
        let edit = json!({ "hook_event_name": "PostToolUse", "tool_name": "Edit",
            "tool_input": { "file_path": "/work/repo/roles/x/tasks/main.yml", "old_string": "a", "new_string": "b" },
            "tool_response": { "filePath": "/work/repo/roles/x/tasks/main.yml" } });
        answer(&state, report(Some("cc"), &edit)).await;
        let failed = json!({ "hook_event_name": "PostToolUseFailure", "tool_name": "Bash",
            "tool_input": { "command": "ansible-lint" }, "error": "exit 2" });
        answer(&state, report(Some("cc"), &failed)).await;
        let (_, skipped) = answer(
            &state,
            report(
                Some("cc"),
                &json!({ "tool_name": "mcp__endora__post",
            "tool_input": {}, "tool_response": {} }),
            ),
        )
        .await;
        assert!(skipped["skipped"].is_string(), "{skipped}");
        let told: Vec<serde_json::Value> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == "/v1/team/receipt")
            .map(|(_, _, b)| serde_json::from_str(b).unwrap())
            .collect();
        assert_eq!(told.len(), 3, "the team's own tool is not told twice");
        assert_eq!(
            (
                &told[0]["tool"],
                &told[0]["sandbox"],
                &told[0]["ok"],
                &told[0]["act"]
            ),
            (&json!("Bash"), &json!(true), &json!(true), &json!(false))
        );
        assert_eq!(
            told[0]["token"], "cc",
            "named by the turn's own token, to HQ"
        );
        let args = told[0]["args"].as_str().unwrap();
        let excerpt = told[0]["excerpt"].as_str().unwrap();
        assert!(args.starts_with("git log --oneline -1"), "{args}");
        assert!(
            !args.contains(" cc") && !excerpt.contains(" cc)"),
            "no token: {args} / {excerpt}"
        );
        assert!(!excerpt.contains("hunter2"), "scrubbed: {excerpt}");
        assert_eq!(
            (&told[1]["tool"], &told[1]["args"], &told[1]["act"]),
            (
                &json!("Edit"),
                &json!("/work/repo/roles/x/tasks/main.yml"),
                &json!(true)
            )
        );
        assert_eq!(
            (&told[2]["ok"], &told[2]["excerpt"]),
            (&json!(false), &json!("exit 2"))
        );
    }
}
