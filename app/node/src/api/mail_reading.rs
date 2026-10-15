//! The node reading the mail (ADR 0097), end to end: a temporary Maildir copy, the real store
//! and worker tick, and a stand-in for the local model.
use super::AppState;
use endora_application::{
    ChatMessage, ChatRepository, Clock, EventLog, IdSource, MessageId, MessageRole, Timestamp,
};
use endora_infrastructure::{RandomIdSource, ScriptedButler, SqliteStore, SystemClock};
use std::sync::{Arc, Mutex};

const DAY: i64 = 24 * 60 * 60 * 1000;

/// The local model, as far as these tests need it: a canned reply, and a count of calls.
struct Reader {
    reply: &'static str,
    calls: Mutex<usize>,
}
impl endora_application::UntrustedReader for Reader {
    fn read(&self, _i: &str, _d: &str) -> Result<String, endora_application::ProposalError> {
        *self.calls.lock().unwrap() += 1;
        Ok(self.reply.to_owned())
    }
}

const BILL: &str = r#"{"bucket":"money","sentence":"A dental bill for the October visit.","money_due":true,"amount":180,"dates":[]}"#;

struct Copy(std::path::PathBuf);
impl Drop for Copy {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn email(from: &str, at_ms: i64, id: &str) -> String {
    let secs = at_ms / 1000;
    // An RFC 2822 date the parser accepts, from the timestamp (UTC).
    let days = secs.div_euclid(86_400);
    let rem = secs.rem_euclid(86_400);
    // Civil date from days (Hinnant).
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    let months = [
        "Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec",
    ];
    format!(
        "Delivered-To: john@example.com\r\nAuthentication-Results: mx.example.com; dkim=pass header.i=@example.net; dmarc=pass header.from=example.net\r\nFrom: {from}\r\nTo: john@example.com\r\nSubject: s\r\nDate: {d} {} {y} {:02}:{:02}:00 +0000\r\nMessage-ID: {id}\r\n\r\nPlease pay.\r\n",
        months[usize::try_from(m - 1).unwrap()],
        rem / 3600,
        rem % 3600 / 60
    )
}

fn state_with_mail(name: &str, reply: &'static str) -> (AppState, Arc<Reader>, Copy) {
    let root = std::env::temp_dir().join(format!("endora-node-mail-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&root);
    std::fs::create_dir_all(root.join("personal/new")).unwrap();
    let state = AppState::new(
        Arc::new(SqliteStore::open_in_memory().unwrap()),
        Arc::new(RandomIdSource),
        Arc::new(SystemClock),
        Arc::new(ScriptedButler),
    );
    let now = state.clock.now().unix_millis();
    for n in 0..3 {
        std::fs::write(
            root.join(format!("personal/new/h{n}")),
            email(
                "shop@example.net",
                now - 200 * DAY,
                &format!("<h{n}@example.net>"),
            ),
        )
        .unwrap();
    }
    std::fs::write(
        root.join("personal/new/bill"),
        email(
            "Example Dental <billing@example.net>",
            now - 60 * 60 * 1000,
            "<bill@example.net>",
        ),
    )
    .unwrap();
    let reader = Arc::new(Reader {
        reply,
        calls: Mutex::new(0),
    });
    let state = state.with_mail(endora_mail::MaildirCopy::new(&root), reader.clone());
    (state, reader, Copy(root))
}

fn trail(state: &AppState) -> Vec<String> {
    state
        .events
        .recent(50)
        .unwrap()
        .into_iter()
        .map(|e| e.summary)
        .collect()
}

#[tokio::test]
async fn new_mail_that_matters_is_read_in_quiet_time_and_said_in_the_trail() {
    let (state, reader, _copy) = state_with_mail("reads", BILL);
    let read = super::workers::read_mail_once(&state)
        .await
        .expect("one message read");
    assert_eq!(read.bucket, endora_mail::Bucket::Money);
    assert_eq!(*reader.calls.lock().unwrap(), 1);
    assert!(
        trail(&state)
            .iter()
            .any(|l| l.starts_with("Mail worth a look: Example Dental: A dental bill")),
        "{:?}",
        trail(&state)
    );
    // The old mail is indexed and never read.
    assert!(super::workers::read_mail_once(&state).await.is_none());
    assert_eq!(*reader.calls.lock().unwrap(), 1);
}

#[tokio::test]
async fn nothing_is_read_while_the_person_is_talking() {
    let (state, reader, _copy) = state_with_mail("talking", BILL);
    let said = ChatMessage::new(
        MessageId::new(state.ids.new_id()),
        MessageRole::User,
        "hello",
        Timestamp::from_unix_millis(state.clock.now().unix_millis() - 60_000),
    )
    .unwrap();
    state.chat.append(&said).unwrap();
    assert!(super::workers::read_mail_once(&state).await.is_none());
    assert_eq!(*reader.calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn nothing_is_read_while_a_chat_turn_runs() {
    let (state, reader, _copy) = state_with_mail("turn", BILL);
    let _turn = state.turn_lock.clone().try_lock_owned().unwrap();
    assert!(super::workers::read_mail_once(&state).await.is_none());
    assert_eq!(*reader.calls.lock().unwrap(), 0);
}

#[tokio::test]
async fn the_console_sees_what_matters_and_the_buckets_never_a_body() {
    let (state, _reader, _copy) = state_with_mail("console", BILL);
    super::workers::read_mail_once(&state).await;
    let axum::Json(body) = super::mail::mail_digest(axum::extract::State(state.clone()))
        .await
        .unwrap_or_else(|_| panic!("the endpoint failed"));
    assert_eq!(body["mounted"], true);
    assert_eq!(body["indexed"], 4);
    assert_eq!(body["important"][0]["from"], "Example Dental");
    assert_eq!(body["important"][0]["amount"], "180.00");
    assert_eq!(body["important"][0]["why"][0], "asks for money");
    let money = body["buckets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["name"] == "money")
        .unwrap();
    assert_eq!(money["count"], 1);
    assert!(
        !body.to_string().contains("Please pay"),
        "a body reached the console"
    );
}

#[test]
fn the_brief_gets_a_mail_section_once_mail_has_been_read() {
    let (state, _reader, _copy) = state_with_mail("brief", BILL);
    assert!(super::workers::mail_brief_lines(&state).is_empty());
    let rt = tokio::runtime::Runtime::new().unwrap();
    rt.block_on(super::workers::read_mail_once(&state));
    let lines = super::workers::mail_brief_lines(&state);
    assert_eq!(lines[0], "Mail: 1 worth a look since yesterday.");
}
