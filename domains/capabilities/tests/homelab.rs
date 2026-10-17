//! Dashboard contract and privacy tests.
use endora_capabilities::infrastructure::homelab::{
    normalize, normalize_observed, validate_origin,
};
use serde_json::json;

#[test]
fn one_source_failure_does_not_hide_successes() {
    struct Observer;
    impl endora_capabilities::application::HomelabObserver for Observer {
        fn observe(&self, service: &str) -> Result<serde_json::Value, &'static str> {
            if service == "docker" {
                return Err("connection");
            }
            Ok(json!({"state":"healthy","items":[]}))
        }
    }
    let result = endora_capabilities::application::observe_homelab(&Observer, 10);
    assert_eq!(result["services"][0]["state"], "unavailable");
    assert_eq!(result["services"][1]["state"], "healthy");
}

#[test]
fn explicit_origins_only_and_no_credential_urls() {
    assert!(validate_origin("http://192.168.1.10:8989").is_ok());
    for url in [
        "file:///etc/passwd",
        "http://user:key@example.com",
        "http://example.com?secret=key",
        "http://example.com/redirect",
    ] {
        assert!(validate_origin(url).is_err());
    }
}

#[test]
fn docker_projection_never_returns_environment_or_mounts() {
    let result = normalize("docker", &json!([{"Names":["/example"],"State":"running","Status":"Up (healthy)","Env":["SECRET=fictional"],"Mounts":["/private"]}])).unwrap();
    assert_eq!(result["items"][0]["name"], "example");
    assert_eq!(result["items"][0]["health"], "healthy");
    assert!(!result.to_string().contains("SECRET"));
    assert!(!result.to_string().contains("private"));
}

#[test]
fn queue_is_bounded_and_progress_is_derived() {
    let result = normalize("sonarr", &json!({"totalRecords":25,"records":[{"title":"Example", "size":100,"sizeleft":25,"status":"downloading","outputPath":"/private"}]})).unwrap();
    assert_eq!(result["total"], 25);
    assert_eq!(result["items"][0]["progress"], 75.0);
    assert!(!result.to_string().contains("private"));
    assert!(normalize("radarr", &json!({"unexpected":"shape"})).is_err());
}

#[test]
fn the_observer_sees_a_containers_id_and_how_it_exited_and_a_download_whether_it_is_stuck() {
    let id = "ab".repeat(32);
    let containers = json!([
        {"Id": id, "Names":["/crashed"],"State":"exited","Status":"Exited (1) 2 minutes ago"},
        {"Id": id, "Names":["/stopped"],"State":"exited","Status":"Exited (137) 3 hours ago"},
        {"Id": id, "Names":["/up"],"State":"running","Status":"Up 2 days"}
    ]);
    let result = normalize_observed("docker", &containers).unwrap();
    assert_eq!(result["items"][0]["id"], id.as_str());
    assert_eq!(result["items"][0]["exit"], 1);
    assert_eq!(result["items"][1]["exit"], 137);
    assert!(result["items"][2]["exit"].is_null());
    // And Docker's status text, which the team's homelab_status reads (ADR 0106).
    assert_eq!(result["items"][2]["status"], "Up 2 days");
    // What a chat skill hands a model has neither.
    let said = normalize("docker", &containers).unwrap().to_string();
    assert!(!said.contains(&id) && !said.contains("\"exit\""), "{said}");
    let queue = normalize(
        "radarr",
        &json!({"totalRecords":2,"records":[
        {"title":"Stuck","trackedDownloadStatus":"warning"},
        {"title":"Fine","trackedDownloadStatus":"ok"}]}),
    )
    .unwrap();
    assert_eq!(queue["items"][0]["stuck"], true);
    assert_eq!(queue["items"][1]["stuck"], false);
}

#[test]
fn only_a_name_docker_could_give_is_inspected() {
    use endora_capabilities::infrastructure::homelab::a_container_name;
    for name in ["gluetun", "endora-node", "project_runner_1", "a.b"] {
        assert!(a_container_name(name), "{name}");
    }
    for name in [
        "", "../json", "x/json", "-x", ".x", "x?size=1", "x y", "x%2f",
    ] {
        assert!(!a_container_name(name), "{name}");
    }
    assert!(!a_container_name(&"x".repeat(129)));
}
