//! Sonarr's and Radarr's queues say why an item is stuck, and Endora can ask either app to
//! try its imports again, or take a stuck item out, blocklist it and search for another (ADR
//! 0108). Fictional hosts, keys and releases throughout.
use endora_capabilities::infrastructure::{
    Capability, CapabilitySettings, arr::ARR_APPS, default_capabilities,
};
use endora_capabilities::tiers::tier_of;
use endora_capabilities::{Stance, default_stance};
use endora_kernel::{Reversibility, Tier};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

/// A fake *arr app: answers each request by its method and path (without the query), and
/// records each request's first line and body.
struct App {
    origin: String,
    seen: Arc<Mutex<Vec<(String, String)>>>,
}

fn app(answers: Vec<(&'static str, &'static str, u16, Value)>) -> App {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut socket) = stream else { break };
            // The head, then as much body as it says it has: a body may come in a second write.
            let mut data = Vec::new();
            let mut chunk = vec![0; 16384];
            loop {
                let n = socket.read(&mut chunk).unwrap_or(0);
                data.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&data).to_lowercase();
                let Some((head, body)) = text.split_once("\r\n\r\n") else {
                    if n == 0 {
                        break;
                    }
                    continue;
                };
                let length: usize = head
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length: "))
                    .and_then(|v| v.trim().parse().ok())
                    .unwrap_or(0);
                if n == 0 || body.len() >= length {
                    break;
                }
            }
            let request = String::from_utf8_lossy(&data).into_owned();
            let first = request.lines().next().unwrap_or("").to_owned();
            let body = request
                .split_once("\r\n\r\n")
                .map_or("", |(_, b)| b)
                .to_owned();
            log.lock().unwrap().push((first.clone(), body));
            let mut parts = first.split(' ');
            let method = parts.next().unwrap_or("");
            let path = parts.next().unwrap_or("").split('?').next().unwrap_or("");
            let (status, reply) = answers
                .iter()
                .find(|(m, p, _, _)| *m == method && *p == path)
                .map_or((404, json!({})), |(_, _, s, r)| (*s, r.clone()));
            let reply = reply.to_string();
            let _ = write!(
                socket,
                "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                reply.len()
            );
        }
    });
    App { origin, seen }
}

fn settings(origin: &str) -> CapabilitySettings {
    [("origin", origin), ("api_key", "fictional-key-123")]
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect::<BTreeMap<_, _>>()
}

fn skill(id: &str) -> Arc<dyn Capability> {
    default_capabilities()
        .into_iter()
        .find(|c| c.info().id == id)
        .unwrap_or_else(|| panic!("no capability {id:?} is registered"))
}

/// A queue with one download going well and one finished but stuck, as Sonarr says it.
fn queue() -> Value {
    json!({"totalRecords": 2, "records": [
        {"id": 101, "title": "Some.Show.S01E01.1080p", "size": 1000.0, "sizeleft": 250.0,
         "status": "downloading", "trackedDownloadStatus": "ok", "trackedDownloadState": "downloading",
         "statusMessages": []},
        {"id": 102, "title": "Some.Show.S01E02.1080p", "size": 500.0, "sizeleft": 0.0,
         "status": "completed", "trackedDownloadStatus": "warning", "trackedDownloadState": "importBlocked",
         "downloadId": "ABCDEF", "outputPath": "/downloads/Some.Show.S01E02.1080p",
         "statusMessages": [{"title": "Some.Show.S01E02.1080p.mkv",
           "messages": ["No files found are eligible for import", "\u{1b}[31mSample\u{1b}[0m"]}]}
    ]})
}

#[test]
fn the_queue_says_why_an_item_is_stuck_in_the_apps_own_words() {
    let sonarr = app(vec![("GET", "/api/v3/queue", 200, queue())]);
    let out = skill("sonarr_queue")
        .invoke(&json!({}), &settings(&sonarr.origin))
        .expect("reads");
    let stuck = &out["items"][1];
    assert_eq!(stuck["item"], 102, "{out}");
    assert_eq!(stuck["tracked"], "importBlocked");
    assert_eq!(stuck["stuck"], true);
    assert_eq!(
        stuck["why"][0],
        "Some.Show.S01E02.1080p.mkv: No files found are eligible for import"
    );
    assert!(
        !out.to_string().contains('\u{1b}'),
        "no control characters reach a model: {out}"
    );
    assert!(
        !out.to_string().contains("ABCDEF"),
        "the download client's id stays home: {out}"
    );
    let said = skill("sonarr_queue").summarize(&out);
    assert!(
        said.contains("Some.Show.S01E01.1080p (item 101): 75%"),
        "{said}"
    );
    assert!(
        said.contains("Some.Show.S01E02.1080p (item 102): 100% (completed, import blocked)"),
        "{said}"
    );
    assert!(
        said.contains("Stuck: Some.Show.S01E02.1080p.mkv: No files found are eligible for import"),
        "{said}"
    );
}

#[test]
fn retrying_imports_asks_the_app_to_check_its_downloads_and_changes_nothing_else() {
    let radarr = app(vec![(
        "POST",
        "/api/v3/command",
        201,
        json!({"id": 9, "name": "RefreshMonitoredDownloads", "status": "queued"}),
    )]);
    let retry = skill("radarr_retry_import");
    let out = retry
        .invoke(&json!({}), &settings(&radarr.origin))
        .expect("asked");
    let seen = radarr.seen.lock().unwrap().clone();
    assert_eq!(seen.len(), 1, "one request, and nothing removed: {seen:?}");
    assert!(seen[0].0.starts_with("POST /api/v3/command "), "{seen:?}");
    assert_eq!(
        serde_json::from_str::<Value>(&seen[0].1).unwrap(),
        json!({"name": "RefreshMonitoredDownloads"})
    );
    let said = retry.summarize(&out);
    assert!(
        said.starts_with("Radarr is checking its downloads again"),
        "{said}"
    );
}

#[test]
fn removing_an_item_blocklists_it_and_searches_again_only_for_an_item_in_the_queue() {
    let sonarr = app(vec![
        ("GET", "/api/v3/queue", 200, queue()),
        ("DELETE", "/api/v3/queue/102", 200, json!({})),
    ]);
    let remove = skill("sonarr_remove_and_search");
    let out = remove
        .invoke(&json!({"item": 102}), &settings(&sonarr.origin))
        .expect("removed");
    let seen = sonarr.seen.lock().unwrap().clone();
    let deleted = &seen.last().unwrap().0;
    assert!(
        deleted.starts_with(
            "DELETE /api/v3/queue/102?removeFromClient=true&blocklist=true&skipRedownload=false "
        ),
        "{deleted}"
    );
    let said = remove.summarize(&out);
    assert!(
        said.contains("Some.Show.S01E02.1080p") && said.contains("searching for another"),
        "{said}"
    );

    // An item the queue does not hold is refused, and nothing is deleted.
    let before = sonarr.seen.lock().unwrap().len();
    for asked in [json!({"item": 999}), json!({"item": "102/../1"}), json!({})] {
        assert!(
            remove.invoke(&asked, &settings(&sonarr.origin)).is_err(),
            "{asked}"
        );
    }
    let after = sonarr.seen.lock().unwrap().clone();
    assert!(
        after[before..]
            .iter()
            .all(|(line, _)| line.starts_with("GET ")),
        "{after:?}"
    );
}

#[test]
fn each_app_is_a_row_of_configuration_with_a_read_and_two_acts_at_their_stances() {
    assert_eq!(
        ARR_APPS.iter().map(|a| a.name).collect::<Vec<_>>(),
        ["Sonarr", "Radarr"]
    );
    for app in &ARR_APPS {
        let read = skill(app.queue).info();
        let retry = skill(app.retry).info();
        let remove = skill(app.remove).info();
        assert_eq!(read.reversibility, Reversibility::Observe);
        // Retrying an import runs on its own (on); taking an item out asks the owner (ask).
        assert_eq!(
            default_stance(retry.reversibility),
            Stance::Auto,
            "{}",
            app.retry
        );
        assert_eq!(
            default_stance(remove.reversibility),
            Stance::Ask,
            "{}",
            app.remove
        );
        for info in [&read, &retry, &remove] {
            assert_eq!(info.borrows_from, Some(app.lender), "{}", info.id);
            assert!(info.settings.is_empty(), "{}", info.id);
            assert_eq!(tier_of(info.id), Tier::Confidential, "{}", info.id);
        }
        assert!(
            retry.input_schema.is_none(),
            "the model gives a retry nothing"
        );
        let schema: Value = serde_json::from_str(remove.input_schema.unwrap()).unwrap();
        assert_eq!(schema["required"], json!(["item"]));
    }
}
