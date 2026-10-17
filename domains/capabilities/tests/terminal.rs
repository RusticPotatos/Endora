//! Explicit administration privacy and at-most-once contracts.
use endora_capabilities::infrastructure::terminal::{Journal, project_home, protected_container};
use endora_persistence::Db;
use serde_json::json;

/// What the handheld reads must fit the protocol (ADR 0099): the node refuses a response that
/// does not, so a builder that drifts from it fails here first.
fn fits<T: serde::de::DeserializeOwned>(value: &serde_json::Value) -> T {
    serde_json::from_value(value.clone())
        .unwrap_or_else(|e| panic!("does not fit the protocol: {e}: {value}"))
}

#[test]
fn mutation_ids_survive_errors_and_conflicting_reuse_is_refused() {
    let db = Db::open_in_memory().unwrap();
    let journal = Journal::new(db);
    let request = json!({"kind":"container","action":"restart","target":"abc","request_id":"fictional-operation","confirmed":true});
    let first = journal.run(&request, || Err("connection"));
    assert_eq!(first.unwrap()["state"], "uncertain");
    let replay = journal.run(&request, || {
        panic!("must never resend an uncertain mutation")
    });
    assert_eq!(replay.unwrap()["state"], "uncertain");
    let mut changed = request;
    changed["action"] = "stop".into();
    assert!(journal.run(&changed, || Ok(json!({}))).is_err());
}

#[test]
fn home_projection_is_allowlisted_and_contains_only_display_fields() {
    let value = json!([{"entity_id":"light.example","state":"on","attributes":{"friendly_name":"Example","secret":"hidden"}},{"entity_id":"sensor.private","state":"42"}]);
    let result = project_home(&value, &["light.example".into()]);
    assert_eq!(result["items"].as_array().unwrap().len(), 1);
    assert!(!result.to_string().contains("hidden"));
    assert!(!result.to_string().contains("sensor.private"));
    assert!(protected_container(&json!({"Names":["/cloudflared"]})));
    assert!(protected_container(&json!({"Names":["/endora-node"]})));
    assert!(!protected_container(&json!({"Names":["/example-app"]})));
    assert!(protected_container(
        &json!({"Names":["/renamed"],"Image":"cloudflare/cloudflared:latest"})
    ));
    assert!(protected_container(
        &json!({"Names":["/renamed"],"Labels":{"com.docker.compose.service":"endora-node"}})
    ));
}

#[test]
fn favorite_order_survives_upstream_state_reordering() {
    let value = json!([{"entity_id":"light.second","state":"off"},{"entity_id":"light.first","state":"on"}]);
    let result = project_home(&value, &["light.first".into(), "light.second".into()]);
    assert_eq!(result["items"][0]["id"], "light.first");
    assert_eq!(result["items"][1]["id"], "light.second");
}

#[test]
fn empty_favorites_offer_catalog_selection_without_observing_the_whole_home() {
    use endora_capabilities::{ConfigStore, application::HomeConsole};
    let db = Db::open_in_memory().unwrap();
    endora_capabilities::migrate(&db).unwrap();
    let adapter = endora_capabilities::infrastructure::terminal::Adapter::new(
        std::sync::Arc::new(ConfigStore::new(db.clone())),
        db,
    );
    let result = adapter.read("home", "").unwrap();
    let _: endora_protocol::ConsoleView = fits(&result);
    assert!(result["summary"].as_str().unwrap().contains("catalog"));
    assert!(result["items"].as_array().unwrap().is_empty());
}

#[test]
fn operation_results_report_readback_and_jobs_without_claiming_job_completion() {
    let journal = Journal::new(Db::open_in_memory().unwrap());
    let action = json!({"request_id":"readback-result","kind":"home","action":"on","target":"light.example","confirmed":true});
    let result = journal
        .run(&action, || Ok(json!({"state":"off","verified":false})))
        .unwrap();
    assert!(result["summary"].as_str().unwrap().contains("not verified"));
    let action = json!({"request_id":"queued-result","kind":"job","action":"run","target":"syntax","confirmed":true});
    let result = journal
        .run(&action, || {
            Ok(json!({"job_id":"queued-result","state":"queued"}))
        })
        .unwrap();
    assert_eq!(result["state"], "queued");
}

#[test]
fn explicit_action_vocabulary_rejects_unconfirmed_and_arbitrary_destinations() {
    use endora_capabilities::application::validate_console_action;
    let mut action = json!({"request_id":"fictional-id","kind":"home","action":"on","target":"light.example","confirmed":true});
    assert!(validate_console_action(&action).is_ok());
    action["confirmed"] = false.into();
    assert!(validate_console_action(&action).is_err());
    action["confirmed"] = true.into();
    action["target"] = "http://192.168.1.10/admin".into();
    assert!(validate_console_action(&action).is_err());
    action["target"] = "light.example".into();
    action["kind"] = "shell".into();
    assert!(validate_console_action(&action).is_err());
}

#[test]
fn home_favorite_changes_use_only_exact_entity_identifiers() {
    use endora_capabilities::application::validate_console_action;
    assert!(validate_console_action(&json!({"request_id":"favorite-choice","kind":"favorite","action":"add","target":"sensor.example","confirmed":true})).is_ok());
    assert!(validate_console_action(&json!({"request_id":"favorite-choice","kind":"favorite","action":"add","target":"sensor.example,light.other","confirmed":true})).is_err());
}

#[test]
fn docker_log_frames_are_decoded_without_transport_headers() {
    use endora_capabilities::infrastructure::terminal::decode_logs;
    let mut bytes = vec![1, 0, 0, 0, 0, 0, 0, 6];
    bytes.extend(b"hello\n");
    assert_eq!(decode_logs(&bytes), "hello\n");
    assert_eq!(decode_logs(b"plain\n"), "plain\n");
    assert_eq!(decode_logs(&bytes[..10]), "Log frame interrupted");
}

/// Exercises production adapters against a bounded local fake, including headers/routes.
fn mock_adapter(
    service: &'static str,
    responses: Vec<(&'static str, serde_json::Value)>,
) -> (
    endora_capabilities::infrastructure::terminal::Adapter,
    std::thread::JoinHandle<()>,
) {
    mock_adapter_with(service, responses, true)
}

/// [`mock_adapter`], with the service's address stored with or without `http://`, as a person
/// may type it.
fn mock_adapter_with(
    service: &'static str,
    responses: Vec<(&'static str, serde_json::Value)>,
    with_scheme: bool,
) -> (
    endora_capabilities::infrastructure::terminal::Adapter,
    std::thread::JoinHandle<()>,
) {
    use endora_capabilities::{CapabilitySettingsRepository, ConfigStore};
    use std::{
        io::{Read, Write},
        net::TcpListener,
        sync::Arc,
        time::Duration,
    };
    let listener = TcpListener::bind("localhost:0").unwrap();
    let address = listener.local_addr().unwrap().to_string();
    let origin = if with_scheme {
        format!("http://{address}")
    } else {
        address
    };
    let db = Db::open_in_memory().unwrap();
    endora_capabilities::migrate(&db).unwrap();
    let settings = Arc::new(ConfigStore::new(db.clone()));
    let id = if service == "home" {
        "home_assistant".into()
    } else {
        format!("terminal-{service}")
    };
    settings
        .set_setting(
            &id,
            if service == "home" { "url" } else { "origin" },
            &origin,
        )
        .unwrap();
    settings
        .set_setting(
            &id,
            if service == "home" {
                "token"
            } else {
                "api_key"
            },
            "fictional-adapter-secret",
        )
        .unwrap();
    settings
        .set_setting("terminal-home", "favorites", "light.example")
        .unwrap();
    settings
        .set_setting("terminal-overseerr", "request_user", "2")
        .unwrap();
    settings
        .set_setting("terminal-portainer", "endpoint", "1")
        .unwrap();
    let worker = std::thread::spawn(move || {
        for (expected, response) in responses {
            let (mut socket, _) = listener.accept().unwrap();
            socket
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut bytes = Vec::new();
            let mut byte = [0u8];
            while !bytes.ends_with(b"\r\n\r\n") {
                socket.read_exact(&mut byte).unwrap();
                bytes.push(byte[0]);
                assert!(bytes.len() < 8192);
            }
            let headers = String::from_utf8(bytes).unwrap();
            assert!(headers.starts_with(expected), "unexpected adapter route");
            assert!(headers.to_ascii_lowercase().contains(if service == "home" {
                "authorization: bearer fictional-adapter-secret"
            } else {
                "x-api-key: fictional-adapter-secret"
            }));
            if service == "overseerr" && expected.starts_with("POST") {
                assert!(headers.to_ascii_lowercase().contains("x-api-user: 2"));
            }
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length: ")
                        .map(|v| v.parse::<usize>().unwrap())
                })
                .unwrap_or(0);
            assert!(length < 8192);
            let mut body = vec![0; length];
            socket.read_exact(&mut body).unwrap();
            if expected.starts_with("POST /api/services/") {
                assert_eq!(
                    serde_json::from_slice::<serde_json::Value>(&body).unwrap(),
                    json!({"entity_id":"light.example"})
                );
            }
            let body = response.to_string();
            write!(socket,"HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",body.len()).unwrap();
        }
    });
    (
        endora_capabilities::infrastructure::terminal::Adapter::new(settings, db),
        worker,
    )
}

#[test]
fn home_action_uses_fixed_route_reads_back_and_does_not_replay() {
    use endora_capabilities::application::HomeConsole;
    let (adapter, mock) = mock_adapter(
        "home",
        vec![
            ("POST /api/services/light/turn_on ", json!([])),
            (
                "GET /api/states/light.example ",
                json!({"state":"on","attributes":{"secret":"private-upstream-payload"}}),
            ),
        ],
    );
    let request = json!({"kind":"home","action":"on","target":"light.example","confirmed":true,"request_id":"fixed-home-operation"});
    let first = adapter.act(&request).unwrap();
    mock.join().unwrap();
    let _: endora_protocol::Operation = fits(&first);
    assert_eq!(first["observed"]["verified"], true);
    assert!(!first.to_string().contains("private-upstream-payload"));
    assert!(!first.to_string().contains("fictional-adapter-secret"));
    assert_eq!(adapter.act(&request).unwrap(), first);
}

#[test]
fn the_team_sees_containers_by_name_with_their_service_and_whether_they_are_protected() {
    use endora_capabilities::application::HomeConsole;
    let (adapter, mock) = mock_adapter(
        "portainer",
        vec![(
            "GET /api/endpoints/1/docker/containers/json?all=true ",
            json!([
                {"Id": "ab".repeat(32), "Names": ["/sonarr"], "State": "running", "Status": "Up 2 days",
                 "Image": "lscr.io/linuxserver/sonarr", "Labels": {"com.docker.compose.service": "sonarr"}},
                {"Id": "cd".repeat(32), "Names": ["/quirky_hopper"], "State": "running", "Status": "Up 1 minute",
                 "Image": "endora-node:latest", "Labels": {}}
            ]),
        )],
    );
    let seen = adapter.read("team-containers", "").unwrap();
    mock.join().unwrap();
    assert_eq!(
        seen["items"][0],
        json!({"id": "ab".repeat(32), "name": "sonarr", "state": "running", "status": "Up 2 days",
               "service": "sonarr", "protected": false})
    );
    assert_eq!(
        seen["items"][1]["protected"], true,
        "a worker, by its image"
    );
}

#[test]
fn the_team_inspects_one_container_for_what_a_restart_would_not_fix() {
    use endora_capabilities::application::HomeConsole;
    let id = "ab".repeat(32);
    let (adapter, mock) = mock_adapter(
        "portainer",
        vec![(
            "GET /api/endpoints/1/docker/containers/abab",
            json!({ "Id": id, "RestartCount": 14, "Config": { "Env": ["DB_PASSWORD=hunter2"] },
                    "State": { "Status": "restarting", "OOMKilled": true, "ExitCode": 137,
                               "StartedAt": "2026-10-17T10:00:00.123456789Z" } }),
        )],
    );
    let seen = adapter.read("team-inspect", &id).unwrap();
    mock.join().unwrap();
    assert_eq!(
        seen,
        json!({ "state": "restarting", "restart_count": 14, "oom_killed": true, "exit_code": 137,
                "started_at": "2026-10-17T10:00:00.123456789Z" })
    );
    assert!(
        !seen.to_string().contains("hunter2"),
        "only the state, never the config"
    );
    assert!(
        adapter.read("team-inspect", "sonarr").is_err(),
        "an id only"
    );
}

#[test]
fn media_request_uses_privately_chosen_user_and_retains_pending_approval() {
    use endora_capabilities::application::HomeConsole;
    let (adapter, mock) = mock_adapter(
        "overseerr",
        vec![(
            "POST /api/v1/request ",
            json!({"id":7,"status":1,"secret":"private-upstream-payload"}),
        )],
    );
    let result=adapter.act(&json!({"kind":"media","action":"movie","target":"123","confirmed":true,"request_id":"fixed-media-operation"})).unwrap();
    mock.join().unwrap();
    let _: endora_protocol::Operation = fits(&result);
    assert_eq!(result["summary"], "Media request: pending approval");
    assert!(!result.to_string().contains("private-upstream-payload"));
}

#[test]
fn handheld_sources_are_settings_only_and_never_offered_to_the_model() {
    use endora_capabilities::infrastructure::default_capabilities;
    let all = default_capabilities();
    for (id, secret_key) in [
        ("terminal-overseerr", Some("api_key")),
        ("terminal-portainer", Some("api_key")),
        ("terminal-metrics", None),
        ("homelab-docker", None),
        ("homelab-sonarr", Some("api_key")),
        ("homelab-radarr", Some("api_key")),
    ] {
        let capability = all
            .iter()
            .find(|c| c.info().id == id)
            .unwrap_or_else(|| panic!("{id} must be listed so the console can set it up"));
        let info = capability.info();
        assert!(info.settings.iter().any(|s| s.key == "origin" && !s.secret));
        if let Some(key) = secret_key {
            assert!(info.settings.iter().any(|s| s.key == key && s.secret));
        }
        // Code-not-ready keeps it out of the model's catalogue (ADR 0093).
        assert!(!info.configured);
        assert!(capability.invoke(&json!({}), &Default::default()).is_err());
    }
}

#[test]
fn nas_metrics_use_the_telegraf_names_prometheus_actually_scrapes() {
    use endora_capabilities::infrastructure::terminal::metric_queries;
    let queries = metric_queries("fictional-nas").unwrap();
    let text: String = queries.iter().map(|(_, q)| q.as_str()).collect();
    for name in [
        "cpu_usage_idle",
        "mem_used_percent",
        "disk_free",
        "net_bytes_recv",
    ] {
        assert!(text.contains(name), "{name} missing");
    }
    assert!(
        !text.contains("node_"),
        "node_exporter names are not scraped here"
    );
    assert!(
        queries
            .iter()
            .all(|(_, q)| q.contains("host=\"fictional-nas\""))
    );
    // A label cannot break out of its string.
    let hostile = metric_queries("a\"}) or vector(1) #").unwrap();
    assert!(hostile.iter().all(|(_, q)| q.contains("host=\"a\\\"})")));
}

fn fake(status: &'static str, body: &'static str) -> (String, std::thread::JoinHandle<String>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let (mut socket, _) = listener.accept().unwrap();
        let mut data = vec![0; 4096];
        let n = socket.read(&mut data).unwrap();
        let _ = write!(
            socket,
            "HTTP/1.1 {status}\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        String::from_utf8_lossy(&data[..n]).to_lowercase()
    });
    (origin, thread)
}

#[test]
fn the_settings_test_button_checks_the_service_and_never_echoes_the_key() {
    use endora_capabilities::infrastructure::default_capabilities;
    let all = default_capabilities();
    let source = |id: &str| all.iter().find(|c| c.info().id == id).unwrap().clone();
    let settings = |pairs: &[(&str, &str)]| -> std::collections::BTreeMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect()
    };
    // Overseerr: an authenticated read reaches the service with the key.
    let (origin, thread) = fake("200 OK", r#"{"id":1}"#);
    let ok = source("terminal-overseerr")
        .invoke(
            &json!({}),
            &settings(&[("origin", &origin), ("api_key", "fictional-key")]),
        )
        .unwrap();
    assert_eq!(ok["ok"], true);
    assert!(!ok.to_string().contains("fictional-key"));
    assert!(thread.join().unwrap().contains("x-api-key: fictional-key"));
    // A rejected key is reported as a category, not upstream text.
    let (origin, thread) = fake("401 Unauthorized", "secret upstream detail");
    let err = source("terminal-portainer")
        .invoke(
            &json!({}),
            &settings(&[("origin", &origin), ("api_key", "fictional-key")]),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("authentication") && !err.contains("secret"));
    thread.join().unwrap();
    // Metrics: no data for the host label is a failure worth saying.
    let (origin, thread) = fake("200 OK", r#"{"data":{"result":[]}}"#);
    let err = source("terminal-metrics")
        .invoke(
            &json!({}),
            &settings(&[("origin", &origin), ("instance", "fictional-nas")]),
        )
        .unwrap_err()
        .to_string();
    assert!(err.contains("no data"));
    thread.join().unwrap();
    // The Overview dashboard sources probe their own routes; Docker needs no key.
    let (origin, thread) = fake("200 OK", r#"{"Version":"1"}"#);
    source("homelab-docker")
        .invoke(&json!({}), &settings(&[("origin", &origin)]))
        .unwrap();
    assert!(thread.join().unwrap().starts_with("get /version "));
    let (origin, thread) = fake("200 OK", r#"{"version":"4"}"#);
    source("homelab-sonarr")
        .invoke(
            &json!({}),
            &settings(&[("origin", &origin), ("api_key", "fictional-key")]),
        )
        .unwrap();
    let request = thread.join().unwrap();
    assert!(
        request.starts_with("get /api/v3/system/status ")
            && request.contains("x-api-key: fictional-key")
    );
    // Unset values say what is missing instead of connecting anywhere.
    assert!(
        source("terminal-overseerr")
            .invoke(&json!({}), &settings(&[]))
            .is_err()
    );
}

/// A service answering several requests in turn; returns what each one asked, lowercased.
fn fake_answering(
    answers: Vec<(&'static str, &'static str)>,
) -> (String, std::thread::JoinHandle<Vec<String>>) {
    fake_answering_with(answers.into_iter().map(|(s, b)| (s, "", b)).collect())
}

/// [`fake_answering`], with extra response headers (each ending in `\r\n`) per answer.
fn fake_answering_with(
    answers: Vec<(&'static str, &'static str, &'static str)>,
) -> (String, std::thread::JoinHandle<Vec<String>>) {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let thread = std::thread::spawn(move || {
        let mut asked = Vec::new();
        for (status, headers, body) in answers {
            let (mut socket, _) = listener.accept().unwrap();
            // The headers, then as much body as they announce, however the packets fall.
            let mut data = Vec::new();
            let mut chunk = [0u8; 4096];
            loop {
                let n = socket.read(&mut chunk).unwrap();
                data.extend_from_slice(&chunk[..n]);
                let text = String::from_utf8_lossy(&data).to_lowercase();
                let Some(end) = text.find("\r\n\r\n") else {
                    continue;
                };
                let length = text
                    .lines()
                    .find_map(|l| l.strip_prefix("content-length: "))
                    .and_then(|v| v.trim().parse::<usize>().ok())
                    .unwrap_or(0);
                if n == 0 || data.len() >= end + 4 + length {
                    break;
                }
            }
            asked.push(String::from_utf8_lossy(&data).to_lowercase());
            write!(
                socket,
                "HTTP/1.1 {status}\r\n{headers}Content-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            )
            .unwrap();
        }
        asked
    });
    (origin, thread)
}

#[test]
fn the_pihole_test_signs_in_with_the_app_password_and_signs_out_again() {
    use endora_capabilities::infrastructure::default_capabilities;
    let all = default_capabilities();
    let pihole = all
        .iter()
        .find(|c| c.info().id == "network-pihole")
        .unwrap()
        .clone();
    let settings = |origin: &str| -> std::collections::BTreeMap<String, String> {
        [("origin", origin), ("password", "fictional-app-password")]
            .iter()
            .map(|(k, v)| ((*k).into(), (*v).into()))
            .collect()
    };
    let (origin, thread) = fake_answering(vec![
        (
            "200 OK",
            r#"{"session":{"valid":true,"sid":"fictional-sid"}}"#,
        ),
        ("200 OK", ""),
    ]);
    let ok = pihole.invoke(&json!({}), &settings(&origin)).unwrap();
    assert_eq!(ok["ok"], true);
    assert!(!ok.to_string().contains("fictional"));
    let asked = thread.join().unwrap();
    assert!(asked[0].starts_with("post /api/auth ") && asked[0].contains("fictional-app-password"));
    // Pi-hole allows only a few sessions at once: the one the test made is given back.
    assert!(
        asked[1].starts_with("delete /api/auth ") && asked[1].contains("x-ftl-sid: fictional-sid")
    );
    // A wrong password is reported as a category.
    let (origin, thread) = fake_answering(vec![(
        "401 Unauthorized",
        r#"{"session":{"valid":false},"error":"secret upstream detail"}"#,
    )]);
    let err = pihole
        .invoke(&json!({}), &settings(&origin))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("authentication") && !err.contains("secret"),
        "{err}"
    );
    thread.join().unwrap();
    // The model is never offered it: it only holds settings.
    assert!(!pihole.info().configured);
    let unifi = all.iter().find(|c| c.info().id == "network-unifi").unwrap();
    let keys: Vec<_> = unifi
        .info()
        .settings
        .iter()
        .map(|s| (s.key, s.secret))
        .collect();
    assert_eq!(
        keys,
        [
            ("origin", false),
            ("username", false),
            ("password", true),
            ("site", false)
        ]
    );
}

#[test]
fn the_unifi_test_signs_in_with_the_view_only_admin_and_signs_out_again() {
    use endora_capabilities::infrastructure::default_capabilities;
    let all = default_capabilities();
    let unifi = all
        .iter()
        .find(|c| c.info().id == "network-unifi")
        .unwrap()
        .clone();
    let settings = |origin: &str| -> std::collections::BTreeMap<String, String> {
        [
            ("origin", origin),
            ("username", "endora"),
            ("password", "fictional-password"),
        ]
        .iter()
        .map(|(k, v)| ((*k).into(), (*v).into()))
        .collect()
    };
    let (origin, thread) = fake_answering_with(vec![
        (
            "200 OK",
            "Set-Cookie: unifises=fictional-session; Path=/; HttpOnly\r\n",
            r#"{"meta":{"rc":"ok"},"data":[]}"#,
        ),
        ("200 OK", "", r#"{"meta":{"rc":"ok"}}"#),
    ]);
    let ok = unifi.invoke(&json!({}), &settings(&origin)).unwrap();
    assert_eq!(ok["ok"], true);
    assert!(!ok.to_string().contains("fictional"));
    let asked = thread.join().unwrap();
    assert!(asked[0].starts_with("post /api/login ") && asked[0].contains("fictional-password"));
    assert!(
        asked[1].starts_with("post /api/logout ")
            && asked[1].contains("cookie: unifises=fictional-session"),
        "{}",
        asked[1]
    );
    let (origin, thread) = fake_answering_with(vec![(
        "400 Bad Request",
        "",
        r#"{"meta":{"rc":"error","msg":"api.err.Invalid"}}"#,
    )]);
    let err = unifi
        .invoke(&json!({}), &settings(&origin))
        .unwrap_err()
        .to_string();
    assert!(
        err.contains("authentication") && !err.contains("api.err"),
        "{err}"
    );
    thread.join().unwrap();
}

#[test]
fn media_requests_show_titles_and_years_not_database_numbers() {
    use endora_capabilities::application::HomeConsole;
    let (adapter, mock) = mock_adapter(
        "overseerr",
        vec![
            (
                "GET /api/v1/request?",
                json!({"results":[
                    {"type":"movie","status":1,"media":{"tmdbId":603}},
                    {"type":"tv","status":5,"media":{"tmdbId":1399}}]}),
            ),
            (
                "GET /api/v1/movie/603 ",
                json!({"title":"Example Film","releaseDate":"1999-03-30"}),
            ),
            (
                "GET /api/v1/tv/1399 ",
                json!({"name":"Example Show","firstAirDate":"2011-04-17"}),
            ),
        ],
    );
    let result = adapter.read("requests", "").unwrap();
    mock.join().unwrap();
    let _: endora_protocol::ConsoleView = fits(&result);
    let labels: Vec<_> = result["items"]
        .as_array()
        .unwrap()
        .iter()
        .map(|i| i["label"].as_str().unwrap().to_owned())
        .collect();
    assert_eq!(labels, ["Example Film (1999)", "Example Show (2011)"]);
    assert_eq!(result["items"][0]["detail"], "movie | pending approval");
}

#[test]
fn media_search_labels_show_the_year_not_the_full_date() {
    use endora_capabilities::application::HomeConsole;
    let (adapter, mock) = mock_adapter(
        "overseerr",
        vec![(
            "GET /api/v1/search?",
            json!({"results":[{"id":603,"mediaType":"movie","title":"Example Film","releaseDate":"1999-03-30"}]}),
        )],
    );
    let result = adapter.read("media", "example").unwrap();
    mock.join().unwrap();
    let view: endora_protocol::ConsoleView = fits(&result);
    assert_eq!(view.items[0].media_type.as_deref(), Some("movie"));
    assert_eq!(result["items"][0]["label"], "Example Film (1999)");
    assert_eq!(
        result["items"][0]["detail"],
        "movie | Availability: not requested"
    );
}

#[test]
fn dashboard_mirrors_homepage_groups_with_container_state_and_pretty_stats() {
    use endora_capabilities::infrastructure::terminal::project_dashboard;
    let homepage = json!([
        {"name":"Alerts","services":[{"name":"Firing alerts","href":"http://192.168.1.10:9090/alerts"}]},
        {"name":"Media","services":[
            {"name":"Overseerr","container":"overseerr","href":"http://192.168.1.10:5055"},
            {"name":"Plex","container":"plex"},
            {"name":"Router"},
            {"name":"Missing","container":"gone"}]}
    ]);
    let container_id = "1".repeat(64);
    let containers = json!([
        {"Id":container_id,"Names":["/overseerr"],"State":"running","Status":"Up 2 days"},
        {"Names":["/plex"],"State":"running","Status":"Up 2 days (unhealthy)"}
    ]);
    let metrics = json!({"items":[
        {"label":"CPU %","detail":"9.908"},
        {"label":"Memory %","detail":"38.3"},
        {"label":"Storage free bytes","detail":"13868097601536"},
        {"label":"Network receive bytes/s","detail":"oops"}]});
    let out = project_dashboard(
        &homepage,
        Some(&containers),
        Some(&metrics),
        &["DiskLow".to_owned()],
    );
    assert_eq!(
        out["stats"],
        json!(["CPU 10%", "Mem 38%", "Free 13.9 TB", "Net n/a"])
    );
    assert_eq!(out["alerts"], json!(["DiskLow"]));
    // The Alerts group lists each firing alert itself instead of Homepage's one widget row.
    let alerts_group: Vec<_> = out["groups"][0]["services"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["name"].as_str().unwrap(), s["state"].as_str().unwrap()))
        .collect();
    assert_eq!(alerts_group, [("DiskLow", "unhealthy")]);
    let quiet = project_dashboard(&homepage, None, None, &[]);
    assert_eq!(
        quiet["groups"][0]["services"][0]["name"],
        "No alerts firing"
    );
    assert_eq!(quiet["groups"][0]["services"][0]["state"], "running");
    let media = &out["groups"][1];
    assert_eq!(media["name"], "Media");
    let states: Vec<_> = media["services"]
        .as_array()
        .unwrap()
        .iter()
        .map(|s| (s["name"].as_str().unwrap(), s["state"].as_str().unwrap()))
        .collect();
    assert_eq!(
        states,
        [
            ("Overseerr", "running"),
            ("Plex", "unhealthy"),
            ("Router", "unknown"),
            ("Missing", "stopped")
        ]
    );
    // A running service carries its container id so the handheld can open its logs.
    assert_eq!(media["services"][0]["id"], container_id);
    assert!(media["services"][2].get("id").is_none());
    // Links and other homepage fields never reach the handheld.
    assert!(!out.to_string().contains("192.168.1.10"));
    // Without Portainer or metrics the groups still render.
    let bare = project_dashboard(&homepage, None, None, &[]);
    assert_eq!(bare["groups"][1]["services"][0]["state"], "unknown");
    assert_eq!(bare["stats"], json!([]));
}

#[test]
fn the_containers_page_shows_the_dashboards_short_figures_not_raw_numbers() {
    use endora_capabilities::infrastructure::terminal::nas_figures;
    // As Prometheus answers: unrounded, in bytes.
    let metrics = json!({"items":[
        {"label":"CPU %","detail":"7.251164760358932"},
        {"label":"Memory %","detail":"53.98017708186086"},
        {"label":"Storage free bytes","detail":"13759627984896"},
        {"label":"Network receive bytes/s","detail":"919.7017543859648"}]});
    assert_eq!(
        nas_figures(&metrics),
        ["CPU 7%", "Mem 54%", "Free 13.8 TB", "Net 920 B/s"]
    );
    assert!(nas_figures(&json!({"summary": "Metrics need private setup"})).is_empty());
}

#[test]
fn homepage_widgets_become_short_number_summaries() {
    use endora_capabilities::infrastructure::terminal::{widget_endpoints, widget_summary};
    assert_eq!(widget_endpoints("overseerr"), ["request/count"]);
    assert_eq!(widget_endpoints("radarr"), ["movie", "queue/status"]);
    assert!(widget_endpoints("fictional-widget").is_empty());
    let summary = |kind: &str, parts: Vec<(&str, serde_json::Value)>| {
        let parts: Vec<_> = parts.into_iter().map(|(e, v)| (e.to_owned(), v)).collect();
        widget_summary(kind, &parts)
    };
    assert_eq!(
        summary(
            "overseerr",
            vec![(
                "request/count",
                json!({"pending":2,"approved":17,"processing":5,"secret":"x"})
            )]
        )
        .unwrap(),
        "2 pending | 17 approved | 5 processing"
    );
    assert_eq!(
        summary(
            "radarr",
            vec![
                (
                    "movie",
                    json!({"have":194,"missing":28,"all":[{"title":"hidden"}]})
                ),
                ("queue/status", json!({"totalCount":6}))
            ]
        )
        .unwrap(),
        "194 have | 28 missing | 6 queued"
    );
    // One failed endpoint still yields what the others said.
    assert_eq!(
        summary("radarr", vec![("queue/status", json!({"totalCount":6}))]).unwrap(),
        "6 queued"
    );
    assert_eq!(
        summary("sonarr", vec![("series", json!([{}, {}, {}]))]).unwrap(),
        "3 series"
    );
    assert_eq!(
        summary(
            "nzbget",
            vec![("status", json!({"RemainingSizeMB":48574,"DownloadRate":0}))]
        )
        .unwrap(),
        "48.6 GB left | 0 B/s"
    );
    assert_eq!(
        summary("prowlarr", vec![("indexer", json!([{}, {}]))]).unwrap(),
        "2 indexers"
    );
    assert_eq!(
        summary(
            "gatus",
            vec![(
                "status",
                json!([{"results":[{"success":true}]},{"results":[{"success":false}]}])
            )]
        )
        .unwrap(),
        "1/2 up"
    );
    assert_eq!(
        summary(
            "prometheus",
            vec![(
                "targets",
                json!({"data":{"activeTargets":[{"health":"up"},{"health":"down"}]}})
            )]
        )
        .unwrap(),
        "1/2 targets up"
    );
    assert_eq!(
        summary(
            "backrest",
            vec![("summary", json!({"numPlans":2,"numFailureLatest":0}))]
        )
        .unwrap(),
        "2 plans | 0 failed"
    );
    assert_eq!(summary("overseerr", vec![]), None);
    assert_eq!(summary("fictional-widget", vec![("x", json!({}))]), None);
}

#[test]
fn dashboard_services_carry_a_detail_only_when_one_was_read() {
    use endora_capabilities::infrastructure::terminal::project_dashboard_with;
    let homepage = json!([{"name":"Media","services":[{"name":"Overseerr"},{"name":"Plex"}]}]);
    let mut details = std::collections::HashMap::new();
    details.insert("Media/Overseerr".to_owned(), "2 pending".to_owned());
    let out = project_dashboard_with(&homepage, None, None, &[], &details);
    assert_eq!(out["groups"][0]["services"][0]["detail"], "2 pending");
    assert!(out["groups"][0]["services"][1].get("detail").is_none());
}

#[test]
fn the_home_screen_and_favorites_fit_the_protocol() {
    // Homepage's groups with a running container and a firing alert, and two favorites.
    let homepage = json!([{"name": "Media", "services": [{"name": "Example Server", "server": "local", "container": "example"}]}]);
    let containers = json!([{"Names": ["/example"], "State": "running", "Status": "Up 2 hours", "Id": "0123456789abcdef"}]);
    let dashboard = endora_capabilities::infrastructure::terminal::project_dashboard(
        &homepage,
        Some(&containers),
        None,
        &["Example alert".to_owned()],
    );
    let typed: endora_protocol::Dashboard = fits(&dashboard);
    assert!(typed.groups.iter().any(|g| g.name == "Media"));
    let states = json!([
        {"entity_id": "light.example", "state": "on", "attributes": {"friendly_name": "Example lamp"}},
        {"entity_id": "sensor.example", "state": "21", "attributes": {"friendly_name": "Example", "unit_of_measurement": "°C"}}
    ]);
    let home = project_home(
        &states,
        &["light.example".to_owned(), "sensor.example".to_owned()],
    );
    let typed: endora_protocol::ConsoleView = fits(&home);
    assert_eq!(typed.items.len(), 2);
    assert_eq!(typed.items[0].id.as_deref(), Some("light.example"));
}

#[test]
fn a_home_assistant_address_typed_without_http_still_reaches_the_catalog() {
    // Live: the Home Assistant skill accepted an address typed as `host:8123`, the console's
    // own requests refused it as "configuration", and the handheld's Home catalog never loaded.
    use endora_capabilities::application::HomeConsole;
    let (adapter, mock) = mock_adapter_with(
        "home",
        vec![(
            "GET /api/states ",
            json!([{"entity_id": "light.example", "state": "on", "attributes": {"friendly_name": "Example lamp"}}]),
        )],
        false,
    );
    let result = adapter.read("home-catalog", "").expect("the catalog loads");
    mock.join().unwrap();
    let view: endora_protocol::ConsoleView = fits(&result);
    assert_eq!(view.items[0].label, "Example lamp");
}

#[test]
fn the_agenda_watches_firing_alerts_and_failing_checks_through_fixed_queries_and_nothing_else() {
    use endora_capabilities::application::HomeConsole;
    let (adapter, mock) = mock_adapter(
        "metrics",
        vec![
            (
                "GET /api/v1/alerts ",
                json!({"status":"success","data":{"alerts":[
                    {"state":"firing","labels":{"alertname":"DiskFailingSmart","severity":"critical",
                        "drive":"/dev/sda","serial_number":"FICTIONAL-SERIAL","instance":"192.168.1.10:9902"},
                     "annotations":{"summary":"/dev/sda is failing its SMART health check"}},
                    {"state":"pending","labels":{"alertname":"LowMemory","severity":"warning"}}]}}),
            ),
            (
                "GET /api/v1/query?query=gatus_results_endpoint_success",
                json!({"status":"success","data":{"result":[
                    {"metric":{"name":"Plex","instance":"192.168.1.10:9382"},"value":[1,"0"]}]}}),
            ),
        ],
    );
    let watched = adapter.read("watch", "").unwrap();
    mock.join().unwrap();
    assert_eq!(
        watched,
        json!({"alerts":[{"name":"DiskFailingSmart","severity":"critical","target":"/dev/sda",
            "summary":"/dev/sda is failing its SMART health check"}],"checks_failing":["Plex"]}),
        "a pending alert is not firing, and no other label is kept"
    );
    assert!(!watched.to_string().contains("FICTIONAL-SERIAL"));
    assert!(!watched.to_string().contains("192.168"));
}
