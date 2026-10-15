//! Dashboard contract and privacy tests.
use endora_capabilities::infrastructure::homelab::{normalize, validate_origin};
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
