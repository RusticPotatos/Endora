//! The handheld's configured services, usable from chat — read-only, and only as far as the
//! person has already set them up. Fictional hosts and keys throughout.
use endora_capabilities::infrastructure::terminal::metric_queries;
use endora_capabilities::infrastructure::{CapabilitySettings, default_capabilities};
use endora_capabilities::{CapabilityRunner, RegistryRunner};
use serde_json::{Value, json};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::io::{Read, Write};
use std::sync::{Arc, Mutex};

/// A fake service. Serves any number of requests with one canned response, and records each
/// request it received (lower-cased, headers included) so a test can see what was sent and
/// where.
struct Upstream {
    origin: String,
    seen: Arc<Mutex<Vec<String>>>,
}

fn upstream(status: &'static str, extra_headers: &'static str, body: String) -> Upstream {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let origin = format!("http://{}", listener.local_addr().unwrap());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let log = seen.clone();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut socket) = stream else { break };
            let mut data = vec![0; 8192];
            let n = socket.read(&mut data).unwrap_or(0);
            log.lock()
                .unwrap()
                .push(String::from_utf8_lossy(&data[..n]).to_lowercase());
            let _ = write!(
                socket,
                "HTTP/1.1 {status}\r\nContent-Type: application/json\r\n{extra_headers}Content-Length: {}\r\nConnection: close\r\n\r\n{body}",
                body.len()
            );
        }
    });
    Upstream { origin, seen }
}

fn ok(body: Value) -> Upstream {
    upstream("200 OK", "", body.to_string())
}

fn settings(pairs: &[(&str, &str)]) -> CapabilitySettings {
    pairs
        .iter()
        .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
        .collect::<BTreeMap<_, _>>()
}

fn skill(id: &str) -> Arc<dyn endora_capabilities::infrastructure::Capability> {
    default_capabilities()
        .into_iter()
        .find(|c| c.info().id == id)
        .unwrap_or_else(|| panic!("no capability {id:?} is registered"))
}

/// Docker's container list, as both the Docker API and Portainer's proxy of it return it.
fn containers() -> Value {
    json!([
        {"Names":["/plex"],"State":"running","Status":"Up 3 days (healthy)","Id":"deadbeef01"},
        {"Names":["/nextcloud"],"State":"exited","Status":"Exited (1) 2 hours ago","Id":"deadbeef02"},
        {"Names":["/sonarr"],"State":"running","Status":"Up 3 days (unhealthy)","Id":"deadbeef03"}
    ])
}

fn queue() -> Value {
    json!({"totalRecords":2,"records":[
        {"title":"Some.Show.S01E01.1080p","size":1000.0,"sizeleft":250.0,"status":"downloading","trackedDownloadStatus":"ok"},
        {"title":"Other.Show.S02E05.720p","size":500.0,"sizeleft":500.0,"status":"queued","trackedDownloadStatus":"warning"}
    ]})
}

// ---------------------------------------------------------------------------------------

#[test]
fn a_chat_skill_is_offered_only_once_the_handheld_entry_it_rides_on_is_set_up() {
    let all = Arc::new(default_capabilities());
    let spec = |runner: &RegistryRunner, id: &str| {
        runner
            .available()
            .into_iter()
            .find(|s| s.id == id)
            .unwrap_or_else(|| panic!("{id} is not in the catalogue at all"))
    };

    let nothing_set = RegistryRunner::new(all.clone());
    assert!(!spec(&nothing_set, "containers").configured);

    let mut stored = HashMap::new();
    stored.insert(
        "homelab-docker".to_owned(),
        settings(&[("origin", "http://127.0.0.1:2375")]),
    );
    let docker_set = RegistryRunner::with_config(
        all,
        vec![],
        HashSet::new(),
        endora_capabilities::AutonomyEnvelope::default(),
        stored,
    );
    assert!(
        spec(&docker_set, "containers").configured,
        "set up for the handheld, so usable from chat — no second configuration"
    );
    assert!(
        !spec(&docker_set, "sonarr_queue").configured,
        "a different service that was not set up stays unavailable"
    );
    assert!(
        !spec(&docker_set, "homelab-docker").configured,
        "and the settings entry itself is unchanged: it holds settings, it is not a tool"
    );
}

#[test]
fn every_chat_skill_only_reads_and_keeps_the_turn_on_the_device() {
    for id in [
        "containers",
        "portainer_containers",
        "sonarr_queue",
        "radarr_queue",
        "nas_status",
    ] {
        let info = skill(id).info();
        assert_eq!(
            info.reversibility,
            endora_kernel::Reversibility::Observe,
            "{id} may only observe"
        );
        assert!(
            info.input_schema.is_none(),
            "{id} takes no input, so the model cannot choose where it reads"
        );
        assert!(
            info.third_party,
            "{id}: its output marks the turn, so it can neither act nor leave the device \
             (queue titles come from indexers; the rest is the person's infrastructure)"
        );
        assert!(
            info.borrows_from.is_some(),
            "{id} must ride on the handheld's settings"
        );
        assert!(
            info.settings.is_empty(),
            "{id} asks for no second configuration"
        );
    }
}

#[test]
fn containers_reports_names_and_states_and_nothing_else() {
    let service = ok(containers());
    let out = skill("containers")
        .invoke(&json!({}), &settings(&[("origin", &service.origin)]))
        .expect("reads");
    let said = skill("containers").summarize(&out);

    assert!(said.contains("plex") && said.contains("running"), "{said}");
    assert!(
        said.contains("nextcloud") && said.contains("exited"),
        "{said}"
    );
    assert!(said.contains("unhealthy"), "{said}");
    assert!(
        !said.contains("deadbeef") && !out.to_string().contains("deadbeef"),
        "ids and everything else upstream said are projected away: {out}"
    );
}

#[test]
fn the_model_cannot_choose_where_a_chat_skill_reads() {
    let real = ok(containers());
    let elsewhere = ok(containers());
    skill("containers")
        .invoke(
            &json!({"url": elsewhere.origin, "origin": elsewhere.origin, "host": "evil.example"}),
            &settings(&[("origin", &real.origin)]),
        )
        .expect("reads");
    assert_eq!(
        real.seen.lock().unwrap().len(),
        1,
        "the configured origin was read"
    );
    assert!(
        elsewhere.seen.lock().unwrap().is_empty(),
        "an address in the model's input was never contacted"
    );
}

#[test]
fn the_key_travels_in_a_header_and_never_in_the_address() {
    let service = ok(queue());
    skill("sonarr_queue")
        .invoke(
            &json!({}),
            &settings(&[
                ("origin", &service.origin),
                ("api_key", "fictional-key-123"),
            ]),
        )
        .expect("reads");
    let request = service.seen.lock().unwrap()[0].clone();
    let first_line = request.lines().next().unwrap();
    assert!(
        request.contains("x-api-key: fictional-key-123"),
        "{request}"
    );
    assert!(
        !first_line.contains("fictional-key-123"),
        "key in the URL: {first_line}"
    );
}

#[test]
fn a_queue_reports_titles_and_progress_and_flags_trouble() {
    let service = ok(queue());
    let out = skill("sonarr_queue")
        .invoke(
            &json!({}),
            &settings(&[("origin", &service.origin), ("api_key", "k")]),
        )
        .expect("reads");
    let said = skill("sonarr_queue").summarize(&out);
    assert!(
        said.contains("Some.Show.S01E01.1080p") && said.contains("75"),
        "{said}"
    );
    assert!(
        said.to_lowercase().contains("warning") || said.to_lowercase().contains("degraded"),
        "a download in trouble must be said, not buried: {said}"
    );
}

#[test]
fn a_redirect_is_never_followed() {
    let trap = ok(containers());
    let redirecting = upstream(
        "302 Found",
        Box::leak(format!("Location: {}/containers/json\r\n", trap.origin).into_boxed_str()),
        String::new(),
    );
    let outcome =
        skill("containers").invoke(&json!({}), &settings(&[("origin", &redirecting.origin)]));
    assert!(outcome.is_err(), "a redirect is not an answer");
    assert!(
        trap.seen.lock().unwrap().is_empty(),
        "the redirect target was contacted: a configured address must not become a pivot"
    );
}

#[test]
fn an_oversized_answer_is_refused_rather_than_held() {
    let service = upstream("200 OK", "", "[".to_owned() + &"1,".repeat(700_000) + "1]");
    assert!(
        skill("containers")
            .invoke(&json!({}), &settings(&[("origin", &service.origin)]))
            .is_err()
    );
}

#[test]
fn a_failure_names_a_category_and_never_the_key_or_what_the_service_said() {
    let service = upstream(
        "401 Unauthorized",
        "",
        r#"{"error":"bad key fictional-key-999, contact admin@example.com"}"#.to_owned(),
    );
    let err = skill("radarr_queue")
        .invoke(
            &json!({}),
            &settings(&[
                ("origin", &service.origin),
                ("api_key", "fictional-key-999"),
            ]),
        )
        .expect_err("refused");
    let said = format!("{err:?}");
    assert!(!said.contains("fictional-key-999"), "{said}");
    assert!(!said.contains("admin@example.com"), "{said}");
    assert!(
        said.to_lowercase().contains("key") || said.to_lowercase().contains("authenticat"),
        "{said}"
    );
}

#[test]
fn an_unconfigured_skill_says_what_is_missing_without_reaching_anywhere() {
    let err = skill("sonarr_queue")
        .invoke(&json!({}), &settings(&[]))
        .expect_err("nothing to read");
    assert!(
        format!("{err:?}").to_lowercase().contains("set up"),
        "{err:?}"
    );
}

#[test]
fn nas_status_asks_the_configured_host_and_reports_each_figure() {
    let service = ok(json!({"data":{"result":[{"value":[0,"42.5"]}]}}));
    let out = skill("nas_status")
        .invoke(
            &json!({}),
            &settings(&[("origin", &service.origin), ("instance", "fictional-nas")]),
        )
        .expect("reads");
    let wanted = metric_queries("fictional-nas").unwrap().len();
    assert_eq!(out["items"].as_array().unwrap().len(), wanted);
    assert!(skill("nas_status").summarize(&out).contains("42.5"));
    assert_eq!(
        service.seen.lock().unwrap().len(),
        wanted,
        "one read per figure"
    );
}

#[test]
fn portainer_containers_use_the_environment_it_was_set_up_with() {
    let service = ok(containers());
    let out = skill("portainer_containers")
        .invoke(
            &json!({}),
            &settings(&[
                ("origin", &service.origin),
                ("api_key", "k"),
                ("endpoint", "7"),
            ]),
        )
        .expect("reads");
    assert!(
        skill("portainer_containers")
            .summarize(&out)
            .contains("plex")
    );
    let request = service.seen.lock().unwrap()[0].clone();
    assert!(
        request.starts_with("get /api/endpoints/7/docker/containers/json?all=true"),
        "{request}"
    );
}

#[test]
fn an_environment_id_that_is_not_a_plain_number_is_refused_before_anything_is_sent() {
    // The id is spliced into a URL path. A value like `7/../../x` would let a setting steer
    // the request somewhere else on the same host.
    for hostile in [
        "7/../../containers",
        "7?all=false",
        "",
        "abc",
        "1234567890123",
    ] {
        let service = ok(containers());
        let outcome = skill("portainer_containers").invoke(
            &json!({}),
            &settings(&[
                ("origin", &service.origin),
                ("api_key", "k"),
                ("endpoint", hostile),
            ]),
        );
        assert!(outcome.is_err(), "{hostile:?} was accepted");
        assert!(
            service.seen.lock().unwrap().is_empty(),
            "{hostile:?} reached the service"
        );
    }
}

#[test]
fn what_remains_handheld_only_still_says_so_and_what_is_now_in_chat_no_longer_does() {
    let description = |id: &str| skill(id).info().description.to_lowercase();
    for id in ["terminal-overseerr", "terminal-homepage"] {
        assert!(
            description(id).contains("not used by chat"),
            "{id} is still handheld-only and should say so"
        );
    }
    for id in [
        "homelab-docker",
        "homelab-sonarr",
        "homelab-radarr",
        "terminal-metrics",
        "terminal-portainer",
    ] {
        assert!(
            !description(id).contains("not used by chat"),
            "{id} is available to chat now; its entry must not claim otherwise"
        );
    }
}

#[test]
fn a_borrower_is_not_ready_without_its_connections_settings() {
    let info = skill("containers").info();
    assert_eq!(info.borrows_from, Some("homelab-docker"));
    assert!(!endora_capabilities::settings_ready(
        &info,
        &endora_capabilities::CapabilitySettings::new()
    ));
    let mut docker = endora_capabilities::CapabilitySettings::new();
    docker.insert("origin".into(), "http://127.0.0.1:9876".into());
    assert!(endora_capabilities::settings_ready(&info, &docker));
}
