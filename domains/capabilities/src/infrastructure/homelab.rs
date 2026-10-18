//! Fixed-route dashboard reads. No model input and no general web-egress bypass.
use crate::application::{CapabilitySettingsRepository, HomelabObserver};
use serde_json::{Value, json};
use std::{sync::Arc, time::Duration};

/// Validates a service origin; paths and credentials are selected separately.
///
/// # Errors
/// Invalid or credential-bearing origin.
pub fn validate_origin(value: &str) -> Result<(), &'static str> {
    let url: ureq::http::Uri = value.parse().map_err(|_| "configuration")?;
    if !matches!(url.scheme_str(), Some("http" | "https"))
        || url.host().is_none()
        || url.authority().is_some_and(|a| a.as_str().contains('@'))
        || !matches!(url.path(), "" | "/")
        || url.query().is_some()
        || value.contains('#')
    {
        return Err("configuration");
    }
    Ok(())
}

fn text(value: &Value) -> String {
    value
        .as_str()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect()
}

/// An exited container's exit code, from Docker's status text ("Exited (137) 2 hours ago").
/// What tells a crash from a deliberate stop (ADR 0103).
fn exit_code(status: &str) -> Option<i32> {
    let rest = status.strip_prefix("Exited (")?;
    rest.split(')').next()?.parse().ok()
}

/// Projects only display fields, retaining at most 100 items per source.
///
/// # Errors
/// Unexpected upstream response shape.
pub fn normalize(service: &str, value: &Value) -> Result<Value, &'static str> {
    if service == "docker" {
        let records = value.as_array().ok_or("protocol")?;
        let items: Vec<_> = records.iter().take(100).map(|item| {
            let status = item["Status"].as_str().unwrap_or("");
            let health = if status.contains("(unhealthy)") { "unhealthy" } else if status.contains("(healthy)") { "healthy" } else if status.contains("health: starting") { "starting" } else { "unknown" };
            json!({"name":text(&item["Names"][0]).trim_start_matches('/'),"state":text(&item["State"]),"health":health})
        }).collect();
        let degraded = items
            .iter()
            .any(|i| i["state"] != "running" || i["health"] == "unhealthy");
        return Ok(
            json!({"state":if degraded {"degraded"} else {"healthy"},"total":records.len(),"items":items}),
        );
    }
    let records = value["records"].as_array().ok_or("protocol")?;
    let total = value["totalRecords"].as_u64().ok_or("protocol")?;
    let items: Vec<_> = records
        .iter()
        .take(100)
        .map(|item| {
            let size = item["size"].as_f64().unwrap_or(0.0);
            let left = item["sizeleft"].as_f64().unwrap_or(size);
            let progress = if size > 0.0 {
                ((size - left) / size * 100.0).clamp(0.0, 100.0)
            } else {
                0.0
            };
            let stuck = item["trackedDownloadStatus"] == "warning" || item["trackedDownloadStatus"] == "error";
            json!({"title":text(&item["title"]),"progress":progress,"status":text(&item["status"]),"stuck":stuck})
        })
        .collect();
    let degraded = records
        .iter()
        .any(|r| r["trackedDownloadStatus"] == "warning" || r["trackedDownloadStatus"] == "error");
    Ok(json!({"state":if degraded {"degraded"} else {"healthy"},"total":total,"items":items}))
}

/// The observer's projection: [`normalize`], and for each container its id and how it exited,
/// which the agenda loop's restart and its crashed-or-stopped rule need (ADR 0103), and
/// Docker's status text, which the team's `homelab_status` contract reads (ADR 0106). For the
/// console, the agenda and the team's contracts only; what a chat skill hands a model stays
/// [`normalize`]'s.
///
/// # Errors
/// Unexpected upstream response shape.
pub fn normalize_observed(service: &str, value: &Value) -> Result<Value, &'static str> {
    let mut out = normalize(service, value)?;
    if service != "docker" {
        return Ok(out);
    }
    let records = value.as_array().ok_or("protocol")?;
    if let Some(items) = out["items"].as_array_mut() {
        for (item, raw) in items.iter_mut().zip(records.iter().take(100)) {
            item["id"] = text(&raw["Id"]).into();
            item["exit"] = exit_code(raw["Status"].as_str().unwrap_or("")).into();
            item["status"] = text(&raw["Status"]).into();
        }
    }
    Ok(out)
}

/// Infrastructure reader using the existing private settings repository.
pub struct Reader {
    settings: Arc<dyn CapabilitySettingsRepository + Send + Sync>,
}

impl Reader {
    /// Binds the dashboard reader without registering model tools.
    pub fn new(settings: Arc<dyn CapabilitySettingsRepository + Send + Sync>) -> Self {
        Self { settings }
    }
}

impl Reader {
    /// One fixed GET of a configured service: its origin and key from the settings, a short
    /// timeout, no redirects and no proxy, a bounded body.
    fn get(&self, service: &str, path: &str) -> Result<Value, &'static str> {
        let id = format!("homelab-{service}");
        let settings = self.settings.all_settings().map_err(|_| "configuration")?;
        let setting = |key: &str| {
            settings
                .iter()
                .find(|(s, k, _)| s == &id && k == key)
                .map(|(_, _, v)| v.as_str())
                .unwrap_or("")
        };
        let origin = setting("origin").trim_end_matches('/');
        if origin.is_empty() {
            return Err("unconfigured");
        }
        validate_origin(origin)?;
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(5)))
            .max_redirects(0)
            .proxy(None)
            .http_status_as_error(false)
            .build()
            .into();
        let mut request = agent.get(format!("{origin}{path}"));
        if service != "docker" {
            if setting("api_key").is_empty() {
                return Err("unconfigured");
            }
            request = request.header("X-Api-Key", setting("api_key"));
        }
        // A read that timed out is no evidence the service is down (ADR 0124): it is said
        // apart from a refused connection, and observed as unverified.
        let mut response = request.call().map_err(|e| match e {
            ureq::Error::Timeout(_) => "timeout",
            _ => "connection",
        })?;
        match response.status().as_u16() {
            200 => {}
            401 | 403 => return Err("authentication"),
            _ => return Err("upstream"),
        }
        response
            .body_mut()
            .with_config()
            .limit(1024 * 1024)
            .read_json()
            .map_err(|_| "protocol")
    }
}

/// Whether a container's name is one Docker could have given it, and so one the inspection
/// route may carry: a letter or digit, then letters, digits, `_`, `.` or `-`.
#[must_use]
pub fn a_container_name(name: &str) -> bool {
    let mut bytes = name.bytes();
    name.len() <= 128
        && bytes.next().is_some_and(|b| b.is_ascii_alphanumeric())
        && bytes.all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'))
}

impl HomelabObserver for Reader {
    fn observe(&self, service: &str) -> Result<Value, &'static str> {
        let path = match service {
            "docker" => "/containers/json?all=true",
            "sonarr" | "radarr" => "/api/v3/queue?page=1&pageSize=100",
            _ => return Err("configuration"),
        };
        let value = self.get(service, path)?;
        normalize_observed(service, &value)
    }

    fn inspect(&self, name: &str) -> Result<Value, &'static str> {
        if !a_container_name(name) {
            return Err("configuration");
        }
        let value = self.get("docker", &format!("/containers/{name}/json"))?;
        Ok(crate::lab_map::project(&value))
    }
}
