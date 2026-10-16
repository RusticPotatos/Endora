//! Explicit home operations. Never registered as arbitrary model tools.
use crate::application::{CapabilitySettingsRepository, HomeConsole};
use endora_persistence::Db;
use rusqlite::{OptionalExtension, params};
use serde_json::{Value, json};
use std::{
    io::{Read, Write},
    sync::{Arc, Mutex},
    time::Duration,
};

const JOURNAL_SCHEMA: &str = "CREATE TABLE IF NOT EXISTS terminal_operations (id TEXT PRIMARY KEY, fingerprint TEXT NOT NULL, result TEXT NOT NULL, created_ms INTEGER NOT NULL) STRICT;";
fn safe(v: &Value) -> String {
    v.as_str()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(200)
        .collect()
}
/// The Homepage widget types the handheld can summarize, and the endpoints each needs.
/// Fixed: the node never forwards a model- or client-supplied endpoint.
#[must_use]
pub fn widget_endpoints(kind: &str) -> &'static [&'static str] {
    match kind {
        "overseerr" => &["request/count"],
        "radarr" => &["movie", "queue/status"],
        "sonarr" => &["series"],
        "nzbget" => &["status"],
        "prowlarr" => &["indexer"],
        "gatus" => &["status"],
        "prometheus" => &["targets"],
        "backrest" => &["summary"],
        _ => &[],
    }
}

/// A short, number-only summary of a widget's data; `None` when nothing usable came back.
#[must_use]
pub fn widget_summary(kind: &str, parts: &[(String, Value)]) -> Option<String> {
    let part = |endpoint: &str| parts.iter().find(|(e, _)| e == endpoint).map(|(_, v)| v);
    let count = |v: &Value| v.as_array().map(Vec::len);
    let mut out: Vec<String> = Vec::new();
    match kind {
        "overseerr" => {
            let v = part("request/count")?;
            for key in ["pending", "approved", "processing"] {
                if let Some(n) = v[key].as_u64() {
                    out.push(format!("{n} {key}"));
                }
            }
        }
        "radarr" => {
            if let Some(v) = part("movie") {
                for key in ["have", "missing"] {
                    if let Some(n) = v[key].as_u64() {
                        out.push(format!("{n} {key}"));
                    }
                }
            }
            if let Some(n) = part("queue/status").and_then(|v| v["totalCount"].as_u64()) {
                out.push(format!("{n} queued"));
            }
        }
        "sonarr" => out.push(format!("{} series", count(part("series")?)?)),
        "nzbget" => {
            let v = part("status")?;
            if let Some(mb) = v["RemainingSizeMB"].as_f64() {
                out.push(format!("{} left", human_bytes(mb * 1e6)));
            }
            if let Some(rate) = v["DownloadRate"].as_f64() {
                out.push(format!("{}/s", human_bytes(rate)));
            }
        }
        "prowlarr" => out.push(format!("{} indexers", count(part("indexer")?)?)),
        "gatus" => {
            let all = part("status")?.as_array()?;
            let up = all
                .iter()
                .filter(|e| {
                    e["results"]
                        .as_array()
                        .and_then(|r| r.last())
                        .is_some_and(|r| r["success"] == true)
                })
                .count();
            out.push(format!("{up}/{} up", all.len()));
        }
        "prometheus" => {
            let all = part("targets")?["data"]["activeTargets"].as_array()?;
            let up = all.iter().filter(|t| t["health"] == "up").count();
            out.push(format!("{up}/{} targets up", all.len()));
        }
        "backrest" => {
            let v = part("summary")?;
            if let Some(n) = v["numPlans"].as_u64() {
                out.push(format!("{n} plans"));
            }
            if let Some(n) = v["numFailureLatest"].as_u64() {
                out.push(format!("{n} failed"));
            }
        }
        _ => return None,
    }
    (!out.is_empty()).then(|| out.join(" | "))
}

fn pretty_metric(label: &str, detail: &str) -> String {
    let name = match label {
        "CPU %" => "CPU",
        "Memory %" => "Mem",
        "Storage free bytes" => "Free",
        _ => "Net",
    };
    let Ok(n) = detail.parse::<f64>() else {
        return format!("{name} n/a");
    };
    match name {
        "CPU" | "Mem" => format!("{name} {n:.0}%"),
        "Free" => format!("Free {}", human_bytes(n)),
        _ => format!("Net {}/s", human_bytes(n)),
    }
}
/// The NAS figures, one short line each ("CPU 7%", "Free 13.8 TB"), wherever they are shown.
pub fn nas_figures(metrics: &Value) -> Vec<String> {
    metrics["items"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|i| pretty_metric(&safe(&i["label"]), &safe(&i["detail"])))
        .collect()
}
fn human_bytes(mut n: f64) -> String {
    let units = ["B", "kB", "MB", "GB", "TB", "PB"];
    let mut unit = 0;
    while n >= 1000.0 && unit < units.len() - 1 {
        n /= 1000.0;
        unit += 1;
    }
    if unit == 0 || n >= 100.0 {
        format!("{n:.0} {}", units[unit])
    } else {
        format!("{n:.1} {}", units[unit])
    }
}

/// The handheld home screen: Homepage's own groups and services, each with the live state
/// of its container, plus NAS figures and firing alerts. Only names and states leave the
/// node; Homepage's links, icons and widget settings never do.
#[must_use]
pub fn project_dashboard(
    homepage: &Value,
    containers: Option<&Value>,
    metrics: Option<&Value>,
    alerts: &[String],
) -> Value {
    project_dashboard_with(
        homepage,
        containers,
        metrics,
        alerts,
        &std::collections::HashMap::new(),
    )
}

/// [`project_dashboard`] plus a short number summary per service, keyed `Group/Service`.
#[must_use]
pub fn project_dashboard_with(
    homepage: &Value,
    containers: Option<&Value>,
    metrics: Option<&Value>,
    alerts: &[String],
    details: &std::collections::HashMap<String, String>,
) -> Value {
    let state_of = |service: &Value| -> (&'static str, Option<String>) {
        let (Some(name), Some(containers)) = (service["container"].as_str(), containers) else {
            return ("unknown", None);
        };
        let found = containers.as_array().into_iter().flatten().find(|c| {
            c["Names"][0]
                .as_str()
                .is_some_and(|n| n.trim_start_matches('/') == name)
        });
        let id = found
            .and_then(|c| c["Id"].as_str())
            .filter(|id| id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()))
            .map(str::to_owned);
        let unhealthy = found
            .and_then(|c| c["Status"].as_str())
            .is_some_and(|s| s.contains("unhealthy"));
        let state = match found {
            None => "stopped",
            Some(c) if c["State"] != "running" => "stopped",
            Some(_) if unhealthy => "unhealthy",
            Some(_) => "running",
        };
        (state, id)
    };
    let groups: Vec<_> = homepage
        .as_array()
        .into_iter()
        .flatten()
        .take(20)
        .map(|group| {
            if safe(&group["name"]).eq_ignore_ascii_case("alerts") {
                let firing: Vec<_> = alerts
                    .iter()
                    .take(10)
                    .map(|a| json!({"name": a.chars().take(40).collect::<String>(), "state": "unhealthy"}))
                    .collect();
                let services = if firing.is_empty() {
                    vec![json!({"name": "No alerts firing", "state": "running"})]
                } else {
                    firing
                };
                return json!({"name": "Alerts", "services": services});
            }
            let services: Vec<_> = group["services"]
                .as_array()
                .into_iter()
                .flatten()
                .take(30)
                .map(|service| {
                    let (state, id) = state_of(service);
                    let name: String = safe(&service["name"]).chars().take(40).collect();
                    let mut out = json!({"name": name, "state": state});
                    if let Some(id) = id {
                        out["id"] = json!(id);
                    }
                    let key = format!(
                        "{}/{}",
                        group["name"].as_str().unwrap_or(""),
                        service["name"].as_str().unwrap_or("")
                    );
                    if let Some(detail) = details.get(&key) {
                        out["detail"] = json!(detail.chars().take(80).collect::<String>());
                    }
                    out
                })
                .collect();
            json!({"name": safe(&group["name"]).chars().take(40).collect::<String>(), "services": services})
        })
        .collect();
    let stats = metrics.map(nas_figures).unwrap_or_default();
    let alerts: Vec<_> = alerts.iter().take(10).collect();
    json!({"stats": stats, "alerts": alerts, "groups": groups})
}

/// "Title (year)" from an Overseerr search hit or detail record, movie or show.
fn titled(item: &Value) -> String {
    let title = if item["title"].is_string() {
        safe(&item["title"])
    } else {
        safe(&item["name"])
    };
    let date = if item["releaseDate"].is_string() {
        safe(&item["releaseDate"])
    } else {
        safe(&item["firstAirDate"])
    };
    match date.get(..4) {
        Some(year) if year.bytes().all(|b| b.is_ascii_digit()) => format!("{title} ({year})"),
        _ => title,
    }
}
fn request_status_name(status: Option<u64>) -> &'static str {
    status.map_or("not requested", |s| match s {
        2 => "pending",
        3 => "processing",
        4 => "partly available",
        5 => "available",
        _ => "not available",
    })
}
fn identifier(s: &str) -> bool {
    !s.is_empty()
        && s.len() <= 128
        && s.bytes()
            .all(|b| b.is_ascii_alphanumeric() || b"-_.".contains(&b))
}
fn encode(s: &str) -> String {
    s.bytes()
        .map(|b| {
            if b.is_ascii_alphanumeric() || b"-_.~".contains(&b) {
                (b as char).to_string()
            } else {
                format!("%{b:02X}")
            }
        })
        .collect()
}

/// Decodes Docker's bounded multiplexed log body or a TTY plain-text body.
#[must_use]
pub fn decode_logs(bytes: &[u8]) -> String {
    let mut payload = Vec::new();
    if bytes.len() >= 8 && matches!(bytes[0], 1 | 2) && bytes[1..4] == [0, 0, 0] {
        let mut offset = 0;
        while offset < bytes.len() {
            if bytes.len() - offset < 8 {
                return "Log frame interrupted".into();
            }
            let length = u32::from_be_bytes(
                bytes[offset + 4..offset + 8]
                    .try_into()
                    .expect("checked log header"),
            ) as usize;
            offset += 8;
            if length > bytes.len() - offset {
                return "Log frame interrupted".into();
            }
            payload.extend_from_slice(&bytes[offset..offset + length]);
            offset += length;
        }
    } else {
        payload.extend_from_slice(bytes);
    }
    String::from_utf8_lossy(&payload)
        .chars()
        .filter(|c| !c.is_control() || matches!(c, '\n' | '\t'))
        .take(32 * 1024)
        .collect()
}

/// Durable at-most-once submission journal. An interrupted operation stays uncertain.
pub struct Journal {
    db: Db,
}
impl Journal {
    /// Shares the capabilities database.
    #[must_use]
    pub fn new(db: Db) -> Self {
        Self { db }
    }
    /// Reserves the id before executing any side effect.
    /// # Errors
    /// Persistence unavailable or conflicting request id.
    pub fn run(
        &self,
        request: &Value,
        effect: impl FnOnce() -> Result<Value, &'static str>,
    ) -> Result<Value, &'static str> {
        let id = request["request_id"]
            .as_str()
            .filter(|s| identifier(s))
            .ok_or("invalid_request")?;
        let fingerprint = request.to_string();
        let uncertain = json!({"request_id":id,"state":"uncertain","summary":"Check operation status and target before any new submission"});
        {
            let mut db = self.db.lock().map_err(|_| "storage")?;
            db.execute_batch(JOURNAL_SCHEMA).map_err(|_| "storage")?;
            let tx = db.transaction().map_err(|_| "storage")?;
            let found: Option<(String, String)> = tx
                .query_row(
                    "SELECT fingerprint,result FROM terminal_operations WHERE id=?1",
                    [id],
                    |r| Ok((r.get(0)?, r.get(1)?)),
                )
                .optional()
                .map_err(|_| "storage")?;
            if let Some((old, result)) = found {
                if old != fingerprint {
                    return Err("request_conflict");
                }
                return serde_json::from_str(&result).map_err(|_| "storage");
            }
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
                .to_string();
            tx.execute("INSERT INTO terminal_operations(id,fingerprint,result,created_ms) VALUES(?1,?2,?3,CAST(?4 AS INTEGER))",params![id,fingerprint,uncertain.to_string(),now]).map_err(|_|"storage")?;
            tx.commit().map_err(|_| "storage")?;
        }
        let result = match effect() {
            Ok(observed) => {
                let kind = request["kind"].as_str().unwrap_or("");
                let state = if kind == "job" {
                    observed["state"].as_str().unwrap_or("uncertain")
                } else {
                    "complete"
                };
                let summary = match kind {
                    "home" if observed["verified"] == true => {
                        format!("Observed state: {} (verified)", safe(&observed["state"]))
                    }
                    "home" => format!(
                        "Observed state: {}; intended state not verified",
                        safe(&observed["state"])
                    ),
                    "container" => {
                        format!("Observed container state: {}", safe(&observed["state"]))
                    }
                    "media" => format!(
                        "Media request: {}",
                        request_status(observed["status"].as_u64())
                    ),
                    "job" => format!("Job {state}; follow progress in Jobs"),
                    "favorite" => "Home favorites updated".into(),
                    _ => "Request submitted".into(),
                };
                json!({"request_id":id,"state":state,"summary":summary,"observed":observed})
            }
            Err(category) => {
                json!({"request_id":id,"state":"uncertain","summary":"Upstream did not confirm completion","error":category})
            }
        };
        self.db
            .lock()
            .map_err(|_| "storage")?
            .execute(
                "UPDATE terminal_operations SET result=?1 WHERE id=?2",
                params![result.to_string(), id],
            )
            .map_err(|_| "storage")?;
        Ok(result)
    }
    /// Reads recorded state without repeating the side effect.
    /// # Errors
    /// Missing id or persistence failure.
    pub fn get(&self, id: &str) -> Result<Value, &'static str> {
        if !identifier(id) {
            return Err("invalid_request");
        }
        let db = self.db.lock().map_err(|_| "storage")?;
        db.execute_batch(JOURNAL_SCHEMA).map_err(|_| "storage")?;
        let value: String = db
            .query_row(
                "SELECT result FROM terminal_operations WHERE id=?1",
                [id],
                |r| r.get(0),
            )
            .map_err(|_| "unknown_operation")?;
        serde_json::from_str(&value).map_err(|_| "storage")
    }
}

/// Limits entity exposure to explicitly selected favorites.
#[must_use]
pub fn project_home(value: &Value, favorites: &[String]) -> Value {
    let states = value.as_array().map(Vec::as_slice).unwrap_or(&[]);
    let items:Vec<_>=favorites.iter().filter_map(|id|states.iter().find(|item|item["entity_id"]==*id)).map(|item|json!({"id":item["entity_id"],"label":safe(&item["attributes"]["friendly_name"]),"detail":format!("{} {}",safe(&item["state"]),safe(&item["attributes"]["unit_of_measurement"]))})).collect();
    json!({"summary":"Home favorites","items":items})
}
/// Keeps the handheld's access and authorization path from disabling itself.
#[must_use]
pub fn protected_container(item: &Value) -> bool {
    let critical = |value: &Value| {
        let name = safe(value).to_ascii_lowercase();
        ["endora", "cloudflared", "portainer", "docker-observer"]
            .iter()
            .any(|critical| name.contains(critical))
    };
    item["Names"].as_array().into_iter().flatten().any(critical)
        || critical(&item["Image"])
        || critical(&item["Labels"]["com.docker.compose.service"])
}

fn request_status(value: Option<u64>) -> &'static str {
    match value {
        Some(1) => "pending approval",
        Some(2) => "approved",
        Some(3) => "declined",
        Some(4) => "failed",
        Some(5) => "completed",
        _ => "unknown",
    }
}

/// Settings-backed, fixed-route adapters for explicit administration.
pub struct Adapter {
    settings: Arc<dyn CapabilitySettingsRepository + Send + Sync>,
    journal: Journal,
    gate: Mutex<()>,
}
impl Adapter {
    /// Composed by the node, never by a model.
    #[must_use]
    pub fn new(settings: Arc<dyn CapabilitySettingsRepository + Send + Sync>, db: Db) -> Self {
        Self {
            settings,
            journal: Journal::new(db),
            gate: Mutex::new(()),
        }
    }
    fn setting(&self, id: &str, key: &str) -> Result<String, &'static str> {
        Ok(self
            .settings
            .all_settings()
            .map_err(|_| "configuration")?
            .into_iter()
            .find(|(s, k, _)| s == id && k == key)
            .map_or(String::new(), |(_, _, v)| v))
    }
    fn favorites(&self) -> Result<Vec<String>, &'static str> {
        let text = self.setting("terminal-home", "favorites")?;
        Ok(text
            .split(',')
            .map(str::trim)
            .filter(|s| identifier(s))
            .take(32)
            .map(str::to_owned)
            .collect())
    }
    fn call(
        &self,
        service: &str,
        method: &str,
        path: &str,
        body: Option<&Value>,
    ) -> Result<Value, &'static str> {
        let (id, origin_key, token_key, header) = if service == "home" {
            ("home_assistant".to_owned(), "url", "token", "Authorization")
        } else {
            (
                format!("terminal-{service}"),
                "origin",
                "api_key",
                "X-Api-Key",
            )
        };
        // An address typed as `host:8123` is read as `http://host:8123`, as the Home Assistant
        // skill reads it: the same setting cannot work in one place and not the other.
        let origin = super::as_url(&self.setting(&id, origin_key)?);
        let origin = origin.trim_end_matches('/');
        if origin.is_empty() {
            return Err("unconfigured");
        }
        super::homelab::validate_origin(origin)?;
        let key = self.setting(&id, token_key)?;
        if !matches!(service, "metrics" | "homepage") && key.is_empty() {
            return Err("unconfigured");
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(8)))
            .max_redirects(0)
            .proxy(None)
            .http_status_as_error(false)
            .build()
            .into();
        let credential = if service == "home" {
            format!("Bearer {key}")
        } else {
            key
        };
        let mut response = if method == "GET" {
            agent
                .get(format!("{origin}{path}"))
                .header(header, &credential)
                .call()
                .map_err(|_| "connection")?
        } else {
            let mut request = agent
                .post(format!("{origin}{path}"))
                .header(header, &credential);
            if service == "overseerr" {
                let user = self.setting("terminal-overseerr", "request_user")?;
                if user.is_empty() || !user.bytes().all(|b| b.is_ascii_digit()) || user == "0" {
                    return Err("request_user_unconfigured");
                }
                request = request.header("X-Api-User", &user);
            }
            request
                .send_json(body.unwrap_or(&json!({})))
                .map_err(|_| "connection")?
        };
        match response.status().as_u16() {
            200..=299 => {}
            401 | 403 => return Err("authentication"),
            _ => return Err("upstream"),
        }
        if response.status().as_u16() == 204 {
            return Ok(json!({"submitted":true}));
        }
        if service == "portainer" && path.contains("/logs?") {
            let mut bytes = Vec::new();
            response
                .body_mut()
                .as_reader()
                .take(64 * 1024 + 1)
                .read_to_end(&mut bytes)
                .map_err(|_| "protocol")?;
            if bytes.len() > 64 * 1024 {
                return Err("protocol");
            }
            let output = decode_logs(&bytes);
            return Ok(json!({"summary":output,"items":[]}));
        }
        response
            .body_mut()
            .with_config()
            .limit(2 * 1024 * 1024)
            .read_json()
            .map_err(|_| "protocol")
    }
    fn endpoint(&self) -> Result<String, &'static str> {
        let id = self.setting("terminal-portainer", "endpoint")?;
        if id.is_empty() || !id.bytes().all(|b| b.is_ascii_digit()) {
            return Err("unconfigured");
        }
        Ok(id)
    }
    /// One short summary per service whose Homepage widget the handheld understands. Homepage
    /// holds the service credentials; the node asks it through its own widget proxy, in
    /// parallel, and a failing widget simply has no detail.
    fn widget_details(&self, homepage: &Value) -> std::collections::HashMap<String, String> {
        let mut jobs: Vec<(String, String, String, String)> = Vec::new();
        for group in homepage.as_array().into_iter().flatten() {
            for service in group["services"].as_array().into_iter().flatten() {
                let (Some(g), Some(n), Some(kind)) = (
                    group["name"].as_str(),
                    service["name"].as_str(),
                    service["widgets"][0]["type"].as_str(),
                ) else {
                    continue;
                };
                if !widget_endpoints(kind).is_empty() {
                    jobs.push((
                        format!("{g}/{n}"),
                        kind.to_owned(),
                        g.to_owned(),
                        n.to_owned(),
                    ));
                }
            }
        }
        jobs.truncate(24);
        std::thread::scope(|scope| {
            let handles: Vec<_> = jobs
                .iter()
                .map(|(key, kind, group, service)| {
                    scope.spawn(move || {
                        let parts: Vec<(String, Value)> = widget_endpoints(kind)
                            .iter()
                            .filter_map(|endpoint| {
                                let path = format!(
                                    "/api/services/proxy?type={}&group={}&service={}&endpoint={}",
                                    encode(kind),
                                    encode(group),
                                    encode(service),
                                    encode(endpoint)
                                );
                                self.call("homepage", "GET", &path, None)
                                    .ok()
                                    .map(|v| ((*endpoint).to_owned(), v))
                            })
                            .collect();
                        widget_summary(kind, &parts).map(|text| (key.clone(), text))
                    })
                })
                .collect();
            handles
                .into_iter()
                .filter_map(|h| h.join().ok().flatten())
                .collect()
        })
    }
    fn containers(&self) -> Result<Value, &'static str> {
        let id = self.endpoint()?;
        self.call(
            "portainer",
            "GET",
            &format!("/api/endpoints/{id}/docker/containers/json?all=true"),
            None,
        )
    }
    fn metrics(&self) -> Result<Value, &'static str> {
        let instance = self.setting("terminal-metrics", "instance")?;
        if instance.is_empty() {
            return Err("unconfigured");
        }
        let queries = metric_queries(&instance)?;
        let items: Vec<_> = queries
            .into_iter()
            .map(|(name, query)| {
                let result = self.call(
                    "metrics",
                    "GET",
                    &format!("/api/v1/query?query={}", encode(&query)),
                    None,
                );
                let value = result
                    .ok()
                    .and_then(|v| {
                        v["data"]["result"][0]["value"][1]
                            .as_str()
                            .map(str::to_owned)
                    })
                    .unwrap_or("unavailable".into());
                json!({"label":name,"detail":value})
            })
            .collect();
        Ok(json!({"summary":"NAS metrics","items":items}))
    }
    fn jobs(&self, method: &str, path: &str, body: Option<&Value>) -> Result<Value, &'static str> {
        let key = self.setting("terminal-jobs", "api_key")?;
        if key.is_empty() {
            return Err("unconfigured");
        }
        let mut socket = std::os::unix::net::UnixStream::connect("/run/endora-jobs/agent.sock")
            .map_err(|_| "connection")?;
        socket
            .set_read_timeout(Some(Duration::from_secs(8)))
            .map_err(|_| "connection")?;
        socket
            .set_write_timeout(Some(Duration::from_secs(8)))
            .map_err(|_| "connection")?;
        let bytes = body.map(Value::to_string).unwrap_or_default();
        if key.contains(['\r', '\n']) {
            return Err("configuration");
        }
        write!(socket,"{method} {path} HTTP/1.0\r\nHost: localhost\r\nAuthorization: Bearer {key}\r\nContent-Type: application/json\r\nContent-Length: {}\r\n\r\n{bytes}",bytes.len()).map_err(|_|"connection")?;
        let mut response = Vec::new();
        socket
            .take(128 * 1024 + 1)
            .read_to_end(&mut response)
            .map_err(|_| "connection")?;
        if response.len() > 128 * 1024 {
            return Err("protocol");
        }
        let offset = response
            .windows(4)
            .position(|b| b == b"\r\n\r\n")
            .ok_or("protocol")?;
        if !response.starts_with(b"HTTP/1.0 200") {
            return Err("upstream");
        }
        serde_json::from_slice(&response[offset + 4..]).map_err(|_| "protocol")
    }
    fn effect(&self, request: &Value) -> Result<Value, &'static str> {
        let target = request["target"].as_str().ok_or("invalid_target")?;
        let action = request["action"].as_str().ok_or("invalid_action")?;
        match request["kind"].as_str() {
            Some("favorite") => {
                let mut favorites = self.favorites()?;
                if action == "add" {
                    let states = self.call("home", "GET", "/api/states", None)?;
                    if !states
                        .as_array()
                        .into_iter()
                        .flatten()
                        .any(|item| item["entity_id"] == target)
                    {
                        return Err("target_not_allowed");
                    }
                    if !favorites.iter().any(|id| id == target) {
                        if favorites.len() >= 32 {
                            return Err("favorite_limit");
                        }
                        favorites.push(target.to_owned());
                    }
                } else if action == "remove" {
                    favorites.retain(|id| id != target);
                } else {
                    return Err("action_not_allowed");
                }
                self.settings
                    .set_setting("terminal-home", "favorites", &favorites.join(","))
                    .map_err(|_| "configuration")?;
                Ok(json!({"summary":"Home favorites updated"}))
            }
            Some("home") => {
                if !self.favorites()?.iter().any(|id| id == target) {
                    return Err("target_not_allowed");
                }
                let domain = target.split('.').next().ok_or("invalid_target")?;
                let service = match (domain, action) {
                    ("light" | "switch" | "input_boolean", "on") | ("scene", "scene") => "turn_on",
                    ("light" | "switch" | "input_boolean", "off") => "turn_off",
                    _ => return Err("action_not_allowed"),
                };
                self.call(
                    "home",
                    "POST",
                    &format!("/api/services/{domain}/{service}"),
                    Some(&json!({"entity_id":target})),
                )?;
                let readback = self.call("home", "GET", &format!("/api/states/{target}"), None)?;
                Ok(
                    json!({"state":safe(&readback["state"]),"verified":match action{"on"=>readback["state"]=="on","off"=>readback["state"]=="off",_=>false}}),
                )
            }
            Some("media") => {
                let id: u64 = target.parse().map_err(|_| "invalid_target")?;
                if !matches!(action, "movie" | "tv") {
                    return Err("action_not_allowed");
                }
                let result = self.call(
                    "overseerr",
                    "POST",
                    "/api/v1/request",
                    Some(&json!({"mediaType":action,"mediaId":id,"seasons":"all"})),
                )?;
                Ok(json!({"id":result["id"],"status":result["status"]}))
            }
            Some("container") => {
                if !matches!(action, "start" | "stop" | "restart")
                    || target.len() != 64
                    || !target.bytes().all(|b| b.is_ascii_hexdigit())
                {
                    return Err("action_not_allowed");
                }
                let containers = self.containers()?;
                let item = containers
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|item| item["Id"] == target)
                    .ok_or("target_not_allowed")?;
                if protected_container(item) {
                    return Err("protected_control_path");
                }
                let endpoint = self.endpoint()?;
                self.call(
                    "portainer",
                    "POST",
                    &format!("/api/endpoints/{endpoint}/docker/containers/{target}/{action}"),
                    Some(&json!({})),
                )?;
                let observed = self.containers()?;
                let item = observed
                    .as_array()
                    .into_iter()
                    .flatten()
                    .find(|i| i["Id"] == target)
                    .ok_or("readback_unavailable")?;
                Ok(json!({"state":safe(&item["State"])}))
            }
            Some("job") if action == "run" => self.jobs(
                "POST",
                "/jobs",
                Some(&json!({"job":target,"request_id":request["request_id"]})),
            ),
            _ => Err("action_not_allowed"),
        }
    }
}
impl HomeConsole for Adapter {
    fn read(&self, view: &str, query: &str) -> Result<Value, &'static str> {
        if query.len() > 256 {
            return Err("invalid_query");
        }
        match view {
            "home-catalog" => {
                let result = self.call("home", "GET", "/api/states", None)?;
                let favorites = self.favorites()?;
                let mut states: Vec<_> = result
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|item| item["entity_id"].as_str().is_some_and(identifier))
                    .collect();
                states.sort_by_key(|item| {
                    !matches!(
                        item["entity_id"].as_str().unwrap_or("").split('.').next(),
                        Some("light" | "switch" | "scene")
                    )
                });
                let selected:Vec<_>=states.into_iter().filter(|item|query.is_empty()||safe(&item["attributes"]["friendly_name"]).to_lowercase().contains(&query.to_lowercase())).take(100).map(|item|json!({"id":item["entity_id"],"label":safe(&item["attributes"]["friendly_name"]),"detail":safe(&item["state"]),"favorite":favorites.iter().any(|id|item["entity_id"]==*id)})).collect();
                Ok(
                    json!({"summary":"Home catalog | Space adds/removes a favorite","items":selected}),
                )
            }
            "home" => {
                let favorites = self.favorites()?;
                if favorites.is_empty() {
                    return Ok(
                        json!({"summary":"Choose favorites from the catalog with C","items":[]}),
                    );
                }
                let result = self.call("home", "GET", "/api/states", None)?;
                Ok(project_home(&result, &favorites))
            }
            "media" => {
                let result = self.call(
                    "overseerr",
                    "GET",
                    &format!("/api/v1/search?query={}&page=1", encode(query)),
                    None,
                )?;
                let items: Vec<_> = result["results"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter(|item| matches!(item["mediaType"].as_str(), Some("movie" | "tv")))
                    .take(20)
                    .map(|item| {
                        let kind = safe(&item["mediaType"]);
                        let availability =
                            request_status_name(item["mediaInfo"]["status"].as_u64());
                        json!({
                            "id": item["id"].as_u64().map_or(String::new(), |id| id.to_string()),
                            "media_type": item["mediaType"],
                            "label": titled(item),
                            "detail": format!("{kind} | Availability: {availability}"),
                        })
                    })
                    .collect();
                Ok(json!({"summary":"Up/Down to pick, Enter to request","items":items}))
            }
            "requests" => {
                let result = self.call(
                    "overseerr",
                    "GET",
                    "/api/v1/request?take=20&skip=0&sort=added",
                    None,
                )?;
                let items: Vec<_> = result["results"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .take(10)
                    .map(|item| {
                        let kind = safe(&item["type"]);
                        let tmdb = item["media"]["tmdbId"].as_u64();
                        // Overseerr's request list carries only the TMDB number; ask for the
                        // title. A failed lookup keeps the row, with the number as before.
                        let label = tmdb
                            .filter(|_| matches!(kind.as_str(), "movie" | "tv"))
                            .and_then(|id| {
                                self.call("overseerr", "GET", &format!("/api/v1/{kind}/{id}"), None)
                                    .ok()
                            })
                            .map(|detail| titled(&detail))
                            .filter(|label| !label.is_empty())
                            .unwrap_or_else(|| format!("{kind} #{}", tmdb.unwrap_or(0)));
                        json!({
                            "id": tmdb.map_or(String::new(), |id| id.to_string()),
                            "media_type": item["type"],
                            "label": label,
                            "detail": format!("{kind} | {}", request_status(item["status"].as_u64())),
                        })
                    })
                    .collect();
                Ok(json!({"summary":"Recent media requests","items":items}))
            }
            "containers" => {
                let result = self.containers()?;
                let items:Vec<_>=result.as_array().into_iter().flatten().take(100).map(|item|json!({"id":safe(&item["Id"]),"label":safe(&item["Names"][0]).trim_start_matches('/'),"detail":format!("{} {}",safe(&item["State"]),safe(&item["Status"])),"protected":protected_container(item)})).collect();
                let summary = self
                    .metrics()
                    .map(|m| nas_figures(&m).join(" | "))
                    .unwrap_or_default();
                Ok(json!({"summary":summary,"items":items}))
            }
            "logs" => {
                if query.len() != 64 || !query.bytes().all(|b| b.is_ascii_hexdigit()) {
                    return Err("invalid_target");
                }
                let endpoint = self.endpoint()?;
                self.call("portainer","GET",&format!("/api/endpoints/{endpoint}/docker/containers/{query}/logs?stdout=true&stderr=true&tail=100&timestamps=true"),None)
            }
            "metrics" => self.metrics(),
            "dashboard" => {
                let homepage = self.call("homepage", "GET", "/api/services", None)?;
                let containers = self.containers().ok();
                let metrics = self.metrics().ok();
                let query = encode("ALERTS{alertstate=\"firing\"}");
                let alerts: Vec<String> = self
                    .call(
                        "metrics",
                        "GET",
                        &format!("/api/v1/query?query={query}"),
                        None,
                    )
                    .ok()
                    .and_then(|v| v["data"]["result"].as_array().cloned())
                    .into_iter()
                    .flatten()
                    .map(|a| safe(&a["metric"]["alertname"]))
                    .filter(|a| !a.is_empty())
                    .collect();
                let details = self.widget_details(&homepage);
                Ok(project_dashboard_with(
                    &homepage,
                    containers.as_ref(),
                    metrics.as_ref(),
                    &alerts,
                    &details,
                ))
            }
            // What the agenda loop watches (ADR 0103): the firing alerts and the failing uptime
            // checks, both from Prometheus. Fixed queries; nothing the caller chooses.
            "watch" => {
                let alerts = self.call("metrics", "GET", "/api/v1/alerts", None)?;
                let query = encode("gatus_results_endpoint_success == 0");
                let down = self.call(
                    "metrics",
                    "GET",
                    &format!("/api/v1/query?query={query}"),
                    None,
                )?;
                Ok(project_watch(&alerts, &down))
            }
            "jobs" => self.jobs("GET", "/jobs", None),
            "job" if identifier(query) => self.jobs("GET", &format!("/jobs/{query}"), None),
            _ => Err("unknown_view"),
        }
    }
    fn act(&self, request: &Value) -> Result<Value, &'static str> {
        crate::application::validate_console_action(request)?;
        if !request["target"].as_str().is_some_and(identifier) {
            return Err("invalid_target");
        }
        let _gate = self.gate.lock().map_err(|_| "busy")?;
        self.journal.run(request, || self.effect(request))
    }
    fn operation(&self, id: &str) -> Result<Value, &'static str> {
        self.journal.get(id)
    }
}

const fn field(
    key: &'static str,
    label: &'static str,
    secret: bool,
    optional: bool,
) -> super::SettingSpec {
    super::SettingSpec {
        key,
        label,
        secret,
        optional,
    }
}
const OVERSEERR_SETTINGS: &[super::SettingSpec] = &[
    field(
        "origin",
        "Overseerr address, e.g. http://192.168.1.10:5055",
        false,
        false,
    ),
    field(
        "api_key",
        "Overseerr API key (Settings > General)",
        true,
        false,
    ),
    field(
        "request_user",
        "Overseerr user ID that requests are filed as (numeric)",
        false,
        false,
    ),
];
const PIHOLE_SETTINGS: &[super::SettingSpec] = &[
    field(
        "origin",
        "Pi-hole address, e.g. http://192.168.1.10",
        false,
        false,
    ),
    field(
        "password",
        "Pi-hole app password (Settings > Web interface / API > Configure app password)",
        true,
        false,
    ),
];
const UNIFI_SETTINGS: &[super::SettingSpec] = &[
    field(
        "origin",
        "UniFi Network address, e.g. https://192.168.1.10:8443",
        false,
        false,
    ),
    field(
        "username",
        "A View Only local admin's username",
        false,
        false,
    ),
    field("password", "That admin's password", true, false),
    field(
        "site",
        "Site name (blank for the default site)",
        false,
        true,
    ),
];
const PORTAINER_SETTINGS: &[super::SettingSpec] = &[
    field(
        "origin",
        "Portainer address, e.g. http://192.168.1.10:9000",
        false,
        false,
    ),
    field(
        "api_key",
        "Portainer access token (My account > Access tokens)",
        true,
        false,
    ),
    field(
        "endpoint",
        "Portainer environment ID (numeric)",
        false,
        false,
    ),
];
const HOMEPAGE_SETTINGS: &[super::SettingSpec] = &[field(
    "origin",
    "Homepage address, e.g. http://192.168.1.10:11111",
    false,
    false,
)];
const DOCKER_SETTINGS: &[super::SettingSpec] = &[field(
    "origin",
    "Docker observer address (the restricted proxy, e.g. http://127.0.0.1:9876)",
    false,
    false,
)];
const ARR_SETTINGS: &[super::SettingSpec] = &[
    field(
        "origin",
        "Address, e.g. http://192.168.1.10:8989",
        false,
        false,
    ),
    field("api_key", "API key (Settings > General)", true, false),
];
const METRICS_SETTINGS: &[super::SettingSpec] = &[
    field(
        "origin",
        "Prometheus address, e.g. http://192.168.1.10:9090",
        false,
        false,
    ),
    field(
        "instance",
        "NAS exporter instance label in Prometheus",
        false,
        false,
    ),
];

/// The four NAS figures, as PromQL over the Telegraf metrics the node's Prometheus scrapes.
/// What the agenda loop needs from Prometheus's alerts and Gatus's results: each firing
/// alert's name, severity, what it is about and its summary, and the names of the failing
/// checks. Only those: no other label (a drive's serial number, an address) is kept.
#[must_use]
pub fn project_watch(alerts: &Value, down: &Value) -> Value {
    const ABOUT: [&str; 7] = [
        "drive",
        "path",
        "plan",
        "db",
        "container_name",
        "name",
        "job",
    ];
    let firing: Vec<Value> = alerts["data"]["alerts"]
        .as_array()
        .into_iter()
        .flatten()
        .filter(|a| a["state"] == "firing")
        .take(100)
        .map(|a| {
            let labels = &a["labels"];
            let target = ABOUT
                .iter()
                .map(|key| safe(&labels[*key]))
                .find(|v| !v.is_empty())
                .unwrap_or_default();
            json!({
                "name": safe(&labels["alertname"]),
                "severity": safe(&labels["severity"]),
                "target": target,
                "summary": safe(&a["annotations"]["summary"]),
            })
        })
        .filter(|a| a["name"] != "")
        .collect();
    let failing: Vec<String> = down["data"]["result"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|r| safe(&r["metric"]["name"]))
        .filter(|n| !n.is_empty())
        .take(100)
        .collect();
    json!({ "alerts": firing, "checks_failing": failing })
}

/// `host` is the Telegraf host label; it is quoted as JSON so it cannot leave its string.
///
/// # Errors
/// `"configuration"` if the label cannot be quoted.
pub fn metric_queries(host: &str) -> Result<Vec<(&'static str, String)>, &'static str> {
    let label = serde_json::to_string(host).map_err(|_| "configuration")?;
    Ok(vec![
        (
            "CPU %",
            format!("100-cpu_usage_idle{{host={label},cpu=\"cpu-total\"}}"),
        ),
        ("Memory %", format!("mem_used_percent{{host={label}}}")),
        (
            "Storage free bytes",
            format!(
                "sum(max by (device)(disk_free{{host={label},fstype=~\"ext4|xfs|btrfs|zfs|fuseblk\"}}))"
            ),
        ),
        (
            "Network receive bytes/s",
            format!("sum(rate(net_bytes_recv{{host={label},interface!=\"lo\"}}[5m]))"),
        ),
    ])
}

/// Pi-hole v6's settings test: exchanges the app password for a session, then gives the
/// session back, since Pi-hole allows only a few at once. Reports a category, never what
/// Pi-hole said or the password.
fn pihole_signs_in(origin: &str, password: &str) -> Result<(), super::CapabilityError> {
    let unavailable = |m: &str| super::CapabilityError::Unavailable(m.into());
    if password.is_empty() {
        return Err(unavailable("the app password is not set"));
    }
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .max_redirects(0)
        .proxy(None)
        .http_status_as_error(false)
        .build()
        .into();
    let mut response = agent
        .post(format!("{origin}/api/auth"))
        .send_json(json!({ "password": password }))
        .map_err(|_| unavailable("connection"))?;
    match response.status().as_u16() {
        200..=299 => {}
        401 | 403 => return Err(unavailable("authentication")),
        _ => return Err(unavailable("upstream")),
    }
    let mut bytes = Vec::new();
    response
        .body_mut()
        .as_reader()
        .take(16 * 1024)
        .read_to_end(&mut bytes)
        .map_err(|_| unavailable("protocol"))?;
    let body: Value = serde_json::from_slice(&bytes).map_err(|_| unavailable("protocol"))?;
    if body["session"]["valid"] != true {
        return Err(unavailable("authentication"));
    }
    if let Some(sid) = body["session"]["sid"].as_str().filter(|s| !s.is_empty()) {
        let _ = agent
            .delete(format!("{origin}/api/auth"))
            .header("X-FTL-SID", sid)
            .call();
    }
    Ok(())
}

/// The self-hosted UniFi Network Application's settings test: signs in with the View Only local
/// admin, then signs out. UniFi answers only over HTTPS with a certificate it made itself, and
/// the owner chose not to check it for this address (a LAN service, a view-only account).
/// Reports a category, never what UniFi said or the password.
fn unifi_signs_in(
    origin: &str,
    username: &str,
    password: &str,
) -> Result<(), super::CapabilityError> {
    let unavailable = |m: &str| super::CapabilityError::Unavailable(m.into());
    if username.is_empty() || password.is_empty() {
        return Err(unavailable("the username or password is not set"));
    }
    let tls = ureq::tls::TlsConfig::builder()
        .disable_verification(true)
        .build();
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(8)))
        .max_redirects(0)
        .proxy(None)
        .http_status_as_error(false)
        .tls_config(tls)
        .build()
        .into();
    let response = agent
        .post(format!("{origin}/api/login"))
        .send_json(json!({ "username": username, "password": password }))
        .map_err(|_| unavailable("connection"))?;
    match response.status().as_u16() {
        200..=299 => {}
        400 | 401 | 403 => return Err(unavailable("authentication")),
        _ => return Err(unavailable("upstream")),
    }
    // The session cookie, given back so the test leaves no session behind.
    let session = response
        .headers()
        .get_all("set-cookie")
        .iter()
        .filter_map(|v| v.to_str().ok())
        .filter_map(|v| v.split(';').next())
        .find(|c| c.starts_with("unifises=") || c.starts_with("TOKEN="))
        .map(str::to_owned);
    if let Some(cookie) = session {
        let _ = agent
            .post(format!("{origin}/api/logout"))
            .header("Cookie", &cookie)
            .send_empty();
    }
    Ok(())
}

/// What a [`HandheldSource`] says it needs: it only holds settings.
const SETTINGS_ONLY_NEEDS: &str = "set up here; the handheld console uses it";

/// Whether an entry only holds settings for the handheld (and the chat skills that borrow
/// them) and is never something chat runs. Its on/off switch does nothing, and it is never
/// `configured`, so the settings screen shows it as set up or not by its settings alone.
#[must_use]
pub fn settings_only(info: &super::CapabilityInfo) -> bool {
    !info.configured && info.needs == SETTINGS_ONLY_NEEDS
}

/// A service only the handheld console reads. It exists so its address and key can be
/// entered in the same settings screen as every other integration; it is never code the
/// model can run, which `configured: false` and a failing `invoke` guarantee (ADR 0093).
pub struct HandheldSource {
    id: &'static str,
    name: &'static str,
    description: &'static str,
    category: &'static str,
    settings: &'static [super::SettingSpec],
}

/// The handheld console's own integrations: the connections under Settings → Connections →
/// Home lab, named for the service they hold the address and key of.
#[must_use]
pub fn handheld_sources() -> Vec<Arc<dyn super::Capability>> {
    [
        ("terminal-homepage", "Homepage", "The handheld's home screen mirrors your Homepage groups and services. Not used by chat.", "information", HOMEPAGE_SETTINGS),
        ("terminal-overseerr", "Overseerr", "Media search and requests on the handheld. Not used by chat.", "media", OVERSEERR_SETTINGS),
        ("terminal-portainer", "Portainer", "Container list, logs and controls on the handheld. Chat reads the container list from it; logs and controls stay on the handheld.", "information", PORTAINER_SETTINGS),
        ("homelab-docker", "Docker", "The container summary on the handheld's Overview page, and what chat reads about your containers.", "information", DOCKER_SETTINGS),
        ("homelab-sonarr", "Sonarr", "The download queue on the handheld's Overview page, and what chat reads about it.", "media", ARR_SETTINGS),
        ("homelab-radarr", "Radarr", "The download queue on the handheld's Overview page, and what chat reads about it.", "media", ARR_SETTINGS),
        ("terminal-metrics", "Prometheus", "NAS figures (load, memory, disk) on the handheld, and what chat reads about the NAS.", "information", METRICS_SETTINGS),
        ("network-pihole", "Pi-hole", "Your network's DNS and what it blocks, for what Endora reads about the network. Only ever read: Pi-hole's app password could change settings, and Endora never does.", "information", PIHOLE_SETTINGS),
        ("network-unifi", "UniFi", "Your network's devices and Wi-Fi, for what Endora reads about the network. Give it a View Only local admin. UniFi's own certificate is not checked.", "information", UNIFI_SETTINGS),
    ]
    .into_iter()
    .map(|(id, name, description, category, settings)| {
        Arc::new(HandheldSource { id, name, description, category, settings })
            as Arc<dyn super::Capability>
    })
    .collect()
}

impl super::Capability for HandheldSource {
    fn info(&self) -> super::CapabilityInfo {
        super::CapabilityInfo {
            id: self.id,
            name: self.name,
            description: self.description,
            category: self.category,
            reaches_external: true,
            reversibility: super::Reversibility::Observe,
            configured: false,
            needs: SETTINGS_ONLY_NEEDS,
            settings: self.settings,
            wants_place: false,
            borrows_from: None,
            third_party: false,
            input_schema: None,
        }
    }
    /// The settings screen's Test button: one authenticated, read-only request to the
    /// service. It reports a category (never upstream text or the key). The model cannot
    /// reach this: the entry is never `configured`, so it is never offered (ADR 0093).
    fn invoke(
        &self,
        _input: &Value,
        settings: &super::CapabilitySettings,
    ) -> Result<Value, super::CapabilityError> {
        let unavailable = |m: &str| super::CapabilityError::Unavailable(m.into());
        let get = |k: &str| settings.get(k).map_or("", |v| v.trim());
        let origin = get("origin").trim_end_matches('/');
        if origin.is_empty() {
            return Err(unavailable("the address is not set"));
        }
        super::homelab::validate_origin(origin)
            .map_err(|_| unavailable("configuration: use http://host:port with no path"))?;
        if self.id == "network-pihole" {
            return pihole_signs_in(origin, get("password"))
                .map(|()| json!({"ok": true, "checked": self.name}));
        }
        if self.id == "network-unifi" {
            return unifi_signs_in(origin, get("username"), get("password"))
                .map(|()| json!({"ok": true, "checked": self.name}));
        }
        let (path, key) = match self.id {
            "terminal-overseerr" => ("/api/v1/auth/me".to_owned(), get("api_key")),
            "terminal-portainer" => ("/api/endpoints".to_owned(), get("api_key")),
            "homelab-docker" => ("/version".to_owned(), ""),
            "terminal-homepage" => ("/api/services".to_owned(), ""),
            "homelab-sonarr" | "homelab-radarr" => {
                ("/api/v3/system/status".to_owned(), get("api_key"))
            }
            _ => {
                let host = get("instance");
                if host.is_empty() {
                    return Err(unavailable("the Telegraf host label is not set"));
                }
                let queries = metric_queries(host).map_err(unavailable)?;
                (format!("/api/v1/query?query={}", encode(&queries[1].1)), "")
            }
        };
        let keyless = matches!(
            self.id,
            "terminal-metrics" | "homelab-docker" | "terminal-homepage"
        );
        if !keyless && key.is_empty() {
            return Err(unavailable("the API key is not set"));
        }
        let agent: ureq::Agent = ureq::Agent::config_builder()
            .timeout_global(Some(Duration::from_secs(8)))
            .max_redirects(0)
            .proxy(None)
            .http_status_as_error(false)
            .build()
            .into();
        let mut request = agent.get(format!("{origin}{path}"));
        if !key.is_empty() {
            request = request.header("X-Api-Key", key);
        }
        let mut response = request.call().map_err(|_| unavailable("connection"))?;
        match response.status().as_u16() {
            200..=299 => {}
            401 | 403 => return Err(unavailable("authentication")),
            _ => return Err(unavailable("upstream")),
        }
        if self.id == "terminal-metrics" {
            let mut bytes = Vec::new();
            response
                .body_mut()
                .as_reader()
                .take(64 * 1024)
                .read_to_end(&mut bytes)
                .map_err(|_| unavailable("protocol"))?;
            let body: Value =
                serde_json::from_slice(&bytes).map_err(|_| unavailable("protocol"))?;
            if body["data"]["result"].as_array().is_none_or(Vec::is_empty) {
                return Err(unavailable("no data for that host label"));
            }
        }
        Ok(json!({"ok": true, "checked": self.name}))
    }
}
