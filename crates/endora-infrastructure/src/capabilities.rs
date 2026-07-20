//! Butler **capabilities** (skills) — the modules the butler can reach for
//! (ADR 0019 §capabilities). Each is a self-contained unit that declares what it
//! does, its **autonomy level** (may it act, or must it ask?), whether it
//! **reaches outside the machine**, and whether it is **configured** (ready, or
//! waiting on a key / model / data source). Consequential or unconfigured skills
//! are surfaced but gated by the policy layer — the butler proposes, the person
//! authorizes; the model is never the enforcement boundary.
//!
//! MCP note: these are the internal `Capability` interface; an MCP server is one
//! way to back a capability (ADR 0019 §3). The registry here is the substrate a
//! future MCP host adapter plugs into.

use std::sync::Arc;
use std::time::Duration;

use endora_domain::AutonomyLevel;
use serde_json::{Value, json};

/// Static metadata describing a capability to the person and the policy layer.
#[derive(Debug, Clone)]
pub struct CapabilityInfo {
    /// Stable slug, e.g. `"weather"`.
    pub id: &'static str,
    /// Human name, e.g. `"Weather"`.
    pub name: &'static str,
    /// One-line description of what it does.
    pub description: &'static str,
    /// Grouping for the UI: `information`, `safety`, `travel`, `media`, `presence`.
    pub category: &'static str,
    /// Whether invoking it sends data outside this machine.
    pub reaches_external: bool,
    /// May it act on its own (read-only/low-stakes), or must it ask first?
    pub autonomy: AutonomyLevel,
    /// Ready to use, or waiting on setup (an API key, a model, a data source).
    pub configured: bool,
    /// If not configured, a short note on what it needs.
    pub needs: &'static str,
}

/// Why a capability call failed.
#[derive(Debug, Clone)]
pub enum CapabilityError {
    /// The capability is not set up (missing key/model/source) or unreachable.
    Unavailable(String),
    /// The input was missing or malformed.
    BadInput(String),
}

impl std::fmt::Display for CapabilityError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Unavailable(m) => write!(f, "unavailable: {m}"),
            Self::BadInput(m) => write!(f, "bad input: {m}"),
        }
    }
}

/// A butler skill: metadata plus a synchronous `invoke` (run on a blocking
/// worker in the node, like the model calls).
pub trait Capability: Send + Sync {
    /// Static description of this capability.
    fn info(&self) -> CapabilityInfo;

    /// Runs the capability with JSON `input`, returning a JSON result.
    ///
    /// # Errors
    /// [`CapabilityError`] if the input is bad or the capability is unavailable.
    fn invoke(&self, input: &Value) -> Result<Value, CapabilityError>;
}

/// Builds the default set of capabilities the node offers. Read-only information
/// skills are ready; the rest are declared but await configuration, so they show
/// up as modules to enable rather than silently missing.
#[must_use]
pub fn default_capabilities() -> Vec<Arc<dyn Capability>> {
    vec![
        Arc::new(WeatherCapability),
        Arc::new(WebFetchCapability),
        Arc::new(ImageReviewCapability::from_env()),
        Arc::new(LocalEventsCapability),
        Arc::new(FlightSearchCapability),
        Arc::new(LocationLogCapability),
        Arc::new(SafetyAlertsCapability),
        Arc::new(IncidentScannerCapability),
    ]
}

// ---- helpers ---------------------------------------------------------------

fn agent() -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .build()
        .into()
}

/// GETs a URL and returns the body as text (size-capped), for the info skills.
fn http_get_text(url: &str, max_bytes: usize) -> Result<String, CapabilityError> {
    use std::io::Read;
    let mut resp = agent()
        .get(url)
        .call()
        .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    let mut buf = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(max_bytes as u64)
        .read_to_end(&mut buf)
        .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

fn str_field<'a>(input: &'a Value, key: &str) -> Result<&'a str, CapabilityError> {
    input
        .get(key)
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .ok_or_else(|| CapabilityError::BadInput(format!("missing '{key}'")))
}

/// GET with an explicit `User-Agent` (some APIs, e.g. the US NWS, require one).
fn http_get_text_ua(url: &str, ua: &str, max_bytes: usize) -> Result<String, CapabilityError> {
    use std::io::Read;
    let mut resp = agent()
        .get(url)
        .header("User-Agent", ua)
        .call()
        .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    let mut buf = Vec::new();
    resp.body_mut()
        .as_reader()
        .take(max_bytes as u64)
        .read_to_end(&mut buf)
        .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// Resolves `{lat, lon}` or `{location}` from an input to a `(lat, lon, place)`,
/// geocoding a place name via Open-Meteo (no key). Shared by the location skills.
fn resolve_point(input: &Value) -> Result<(f64, f64, String), CapabilityError> {
    if let (Some(lat), Some(lon)) = (
        input.get("lat").and_then(Value::as_f64),
        input.get("lon").and_then(Value::as_f64),
    ) {
        let place = input
            .get("location")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_owned();
        return Ok((lat, lon, place));
    }
    let q = str_field(input, "location")?;
    // A bare US ZIP ("28277") — common when the person types their postcode — is
    // not resolvable by the place-name geocoder, so use a keyless ZIP lookup.
    if let Some(point) = resolve_us_zip(q)? {
        return Ok(point);
    }
    let geo = http_get_text(
        &format!(
            "https://geocoding-api.open-meteo.com/v1/search?name={}&count=1&language=en&format=json",
            urlencode(q)
        ),
        64 * 1024,
    )?;
    let geo: Value =
        serde_json::from_str(&geo).map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    let first = geo["results"]
        .get(0)
        .ok_or_else(|| CapabilityError::BadInput(format!("couldn't find a place called '{q}'")))?;
    let lat = first["latitude"].as_f64().unwrap_or_default();
    let lon = first["longitude"].as_f64().unwrap_or_default();
    let name = first["name"].as_str().unwrap_or(q);
    let country = first["country"].as_str().unwrap_or("");
    Ok((
        lat,
        lon,
        format!("{name}, {country}")
            .trim_end_matches(", ")
            .to_owned(),
    ))
}

/// If `q` is a 5-digit US ZIP, resolve it to `(lat, lon, "City, ST")` via the
/// keyless zippopotam.us service. Returns `Ok(None)` when `q` isn't a ZIP so the
/// caller falls through to the place-name geocoder.
fn resolve_us_zip(q: &str) -> Result<Option<(f64, f64, String)>, CapabilityError> {
    let zip = q.trim();
    if zip.len() != 5 || !zip.bytes().all(|b| b.is_ascii_digit()) {
        return Ok(None);
    }
    let body = http_get_text(&format!("https://api.zippopotam.us/us/{zip}"), 16 * 1024)?;
    let data: Value =
        serde_json::from_str(&body).map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
    let place = data["places"]
        .get(0)
        .ok_or_else(|| CapabilityError::BadInput(format!("couldn't find the ZIP code '{zip}'")))?;
    // zippopotam returns latitude/longitude as strings.
    let lat = place["latitude"].as_str().and_then(|s| s.parse().ok());
    let lon = place["longitude"].as_str().and_then(|s| s.parse().ok());
    let (Some(lat), Some(lon)) = (lat, lon) else {
        return Ok(None);
    };
    let city = place["place name"].as_str().unwrap_or("");
    let state = place["state abbreviation"].as_str().unwrap_or("");
    Ok(Some((
        lat,
        lon,
        format!("{city}, {state}")
            .trim_matches([',', ' '])
            .to_owned(),
    )))
}

// ---- Weather (real; Open-Meteo, no API key) --------------------------------

struct WeatherCapability;

impl Capability for WeatherCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "weather",
            name: "Weather",
            description: "Current conditions and today's forecast for a place, with a heads-up on severe weather.",
            category: "information",
            reaches_external: true,
            autonomy: AutonomyLevel::ActWithinPolicy,
            configured: true,
            needs: "",
        }
    }

    fn invoke(&self, input: &Value) -> Result<Value, CapabilityError> {
        // Accept {location:"..."} (a place name or US ZIP) or {lat, lon}.
        let (lat, lon, place) = resolve_point(input)?;

        let body = http_get_text(
            &format!(
                "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
                 &current=temperature_2m,apparent_temperature,weather_code,wind_speed_10m\
                 &daily=temperature_2m_max,temperature_2m_min,weather_code&timezone=auto&forecast_days=1"
            ),
            64 * 1024,
        )?;
        let w: Value =
            serde_json::from_str(&body).map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let code = w["current"]["weather_code"].as_i64().unwrap_or(-1);
        let condition = weather_condition(code);
        let severe = matches!(code, 65 | 75 | 82 | 86 | 95 | 96 | 99);
        Ok(json!({
            "place": place,
            "temperature_c": w["current"]["temperature_2m"],
            "feels_like_c": w["current"]["apparent_temperature"],
            "wind_kmh": w["current"]["wind_speed_10m"],
            "condition": condition,
            "high_c": w["daily"]["temperature_2m_max"][0],
            "low_c": w["daily"]["temperature_2m_min"][0],
            "warning": if severe { format!("Heads-up: {condition} expected — take care.") } else { String::new() },
        }))
    }
}

fn weather_condition(code: i64) -> &'static str {
    match code {
        0 => "clear sky",
        1..=3 => "partly cloudy",
        45 | 48 => "fog",
        51 | 53 | 55 | 56 | 57 => "drizzle",
        61 | 63 | 66 | 67 | 80 | 81 => "rain",
        65 | 82 => "heavy rain",
        71 | 73 | 77 | 85 => "snow",
        75 | 86 => "heavy snow",
        95 => "thunderstorm",
        96 | 99 => "thunderstorm with hail",
        _ => "unknown",
    }
}

fn urlencode(s: &str) -> String {
    s.bytes()
        .map(|b| match b {
            b'a'..=b'z' | b'A'..=b'Z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                (b as char).to_string()
            }
            b' ' => "%20".to_owned(),
            other => format!("%{other:02X}"),
        })
        .collect()
}

// ---- Web fetch / browse (real) ---------------------------------------------

struct WebFetchCapability;

impl Capability for WebFetchCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "web_fetch",
            name: "Web browsing",
            description: "Fetch a web page and read its text — for research and briefings.",
            category: "information",
            reaches_external: true,
            autonomy: AutonomyLevel::ActWithinPolicy,
            configured: true,
            needs: "",
        }
    }

    fn invoke(&self, input: &Value) -> Result<Value, CapabilityError> {
        let url = str_field(input, "url")?;
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(CapabilityError::BadInput("url must be http(s)".to_owned()));
        }
        let html = http_get_text(url, 512 * 1024)?;
        let title = between(&html, "<title>", "</title>").unwrap_or_default();
        let text = strip_html(&html);
        let excerpt: String = text.chars().take(2_000).collect();
        Ok(json!({ "url": url, "title": title.trim(), "text": excerpt }))
    }
}

fn between(s: &str, a: &str, b: &str) -> Option<String> {
    let start = s.find(a)? + a.len();
    let end = s[start..].find(b)? + start;
    Some(s[start..end].to_owned())
}

/// Very small HTML-to-text: drops script/style, strips tags, collapses space.
fn strip_html(html: &str) -> String {
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut skip_to: Option<&str> = None;
    let lower = html.to_lowercase();
    let bytes = html.as_bytes();
    let mut i = 0;
    while i < bytes.len() {
        if let Some(close) = skip_to {
            if lower[i..].starts_with(close) {
                skip_to = None;
                i += close.len();
            } else {
                i += 1;
            }
            continue;
        }
        if lower[i..].starts_with("<script") {
            skip_to = Some("</script>");
            continue;
        }
        if lower[i..].starts_with("<style") {
            skip_to = Some("</style>");
            continue;
        }
        let c = bytes[i] as char;
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
        i += 1;
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---- Image review (local vision model via Ollama; env-gated) ---------------

struct ImageReviewCapability {
    model: Option<String>,
}

impl ImageReviewCapability {
    fn from_env() -> Self {
        Self {
            model: std::env::var("ENDORA_VISION_MODEL")
                .ok()
                .filter(|s| !s.is_empty()),
        }
    }
}

impl Capability for ImageReviewCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "image_review",
            name: "Image review",
            description: "Describe or answer questions about an image, using a local vision model.",
            category: "media",
            reaches_external: false,
            autonomy: AutonomyLevel::ActWithinPolicy,
            configured: self.model.is_some(),
            needs: "set ENDORA_VISION_MODEL to a pulled vision model (e.g. llava, llama3.2-vision)",
        }
    }

    fn invoke(&self, _input: &Value) -> Result<Value, CapabilityError> {
        let Some(_model) = &self.model else {
            return Err(CapabilityError::Unavailable(
                "no vision model configured (set ENDORA_VISION_MODEL and pull e.g. llama3.2-vision)".to_owned(),
            ));
        };
        // Wiring the local vision call is the next step; the module is declared and
        // gated so it appears as an enable-able skill rather than a silent gap.
        Err(CapabilityError::Unavailable(
            "image review is configured but the vision call is not wired yet".to_owned(),
        ))
    }
}

// ---- Declared-but-unconfigured modules (scaffolds) -------------------------

/// A skill that is declared with its full metadata but awaits a data source or
/// key. It appears in the registry as "needs setup" rather than being missing.
macro_rules! scaffold {
    ($ty:ident, $id:literal, $name:literal, $desc:literal, $cat:literal, $external:literal, $auto:expr, $needs:literal) => {
        struct $ty;
        impl Capability for $ty {
            fn info(&self) -> CapabilityInfo {
                CapabilityInfo {
                    id: $id,
                    name: $name,
                    description: $desc,
                    category: $cat,
                    reaches_external: $external,
                    autonomy: $auto,
                    configured: false,
                    needs: $needs,
                }
            }
            fn invoke(&self, _input: &Value) -> Result<Value, CapabilityError> {
                Err(CapabilityError::Unavailable(format!(
                    "{} needs setup: {}",
                    $name, $needs
                )))
            }
        }
    };
}

scaffold!(
    LocalEventsCapability,
    "local_events",
    "Local events",
    "What's on near you — concerts, markets, community happenings.",
    "information",
    true,
    AutonomyLevel::ActWithinPolicy,
    "an events data source / API key"
);
scaffold!(
    FlightSearchCapability,
    "flights",
    "Flight search",
    "Find and compare flights for a trip.",
    "travel",
    true,
    AutonomyLevel::ConfirmEachAction,
    "a flights API key (booking stays a human decision)"
);
scaffold!(
    LocationLogCapability,
    "location",
    "Location tracking",
    "Keep a private log of where you are while travelling, so the butler has context.",
    "presence",
    false,
    AutonomyLevel::ConfirmEachAction,
    "your opt-in and a location source (kept private to you)"
);
/// The "guard dog": active public-safety alerts near a place. Real for the US via
/// the National Weather Service (no key); elsewhere it reports no coverage.
struct SafetyAlertsCapability;

impl Capability for SafetyAlertsCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "safety_alerts",
            name: "Guard dog",
            description: "Active safety alerts near you — severe weather and public warnings (US National Weather Service).",
            category: "safety",
            reaches_external: true,
            autonomy: AutonomyLevel::ActWithinPolicy,
            configured: true,
            needs: "",
        }
    }

    fn invoke(&self, input: &Value) -> Result<Value, CapabilityError> {
        let (lat, lon, place) = resolve_point(input)?;
        let body = http_get_text_ua(
            &format!("https://api.weather.gov/alerts/active?point={lat:.4},{lon:.4}"),
            "Endora personal butler (github.com/RusticPotatos/Endora)",
            256 * 1024,
        )?;
        let data: Value =
            serde_json::from_str(&body).map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let alerts: Vec<Value> = data["features"]
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter_map(|f| {
                        let p = &f["properties"];
                        Some(json!({
                            "event": p["event"].as_str()?,
                            "severity": p["severity"].as_str().unwrap_or("Unknown"),
                            "headline": p["headline"].as_str().unwrap_or(""),
                            "area": p["areaDesc"].as_str().unwrap_or(""),
                        }))
                    })
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({
            "place": place,
            "count": alerts.len(),
            "all_clear": alerts.is_empty(),
            "alerts": alerts,
            "note": if alerts.is_empty() { "No active alerts (or outside US coverage)." } else { "" },
        }))
    }
}

scaffold!(
    IncidentScannerCapability,
    "incident_scanner",
    "Incident scanner",
    "Surface public emergency/incident alerts nearby (fire, rescue, major incidents).",
    "safety",
    true,
    AutonomyLevel::ActWithinPolicy,
    "a public incident/emergency feed for your area"
);

// ---- Application-facing runner ---------------------------------------------

/// Adapts the concrete capability registry to the application's
/// [`CapabilityRunner`] port, so the butler use case can list and run skills
/// without depending on this crate. A capability is "autonomous" (may run on its
/// own this turn) exactly when its autonomy is [`AutonomyLevel::ActWithinPolicy`]
/// — read-only, low-stakes. Anything that must confirm stays gated.
pub struct RegistryRunner {
    capabilities: Arc<Vec<Arc<dyn Capability>>>,
}

impl RegistryRunner {
    /// Wraps a shared capability registry.
    #[must_use]
    pub fn new(capabilities: Arc<Vec<Arc<dyn Capability>>>) -> Self {
        Self { capabilities }
    }
}

impl endora_application::CapabilityRunner for RegistryRunner {
    fn available(&self) -> Vec<endora_application::CapabilitySpec> {
        self.capabilities
            .iter()
            .map(|c| {
                let info = c.info();
                endora_application::CapabilitySpec {
                    id: info.id.to_owned(),
                    description: info.description.to_owned(),
                    configured: info.configured,
                    autonomous: matches!(info.autonomy, AutonomyLevel::ActWithinPolicy),
                }
            })
            .collect()
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        let cap = self
            .capabilities
            .iter()
            .find(|c| c.info().id == id)
            .ok_or_else(|| format!("no such skill '{id}'"))?;
        let input: Value = serde_json::from_str(input_json.trim())
            .or_else(|_| Ok::<Value, serde_json::Error>(json!({})))
            .unwrap_or_else(|_| json!({}));
        let out = cap.invoke(&input).map_err(|e| e.to_string())?;
        Ok(out.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn registry_has_modules_with_stable_ids() {
        let caps = default_capabilities();
        assert!(caps.len() >= 6);
        let ids: Vec<_> = caps.iter().map(|c| c.info().id).collect();
        assert!(ids.contains(&"weather"));
        assert!(ids.contains(&"web_fetch"));
        assert!(ids.contains(&"flights"));
    }

    #[test]
    fn a_scaffold_reports_unavailable_with_a_reason() {
        let err = FlightSearchCapability.invoke(&json!({})).unwrap_err();
        assert!(matches!(err, CapabilityError::Unavailable(_)));
    }

    #[test]
    fn zip_detector_ignores_non_zip_input_without_a_network_call() {
        // Non-5-digit or non-numeric input must fall through (Ok(None)), so a place
        // name never triggers the ZIP lookup.
        assert!(resolve_us_zip("Charlotte").unwrap().is_none());
        assert!(resolve_us_zip("2827").unwrap().is_none());
        assert!(resolve_us_zip("abcde").unwrap().is_none());
        assert!(resolve_us_zip("Boston, MA").unwrap().is_none());
    }

    #[test]
    fn web_fetch_rejects_non_http() {
        let err = WebFetchCapability
            .invoke(&json!({ "url": "file:///etc/passwd" }))
            .unwrap_err();
        assert!(matches!(err, CapabilityError::BadInput(_)));
    }

    #[test]
    fn strip_html_drops_tags_and_scripts() {
        let t = strip_html("<html><script>bad()</script><p>Hello <b>world</b></p></html>");
        assert_eq!(t, "Hello world");
    }

    #[test]
    fn image_review_is_unconfigured_without_a_model() {
        let cap = ImageReviewCapability { model: None };
        assert!(!cap.info().configured);
        assert!(cap.invoke(&json!({})).is_err());
    }
}
