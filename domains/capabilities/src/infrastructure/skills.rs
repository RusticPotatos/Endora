use super::*;

// ---- Weather (real; Open-Meteo, no API key) --------------------------------

pub(super) struct WeatherCapability;

impl Capability for WeatherCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "weather",
            name: "Weather",
            description: "Current conditions and today's forecast for a place, with a heads-up on severe weather.",
            category: "information",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0053.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            // Needs a place — a town, or a postcode, and said it needed nothing.
            wants_place: true,
            borrows_from: None,
            third_party: false,
            input_schema: Some(
                r#"{"type":"object","properties":{"location":{"type":"string","description":"a town or postcode"},"lat":{"type":"number"},"lon":{"type":"number"}},"required":["location"]}"#,
            ),
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        // Accept {location:"..."} (a place name or US ZIP) or {lat, lon}.
        let (lat, lon, place) = resolve_point(input)?;

        let body = http_get_text(
            &format!(
                "https://api.open-meteo.com/v1/forecast?latitude={lat}&longitude={lon}\
                 &current=temperature_2m,apparent_temperature,weather_code,wind_speed_10m\
                 &daily=temperature_2m_max,temperature_2m_min,weather_code,precipitation_probability_max\
                 &timezone=auto&forecast_days=1"
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
            // Today's peak chance of rain — what decides the umbrella line below.
            "rain_chance_pct": w["daily"]["precipitation_probability_max"][0],
            // The local time this reading is FROM — a brief is posted once and never
            // updates, so "current" can go stale by the time it's read. Surfacing the
            // observation time makes that obvious ("as of 7 AM") instead of looking
            // like a wrong right-now temperature.
            "observed_at": w["current"]["time"],
            "warning": if severe { format!("Heads-up: {condition} expected — take care.") } else { String::new() },
        }))
    }

    fn summarize(&self, o: &Value) -> String {
        let place = o["place"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("there");
        let cond = o["condition"].as_str().unwrap_or("");
        // Emit BOTH units so the model relays a grounded number instead of doing
        // its own (error-prone) C↔F conversion — whichever unit the person prefers,
        // the real value is right here.
        let cf = |c: f64| format!("{c:.0}°C / {:.0}°F", c * 9.0 / 5.0 + 32.0);
        let mut s = format!("Weather for {place}: {cond}");
        if let Some(t) = o["temperature_c"].as_f64() {
            s.push_str(&format!(", {}", cf(t)));
        }
        if let Some(f) = o["feels_like_c"].as_f64() {
            s.push_str(&format!(" (feels like {})", cf(f)));
        }
        // Time-stamp the reading so a stale brief reads as "as of this morning",
        // not a wrong current temperature.
        if let Some(at) = o["observed_at"].as_str().and_then(observed_time) {
            s.push_str(&format!(" as of {at}"));
        }
        if let (Some(hi), Some(lo)) = (o["high_c"].as_f64(), o["low_c"].as_f64()) {
            s.push_str(&format!("; today's high {}, low {}", cf(hi), cf(lo)));
        }
        // Whether you need an umbrella or a jacket is a fact of the numbers, so it
        // is said here rather than left to the model. Named heuristics — 40% is
        // where most people start carrying one, 10°C / 50°F is where a jacket stops
        // being optional — and the blast radius of a wrong one is a sentence of
        // advice. Missing fields say nothing: no value, no claim.
        if let Some(rain) = o["rain_chance_pct"].as_f64().filter(|r| *r >= 40.0) {
            s.push_str(&format!(". Take an umbrella — {rain:.0}% chance of rain"));
        }
        let chill = [o["feels_like_c"].as_f64(), o["low_c"].as_f64()]
            .into_iter()
            .flatten()
            .fold(f64::INFINITY, f64::min);
        if chill <= 10.0 {
            s.push_str(&format!(". Bring a jacket — expect {}", cf(chill)));
        }
        if let Some(w) = o["warning"].as_str().filter(|w| !w.is_empty()) {
            s.push_str(&format!(". {w}"));
        }
        s
    }
}

/// Formats an Open-Meteo local ISO timestamp (`2026-07-23T07:00`) as a friendly
/// 12-hour time (`7:00 AM`), for the "as of …" tag on a weather reading. Returns
/// `None` if the shape isn't recognized.
pub(super) fn observed_time(iso: &str) -> Option<String> {
    let hm = iso.split('T').nth(1)?;
    let (h, m) = hm.split_once(':')?;
    let h: u32 = h.parse().ok()?;
    if h > 23 || m.len() != 2 || !m.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    let ampm = if h < 12 { "AM" } else { "PM" };
    let h12 = match h % 12 {
        0 => 12,
        x => x,
    };
    Some(format!("{h12}:{m} {ampm}"))
}

pub(super) fn weather_condition(code: i64) -> &'static str {
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

pub(super) fn urlencode(s: &str) -> String {
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

// ---- Internet search (Brave's web search API) -------------------------------

pub(super) struct WebSearchCapability;

pub(super) fn search_results(raw: &Value, count: usize) -> Result<Value, CapabilityError> {
    let results = raw["web"]["results"].as_array().ok_or_else(|| {
        CapabilityError::Unavailable("Search provider returned no result collection".to_owned())
    })?;
    let results: Vec<Value> = results.iter().filter_map(|item| {
        let url = item["url"].as_str()?;
        let authority = url.strip_prefix("https://")?.split('/').next()?;
        if authority.is_empty() || authority.contains('@') || url.contains(char::is_whitespace) { return None; }
        Some(json!({"title":item["title"].as_str().unwrap_or("").chars().take(300).collect::<String>(),
            "url":url, "snippet":item["description"].as_str().unwrap_or("").chars().take(2000).collect::<String>()}))
    }).take(count).collect();
    Ok(json!({"results":results}))
}

pub(super) fn search_request(url: &str, key: &str) -> Result<Value, CapabilityError> {
    // Never forward this provider's key through redirects. The production URL is fixed.
    let mut builder = ureq::Agent::config_builder()
        .timeout_global(Some(Duration::from_secs(15)))
        .max_redirects(0);
    if let Some(proxy) = egress_proxy() {
        builder = builder.proxy(Some(proxy));
    }
    let agent: ureq::Agent = builder.build().into();
    let mut response = agent
        .get(url)
        .header("X-Subscription-Token", key)
        .header("Api-Version", "2023-01-01")
        .header("Accept", "application/json")
        .call()
        .map_err(|error| {
            CapabilityError::Unavailable(format!("Internet search request failed: {error}"))
        })?;
    response
        .body_mut()
        .with_config()
        .limit(512 * 1024)
        .read_json::<Value>()
        .map_err(|_| CapabilityError::Unavailable("Invalid search provider response".to_owned()))
}

impl Capability for WebSearchCapability {
    fn for_people(&self) -> Option<&'static str> {
        Some("Searches the internet for current sources, so answers can rest on something real.")
    }

    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "web_search",
            name: "Internet search",
            description: "Search the internet for current sources. Returns titles, URLs and snippets; fetch useful URLs with web_fetch before treating them as evidence.",
            category: "information",
            reaches_external: true,
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "Brave Search API key",
            settings: &[SettingSpec {
                key: "api_key",
                label: "Brave Search API key",
                secret: true,
                optional: false,
            }],
            wants_place: false,
            borrows_from: None,
            third_party: true,
            input_schema: Some(
                r#"{"type":"object","properties":{"query":{"type":"string","description":"Search terms for current information"},"count":{"type":"integer","minimum":1,"maximum":10}},"required":["query"]}"#,
            ),
        }
    }

    fn invoke(
        &self,
        input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let key = settings
            .get("api_key")
            .map(String::as_str)
            .filter(|s| !s.trim().is_empty())
            .ok_or_else(|| {
                CapabilityError::Unavailable(
                    "Configure Internet search with a Brave Search API key in Skills".to_owned(),
                )
            })?;
        let query = str_field(input, "query")?;
        if query.chars().count() > 600 || query.split_whitespace().count() > 75 {
            return Err(CapabilityError::BadInput(
                "Search query is too long".to_owned(),
            ));
        }
        let count = match input.get("count") {
            None => 5,
            Some(value) => value
                .as_u64()
                .filter(|n| (1..=10).contains(n))
                .ok_or_else(|| {
                    CapabilityError::BadInput("Search count must be 1 through 10".to_owned())
                })? as usize,
        };
        let url = format!(
            "https://api.search.brave.com/res/v1/web/search?q={}&count={count}",
            urlencode(query)
        );
        search_results(&search_request(&url, key)?, count)
    }

    fn summarize(&self, output: &Value) -> String {
        let mut text = "Search leads (fetch pages to verify their contents):\n".to_owned();
        if let Some(results) = output["results"].as_array() {
            if results.is_empty() {
                text.push_str("No matching sources found.");
            }
            for item in results {
                text.push_str(&format!(
                    "{}\n{}\n{}\n",
                    item["title"].as_str().unwrap_or(""),
                    item["url"].as_str().unwrap_or(""),
                    item["snippet"].as_str().unwrap_or("")
                ));
            }
        }
        text
    }
}

// ---- Web fetch / browse (real) ---------------------------------------------

pub(super) struct WebFetchCapability;

impl Capability for WebFetchCapability {
    fn for_people(&self) -> Option<&'static str> {
        Some("Reads a web page you or a search pointed it to.")
    }

    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "web_fetch",
            name: "Web browsing",
            description: "Read ONE web page whose address you already have. It cannot search \
                          — if you do not have a real address, search first and read a result.",
            category: "information",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0053.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            // It requires a URL and said it took nothing, so the model supplied the one
            // address every documentation page uses — fetched `example.com`, got back "this
            // domain is for use in documentation examples", and answered from it. A skill
            // that fails is recoverable; one that succeeds against a placeholder is not.
            wants_place: false,
            borrows_from: None,
            third_party: true,
            input_schema: Some(
                r#"{"type":"object","properties":{
                     "url":{"type":"string","description":"the full https:// address of a real page you already have"}},
                   "required":["url"]}"#,
            ),
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let url = str_field(input, "url")?;
        if !(url.starts_with("http://") || url.starts_with("https://")) {
            return Err(CapabilityError::BadInput("url must be http(s)".to_owned()));
        }
        // Guard against SSRF: a model-provided URL must not reach internal hosts.
        let html = guarded_get_text(url, 512 * 1024)?;
        let title = between(&html, "<title>", "</title>").unwrap_or_default();
        let text = strip_html(&html);
        // Ordinary turns retain their compact window. Commissioned research may ask
        // for the larger bounded window through the same guarded reader.
        let max_chars = input
            .get("max_chars")
            .and_then(Value::as_u64)
            .unwrap_or(2_000)
            .min(12_000) as usize;
        let excerpt: String = text.chars().take(max_chars).collect();
        let mut output = json!({ "url": url, "title": title.trim(), "text": excerpt });
        match crate::application::schedule::json_ld_events(&html) {
            Ok(events) => output["sports_events"] = json!(events),
            Err(error) => output["structured_error"] = json!(error),
        }
        Ok(output)
    }
}

pub(super) fn between(s: &str, a: &str, b: &str) -> Option<String> {
    let start = s.find(a)? + a.len();
    let end = s[start..].find(b)? + start;
    Some(s[start..end].to_owned())
}

/// Very small HTML-to-text: drops script/style, strips tags, collapses space.
///
/// Walks the text a character at a time and compares tag names against the original bytes,
/// ignoring ASCII case. It used to step a byte at a time and slice a lowercased copy at each
/// step, which panicked on the first character that is not ASCII (a live fetch of a page with
/// an arrow in its navigation) and, had it not, would have read every byte of such a
/// character as a character of its own.
pub(super) fn strip_html(html: &str) -> String {
    let bytes = html.as_bytes();
    let starts_with_at = |i: usize, pattern: &str| {
        bytes
            .get(i..i + pattern.len())
            .is_some_and(|window| window.eq_ignore_ascii_case(pattern.as_bytes()))
    };
    let mut out = String::with_capacity(html.len() / 2);
    let mut in_tag = false;
    let mut skip_to: Option<&str> = None;
    let mut chars = html.char_indices();
    while let Some((i, c)) = chars.next() {
        if let Some(close) = skip_to {
            if starts_with_at(i, close) {
                skip_to = None;
                // The closing tag is ASCII, one byte a character, and its first character
                // is the one just read.
                for _ in 1..close.len() {
                    chars.next();
                }
            }
            continue;
        }
        if starts_with_at(i, "<script") {
            skip_to = Some("</script>");
            continue;
        }
        if starts_with_at(i, "<style") {
            skip_to = Some("</style>");
            continue;
        }
        match c {
            '<' => in_tag = true,
            '>' => {
                in_tag = false;
                out.push(' ');
            }
            _ if !in_tag => out.push(c),
            _ => {}
        }
    }
    out.split_whitespace().collect::<Vec<_>>().join(" ")
}

// ---- Local news (real; Google News RSS, no API key) ------------------------

/// How many headlines a news call wants. Six was hardcoded; a brief wants the top
/// three, and "top three" enforced here is real, where "please only mention three"
/// in a prompt is a wish. Clamped: zero headlines is not a news reading, and more
/// than six is a feed, not a briefing.
pub(super) fn wanted_headlines(input: &Value) -> usize {
    input
        .get("count")
        .and_then(Value::as_u64)
        .map_or(6, |n| n.clamp(1, 6)) as usize
}

pub(super) struct LocalNewsCapability;

impl Capability for LocalNewsCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "news",
            name: "Local news",
            description: "Recent news headlines for a place or topic — so answers about the news are real, not guessed.",
            category: "information",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0053.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            // Needs where or what, and said it needed nothing.
            wants_place: true,
            borrows_from: None,
            third_party: true,
            input_schema: Some(
                r#"{"type":"object","properties":{"query":{"type":"string","description":"a topic to search for"},"location":{"type":"string","description":"a town, when asking what is happening there"},"count":{"type":"integer","description":"how many headlines (1-6, default 6)"}},"required":[]}"#,
            ),
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        // Prefer an explicit {query}; else build one from {location}. A bare ZIP or
        // raw coordinates make a poor news search, so resolve the location to a
        // place name first ("10001" → "New York, NY news"). One of query/location
        // is required — without it we say so rather than invent headlines.
        let query = match input.get("query").and_then(Value::as_str) {
            Some(q) if !q.trim().is_empty() => q.trim().to_owned(),
            _ => {
                let raw = str_field(input, "location").map_err(|_| {
                    CapabilityError::BadInput("missing 'location' or 'query'".to_owned())
                })?;
                let place = match resolve_point(input) {
                    Ok((_, _, p)) if !p.is_empty() => p,
                    _ => raw.to_owned(),
                };
                format!("{place} news")
            }
        };
        let url = format!(
            "https://news.google.com/rss/search?q={}&hl=en-US&gl=US&ceid=US:en",
            urlencode(&query)
        );
        let xml = http_get_text(&url, 256 * 1024)?;
        let items = extract_rss_items(&xml, wanted_headlines(input));
        let headlines: Vec<Value> = items
            .iter()
            .map(|(title, link, publisher)| {
                json!({ "title": title, "url": link, "publisher": publisher })
            })
            .collect();
        Ok(json!({
            "query": query,
            "count": headlines.len(),
            "headlines": headlines,
            "note": if headlines.is_empty() {
                "No recent headlines found for that search."
            } else {
                ""
            },
        }))
    }

    fn summarize(&self, o: &Value) -> String {
        let query = o["query"].as_str().unwrap_or("that");
        // A headline is either a plain string (legacy) or `{title, url}`. Include
        // the source URL when present so the butler can cite it, not guess it.
        let render = |i: usize, h: &Value| -> Option<String> {
            if let Some(t) = h.as_str() {
                return Some(format!("{}. {t}", i + 1));
            }
            let title = h["title"].as_str().filter(|s| !s.is_empty())?;
            let publisher = h["publisher"].as_str().filter(|s| !s.is_empty());
            let url = h["url"].as_str().filter(|s| !s.is_empty());
            // Cite the outlet + the link when we have them, so the butler relays a
            // real source rather than guessing one.
            let src = match (publisher, url) {
                (Some(p), Some(u)) => format!(" — {p} ({u})"),
                (Some(p), None) => format!(" — {p}"),
                (None, Some(u)) => format!(" — {u}"),
                (None, None) => String::new(),
            };
            Some(format!("{}. {title}{src}", i + 1))
        };
        let list: Vec<String> = o["headlines"]
            .as_array()
            .map(|a| {
                a.iter()
                    .enumerate()
                    .filter_map(|(i, h)| render(i, h))
                    .collect()
            })
            .unwrap_or_default();
        if list.is_empty() {
            return format!("No recent news headlines were found for {query}.");
        }
        format!("Recent news headlines for {query}:\n{}", list.join("\n"))
    }
}

/// Extracts up to `max` `<item>` headlines from an RSS feed as `(title, link,
/// publisher)`, decoding the common XML entities. The link is the article's
/// **source** and `publisher` the outlet name (Google News RSS carries a
/// `<source url="…">Publisher</source>`) — both kept so the butler can *cite* a
/// real source, not guess one. Small and tolerant.
pub(super) fn extract_rss_items(xml: &str, max: usize) -> Vec<(String, String, String)> {
    let mut out = Vec::new();
    let mut rest = xml;
    while let Some(start) = rest.find("<item>") {
        rest = &rest[start + "<item>".len()..];
        let item = match rest.find("</item>") {
            Some(end) => &rest[..end],
            None => rest,
        };
        let title = between(item, "<title>", "</title>")
            .map(|t| decode_xml_entities(strip_cdata(&t).trim()))
            .unwrap_or_default();
        let link = between(item, "<link>", "</link>")
            .map(|l| decode_xml_entities(strip_cdata(&l).trim()))
            .unwrap_or_default();
        // <source url="…">Publisher</source> — take the text after the opening
        // tag's ">" (skips the url= attribute) up to the closing tag.
        let publisher = between(item, "<source", "</source>")
            .and_then(|s| s.split_once('>').map(|(_, name)| name.to_owned()))
            .map(|n| decode_xml_entities(strip_cdata(n.trim()).trim()))
            .unwrap_or_default();
        if !title.is_empty() {
            out.push((title, link, publisher));
        }
        if out.len() >= max {
            break;
        }
    }
    out
}

/// Unwraps a `<![CDATA[…]]>` wrapper if present, returning the inner text.
pub(super) fn strip_cdata(s: &str) -> String {
    let t = s.trim();
    t.strip_prefix("<![CDATA[")
        .and_then(|r| r.strip_suffix("]]>"))
        .unwrap_or(t)
        .to_owned()
}

/// Decodes the handful of XML entities that show up in RSS titles.
pub(super) fn decode_xml_entities(s: &str) -> String {
    s.replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'")
        .replace("&apos;", "'")
}

// ---- Knowledge lookup (real; Wikipedia, no API key) ------------------------

pub(super) struct KnowledgeCapability;

impl Capability for KnowledgeCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "knowledge",
            name: "Knowledge lookup",
            description: "Look up factual, encyclopedic knowledge about a topic, person, or place (Wikipedia).",
            category: "information",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0053.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            // Needs what to look up, and said it needed nothing.
            wants_place: false,
            borrows_from: None,
            third_party: true,
            input_schema: Some(
                r#"{"type":"object","properties":{"query":{"type":"string","description":"the topic, person or place to look up"}},"required":["query"]}"#,
            ),
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let q = str_field(input, "query").or_else(|_| str_field(input, "topic"))?;
        // Find the best-matching article title, then fetch its summary.
        let search = http_get_text_ua(
            &format!(
                "https://en.wikipedia.org/w/api.php?action=query&list=search&srsearch={}&srlimit=1&format=json",
                urlencode(q)
            ),
            WIKI_UA,
            128 * 1024,
        )?;
        let search: Value = serde_json::from_str(&search)
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let Some(title) = search["query"]["search"]
            .get(0)
            .and_then(|s| s["title"].as_str())
        else {
            return Ok(json!({ "query": q, "found": false, "extract": "" }));
        };
        let summary = http_get_text_ua(
            &format!(
                "https://en.wikipedia.org/api/rest_v1/page/summary/{}",
                urlencode(title)
            ),
            WIKI_UA,
            128 * 1024,
        )?;
        let summary: Value = serde_json::from_str(&summary)
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        Ok(json!({
            "query": q,
            "found": true,
            "title": summary["title"].as_str().unwrap_or(title),
            "extract": summary["extract"].as_str().unwrap_or(""),
            "url": summary["content_urls"]["desktop"]["page"].as_str().unwrap_or(""),
        }))
    }

    fn summarize(&self, o: &Value) -> String {
        let extract = o["extract"].as_str().unwrap_or("");
        if extract.is_empty() {
            return format!(
                "I couldn't find an encyclopedia entry for '{}'.",
                o["query"].as_str().unwrap_or("that")
            );
        }
        let title = o["title"].as_str().unwrap_or("");
        format!("{title}: {extract}")
    }
}

pub(super) const WIKI_UA: &str = "Endora personal butler (github.com/RusticPotatos/Endora)";
// ---- Image review (local vision model via Ollama; env-gated) ---------------

pub(super) struct ImageReviewCapability {
    /// Base URL of the local Ollama (native API), e.g. `http://host:11434`.
    pub(super) ollama_base: String,
}

impl ImageReviewCapability {
    pub(super) fn from_env() -> Self {
        // Reuse the butler's model endpoint; the native /api lives at the base, so
        // strip the OpenAI-compat `/v1` suffix.
        let base = std::env::var("ENDORA_MODEL_URL")
            .unwrap_or_else(|_| "http://localhost:11434/v1".to_owned());
        let base = base
            .trim_end_matches('/')
            .trim_end_matches("/v1")
            .trim_end_matches('/')
            .to_owned();
        Self { ollama_base: base }
    }
}

pub(super) const IMAGE_MODEL_SETTING: &[SettingSpec] = &[SettingSpec {
    key: "model",
    label: "Vision model (a pulled Ollama model, e.g. moondream or llava)",
    secret: false,
    optional: false,
}];

impl Capability for ImageReviewCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "image_review",
            name: "Image review",
            description: "Describe or answer questions about an image, using a local vision model.",
            category: "media",
            reaches_external: false,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0053.
            reversibility: Reversibility::Observe,
            // Code is ready; it becomes usable once the `model` setting is filled.
            configured: true,
            needs: "set the vision model (e.g. moondream) in this skill's settings",
            settings: IMAGE_MODEL_SETTING,
            wants_place: false,
            borrows_from: None,
            third_party: false,
            input_schema: None,
        }
    }

    fn invoke(
        &self,
        input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let model = settings
            .get("model")
            .map(String::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CapabilityError::Unavailable(
                    "no vision model set — configure 'model' (e.g. moondream) in this skill's settings".to_owned(),
                )
            })?;
        let question = input
            .get("question")
            .and_then(Value::as_str)
            .filter(|s| !s.is_empty())
            .unwrap_or("Describe this image in detail.");
        // Accept a URL to fetch, or already-encoded base64.
        let image_b64 = if let Some(url) = input.get("image_url").and_then(Value::as_str) {
            // Guard against SSRF on a model/person-provided image URL.
            base64_encode(&guarded_get_bytes(url, 8 * 1024 * 1024)?)
        } else if let Some(b64) = input.get("image_base64").and_then(Value::as_str) {
            b64.to_owned()
        } else {
            return Err(CapabilityError::BadInput(
                "provide 'image_url' or 'image_base64'".to_owned(),
            ));
        };
        let body = http_post_json(
            &format!("{}/api/generate", self.ollama_base),
            &json!({ "model": model, "prompt": question, "images": [image_b64], "stream": false }),
            256 * 1024,
        )?;
        let v: Value =
            serde_json::from_str(&body).map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        Ok(json!({ "description": v["response"].as_str().unwrap_or("").trim() }))
    }

    fn summarize(&self, o: &Value) -> String {
        let d = o["description"].as_str().unwrap_or("").trim();
        if d.is_empty() {
            "I couldn't make out anything from that image.".to_owned()
        } else {
            d.to_owned()
        }
    }
}

// ---- Declared-but-unconfigured modules (scaffolds) -------------------------

/// A skill that is declared with its full metadata but awaits a data source or
/// key. It appears in the registry as "needs setup" rather than being missing.
macro_rules! scaffold {
    ($ty:ident, $id:literal, $name:literal, $desc:literal, $cat:literal, $external:literal, $reversibility:expr, $needs:literal) => {
        pub(super) struct $ty;
        impl Capability for $ty {
            fn info(&self) -> CapabilityInfo {
                CapabilityInfo {
                    id: $id,
                    name: $name,
                    description: $desc,
                    category: $cat,
                    reaches_external: $external,
                    reversibility: $reversibility,
                    configured: false,
                    needs: $needs,
                    settings: &[],
                    wants_place: false,
                    borrows_from: None,
                    third_party: false,
                    input_schema: None,
                }
            }
            fn invoke(
                &self,
                _input: &Value,
                _settings: &CapabilitySettings,
            ) -> Result<Value, CapabilityError> {
                Err(CapabilityError::Unavailable(format!(
                    "{} needs setup: {}",
                    $name, $needs
                )))
            }
        }
    };
}

/// Today, as `YYYY-MM-DD`.
///
/// The clock does not reach this layer as a port the way it does in the domain — a capability
/// is an adapter to somebody else's service and is allowed to know what day it is. Kept in one
/// place so the format is not written twice.
pub(super) fn today_utc() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or_default();
    let days = (secs / 86_400) as i64;
    civil_from_days(days)
}

/// `YYYY-MM-DD` from days since the epoch — the standard civil-from-days algorithm, so a
/// date is arithmetic rather than a dependency for one line of formatting.
pub(crate) fn civil_from_days(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1_460 + doe / 36_524 - doe / 146_096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = if m <= 2 { y + 1 } else { y };
    format!("{y:04}-{m:02}-{d:02}")
}

/// A failure with the credential taken back out of it.
///
/// This API takes its key in the **query string** — its choice, not ours — so the key is
/// part of the URL of every request. Whether a failure repeats that URL is then decided by
/// the `Display` of an HTTP crate, and a message like *"401 for
/// https://…?apikey=REAL_KEY&…"* would land in an error the person reads and in the record
/// of what was tried.
///
/// So it is removed rather than hoped about. The dependency may print whatever it likes;
/// the key cannot survive this function, and what it says about *why* it failed — a 401 is
/// the most useful thing here — survives intact.
#[must_use]
pub fn without_the_key(said: &str, key: &str) -> String {
    if key.trim().is_empty() {
        return said.to_owned();
    }
    said.replace(key.trim(), "…")
}

/// One ticketed event, as a person would hear it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TicketedEvent {
    /// What it is called.
    pub what: String,
    /// `YYYY-MM-DD`.
    pub on: String,
    /// Local start time, `HH:MM`, empty when the listing has not said.
    pub at: String,
    /// Where, plus its town when the listing gives one.
    pub place: String,
    /// The cheapest advertised ticket, already rounded, empty when none is published.
    pub from: String,
}

/// The events in a Discovery API answer.
///
/// Parsed, never trusted. Field names are the whole of the contract with somebody else's
/// service, and a renamed one has to yield **no events** rather than an error or a
/// half-built row — the same posture as reading a city's agenda, and for the same reason: a
/// screen quietly saying "nothing on" is the worst failure available to a thing whose only
/// job is to say what is on.
#[must_use]
pub fn ticketed_events_in(body: &str) -> Vec<TicketedEvent> {
    let Ok(parsed) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let Some(Value::Array(rows)) = parsed.get("_embedded").and_then(|e| e.get("events")) else {
        // No `_embedded` at all is what an empty search returns, and it is not an error.
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            // Something nobody can name is not worth reporting; the rest is optional.
            let what = row.get("name")?.as_str()?.trim().to_owned();
            if what.is_empty() {
                return None;
            }
            let start = row.get("dates").and_then(|d| d.get("start"));
            let venue = row
                .get("_embedded")
                .and_then(|e| e.get("venues"))
                .and_then(Value::as_array)
                .and_then(|v| v.first());
            let place = match (
                venue.and_then(|v| v.get("name")).and_then(Value::as_str),
                venue
                    .and_then(|v| v.get("city"))
                    .and_then(|c| c.get("name"))
                    .and_then(Value::as_str),
            ) {
                (Some(name), Some(town)) if !name.is_empty() && !town.is_empty() => {
                    format!("{name}, {town}")
                }
                (Some(name), _) => name.to_owned(),
                (None, Some(town)) => town.to_owned(),
                _ => String::new(),
            };
            Some(TicketedEvent {
                what,
                place,
                on: start
                    .and_then(|s| s.get("localDate"))
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .to_owned(),
                // `20:00:00` is a machine's way of saying eight o'clock; the seconds are noise.
                at: start
                    .and_then(|s| s.get("localTime"))
                    .and_then(Value::as_str)
                    .map(|t| t.split(':').take(2).collect::<Vec<_>>().join(":"))
                    .unwrap_or_default(),
                from: row
                    .get("priceRanges")
                    .and_then(Value::as_array)
                    .and_then(|p| p.first())
                    .and_then(|p| p.get("min"))
                    .and_then(Value::as_f64)
                    .map(|m| format!("{}", m.round() as i64))
                    .unwrap_or_default(),
            })
        })
        .collect()
}

/// Says a list the way a person would, not the way a printer would.
///
/// Asked for significant events, the butler recited six council committees verbatim — a
/// summariser that joins every row with "; " hands a small model a list, and a small model
/// handed a list reads the list. Proportion is the fix and it is universal: few enough to
/// say, say them; more than that, say the shape and the first few, and count the rest.
#[must_use]
pub fn said_proportionately(said: Vec<String>, what: &str) -> String {
    /// Up to this many, naming them all reads fine.
    const FEW_ENOUGH_TO_SAY: usize = 4;
    if said.is_empty() {
        return format!("Nothing {what} just now.");
    }
    if said.len() <= FEW_ENOUGH_TO_SAY {
        return said.join("; ");
    }
    let rest = said.len() - FEW_ENOUGH_TO_SAY;
    format!(
        "{} of them — {}; and {rest} more",
        said.len(),
        said[..FEW_ENOUGH_TO_SAY].join("; ")
    )
}

/// Says an event the way somebody would mention it.
#[must_use]
pub fn describe_ticketed_event(e: &TicketedEvent) -> String {
    let mut said = e.what.clone();
    if !e.on.is_empty() {
        said.push_str(&format!(" on {}", e.on));
    }
    if !e.at.is_empty() {
        said.push_str(&format!(" at {}", e.at));
    }
    if !e.place.is_empty() {
        said.push_str(&format!(", {}", e.place));
    }
    if !e.from.is_empty() {
        said.push_str(&format!(", from ${}", e.from));
    }
    said
}

/// One public meeting, as a person would hear it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicMeeting {
    /// `2026-08-03`.
    pub on: String,
    /// `9:00 AM`, in the body's own words. Empty when it does not say.
    pub at: String,
    /// Which committee or council.
    pub who: String,
    /// Where it sits. Empty when it does not say.
    pub place: String,
}

/// Reads Legistar's answer into meetings.
///
/// Legistar runs the legislative calendar for a great many US municipalities and its Web API
/// is **open and keyless** — which is why the civic half of "what's on this week" needs no
/// account, unlike everything else in this area.
///
/// It is also why this is parsed rather than trusted: field names are the only contract, a
/// renamed one would yield empty meetings rather than an error, and a screen quietly saying
/// "nothing on" is the worst possible failure for a thing whose whole job is to say what is on.
#[must_use]
pub fn meetings_in(body: &str) -> Vec<PublicMeeting> {
    let Ok(Value::Array(rows)) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    rows.iter()
        .filter_map(|row| {
            // A meeting nobody can name is not worth reporting; everything else is optional.
            let who = row.get("EventBodyName")?.as_str()?.trim().to_owned();
            if who.is_empty() {
                return None;
            }
            let on = row
                .get("EventDate")
                .and_then(Value::as_str)
                // Legistar dates arrive as `2026-08-03T00:00:00`; the day is the useful part
                // and the zeroed time is noise that reads as a real
                .map(|d| d.split('T').next().unwrap_or(d).to_owned())
                .unwrap_or_default();
            Some(PublicMeeting {
                on,
                at: row
                    .get("EventTime")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
                who,
                place: row
                    .get("EventLocation")
                    .and_then(Value::as_str)
                    .unwrap_or_default()
                    .trim()
                    .to_owned(),
            })
        })
        .collect()
}

/// Says a meeting the way somebody would mention it.
#[must_use]
pub fn describe_meeting(m: &PublicMeeting) -> String {
    let when = match (m.on.is_empty(), m.at.is_empty()) {
        (true, _) => String::new(),
        (false, true) => format!(" on {}", m.on),
        (false, false) => format!(" on {} at {}", m.on, m.at),
    };
    let where_ = if m.place.is_empty() {
        String::new()
    } else {
        format!(", {}", m.place)
    };
    format!("{}{when}{where_}", m.who)
}

/// What the city is doing this week — public meetings from Legistar.
///
/// Named for what it can actually answer rather than for the whole of "local events". The
/// stub this replaces promised "concerts, markets, community happenings" and delivered
/// nothing; ticketed events are a separate source with a separate key, and pretending one
/// skill covers both is how a person ends up asking a question it was never going to answer.
/// What is on at a venue — concerts, sport, shows — from Ticketmaster's Discovery API.
///
/// **ADR 0058 says MCP by default, and this is native anyway.** The rule is *"answers, or a
/// relationship?"*, and this is plainly answers — so a table row should have beaten a
/// thousand lines of Rust. What decides it is the state of the shelf: the only Ticketmaster
/// server published is a v0.1.0 **remote gateway** from a publisher nobody knows, with no
/// local package. Using it routes either the credential or every query — which city, which
/// venue, what somebody is interested in — through a stranger, to reach a public REST API
/// that answers a single authenticated GET.
///
/// That inverts 0058's cost calculus rather than escaping it: native is Rust owned forever,
/// and a row in a table is cheap *only when the row is trustworthy*.
///
/// **So this is deliberately revisitable.** If Ticketmaster publishes a first-party server,
/// or anyone ships one that runs locally over stdio, delete this and add a catalogue entry.
/// Native has to keep earning it.
///
/// Named for what it holds. The skill next door was called `local_events` while answering
/// only with civic agendas, and the butler reached for it for a stadium question twice
/// because on a small model the id is the headline.
pub(super) struct TicketedEventsCapability;

impl Capability for TicketedEventsCapability {
    fn for_people(&self) -> Option<&'static str> {
        Some("Concerts, sport, shows and theatre near you, with dates and the cheapest ticket.")
    }

    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "ticketed_events",
            name: "What's on at a venue",
            description: "Concerts, sport, shows and theatre — with dates, venue and the \
                          cheapest ticket. Ticketed events ONLY: it knows nothing about \
                          council meetings, community listings, or anything without a \
                          ticket. Give it a venue, a team, an act or just a town.",
            category: "information",
            reaches_external: true,
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[SettingSpec {
                key: "ticketmaster_key",
                label: "your Ticketmaster consumer key",
                // It travels in a query string, because that is the only way this API takes
                // it. A URL reaches logs far more readily than a header does, so it is
                // stored as a secret and never echoed back — see `self_test`, which reports
                // what came back and never what was sent.
                secret: true,
                optional: false,
            }],
            // Named fields, because the model is otherwise guessing. It was offered this
            // skill with an empty parameter object, called it with `{}` twice, and was told
            // "say what to look for" twice — doing exactly what it had been told it could.
            wants_place: false,
            borrows_from: None,
            third_party: true,
            input_schema: Some(
                r#"{"type":"object","properties":{
                     "what":{"type":"string","description":"a venue, a team, an act, or a show"},
                     "city":{"type":"string","description":"the town to look in"}},
                   "required":[]}"#,
            ),
        }
    }

    fn invoke(
        &self,
        input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let key = settings
            .get("ticketmaster_key")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CapabilityError::Unavailable(
                    "I need a Ticketmaster consumer key before I can see what's on —                      developer.ticketmaster.com, My Apps."
                        .to_owned(),
                )
            })?;
        // What to look for: a venue, a team, an act. A town narrows it when given.
        let asked = input
            .get("what")
            .or_else(|| input.get("keyword"))
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        let town = input
            .get("city")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .trim();
        if asked.is_empty() && town.is_empty() {
            return Err(CapabilityError::BadInput(
                "say what to look for — a venue, a team, an act, or a town".to_owned(),
            ));
        }
        // Only what has not happened yet. A listing service will happily answer "what is on"
        // with last spring, which is worse than answering nothing.
        let from = format!("{}T00:00:00Z", crate::infrastructure::today_utc());
        let mut url = format!(
            "https://app.ticketmaster.com/discovery/v2/events.json\
             ?apikey={key}&startDateTime={from}&size=20&sort=date,asc"
        );
        if !asked.is_empty() {
            url.push_str(&format!("&keyword={}", urlencode(asked)));
        }
        if !town.is_empty() {
            let (city, state) = city_and_state(town);
            url.push_str(&format!("&city={}", urlencode(&city)));
            if let Some(state) = state {
                url.push_str(&format!("&stateCode={state}"));
            }
        }
        let body = http_get_text_ua(
            &url,
            "Endora personal butler (github.com/RusticPotatos/Endora)",
            512 * 1024,
        )
        .map_err(|e| match e {
            CapabilityError::Unavailable(m) => {
                CapabilityError::Unavailable(without_the_key(&m, key))
            }
            CapabilityError::BadInput(m) => CapabilityError::BadInput(without_the_key(&m, key)),
        })?;
        let on = ticketed_events_in(&body);
        Ok(serde_json::json!({
            "events": on
                .iter()
                .map(|e| serde_json::json!({
                    "what": e.what, "on": e.on, "at": e.at,
                    "place": e.place, "from": e.from,
                }))
                .collect::<Vec<_>>(),
        }))
    }

    fn summarize(&self, output: &Value) -> String {
        let on: Vec<TicketedEvent> = output
            .get("events")
            .and_then(Value::as_array)
            .map(|all| {
                all.iter()
                    .map(|e| TicketedEvent {
                        what: e["what"].as_str().unwrap_or_default().to_owned(),
                        on: e["on"].as_str().unwrap_or_default().to_owned(),
                        at: e["at"].as_str().unwrap_or_default().to_owned(),
                        place: e["place"].as_str().unwrap_or_default().to_owned(),
                        from: e["from"].as_str().unwrap_or_default().to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        if on.is_empty() {
            return "Nothing ticketed coming up for that.".to_owned();
        }
        said_proportionately(
            on.iter().map(describe_ticketed_event).collect(),
            "ticketed coming up",
        )
    }

    /// Proves the key works, without ever repeating it.
    ///
    /// A wrong key here is a 401 from Ticketmaster, which is the single most useful thing
    /// this can report — the same reason an MCP server got a Test button.
    fn self_test(&self, settings: &CapabilitySettings) -> Result<String, CapabilityError> {
        let found = self.invoke(&serde_json::json!({ "city": "New York" }), settings)?;
        let n = found
            .get("events")
            .and_then(Value::as_array)
            .map_or(0, Vec::len);
        Ok(format!("The key works — {n} listings came back."))
    }
}

/// How the drive is — travel times through the house (ADR 0064/0058, the mail
/// skill's pattern).
///
/// Endora holds no maps credential. A travel-time integration (Waze Travel Time is
/// the common free one) publishes sensors into Home Assistant, and this reads them
/// over the connection that already exists. Sensors are discovered by shape
/// (`commute_in`); a house with none gets an honest absence naming what to add,
/// never an invented drive.
pub(super) struct TrafficCapability;

impl Capability for TrafficCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "traffic",
            name: "Traffic",
            description: "How the drive is right now — travel times from the house's own \
                          commute sensors (e.g. Waze Travel Time in Home Assistant).",
            category: "travel",
            reaches_external: false,
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            wants_place: false,
            // Rides the house's connection rather than asking for a token twice.
            borrows_from: Some("home_assistant"),
            third_party: false,
            input_schema: None,
        }
    }

    fn invoke(
        &self,
        _input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let home =
            crate::home_assistant::HomeAssistant::from_settings(settings).ok_or_else(|| {
                CapabilityError::Unavailable(
                    "the drive is read through Home Assistant — set its URL and token in \
                     the Home Assistant skill"
                        .to_owned(),
                )
            })?;
        let entities = home.entities().map_err(CapabilityError::Unavailable)?;
        let said = crate::home_assistant::commute_in(&entities);
        // No sensors is unavailability, not an empty reading: a chat ask gets the
        // honest hint below, and a brief skips the section instead of printing the
        // same hint every morning.
        if said.is_empty() {
            return Err(CapabilityError::Unavailable(
                "no travel-time sensors in the house yet — add one (for example the \
                 Waze Travel Time integration in Home Assistant) and this will report \
                 the drive"
                    .to_owned(),
            ));
        }
        Ok(serde_json::json!({
            "drives": said
                .iter()
                .map(|d| serde_json::json!({
                    "name": d.name,
                    "minutes": d.minutes,
                    "route": d.route,
                }))
                .collect::<Vec<_>>(),
        }))
    }

    fn summarize(&self, output: &Value) -> String {
        let lines: Vec<String> = output
            .get("drives")
            .and_then(Value::as_array)
            .map(|all| {
                all.iter()
                    .filter_map(|d| {
                        let name = d["name"].as_str().filter(|n| !n.is_empty())?;
                        let minutes = d["minutes"].as_f64()?;
                        let route = d["route"].as_str().filter(|r| !r.is_empty());
                        Some(match route {
                            Some(r) => format!("{name} — {minutes:.0} min via {r}"),
                            None => format!("{name} — {minutes:.0} min"),
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if lines.is_empty() {
            return "No travel-time sensors in the house yet — add one (for example the \
                    Waze Travel Time integration in Home Assistant) and this will report \
                    the drive."
                .to_owned();
        }
        format!("Traffic: {}", lines.join(" · "))
    }
}

/// Splits "New York NY" into the city and the state code Ticketmaster wants as a
/// **separate** parameter. The place preference is exactly that shape, and sent
/// whole as `city=` it matches no city at all — live, the home team played at the
/// stadium while the brief said nothing ticketed was coming up. A named heuristic:
/// a trailing two-letter word is read as a state code only when something precedes
/// it, and a trailing comma on the city is shed with it.
pub(super) fn city_and_state(town: &str) -> (String, Option<String>) {
    let mut words: Vec<&str> = town.split_whitespace().collect();
    let state = match words.last() {
        Some(last)
            if words.len() >= 2
                && last.len() == 2
                && last.chars().all(|c| c.is_ascii_alphabetic()) =>
        {
            let code = last.to_uppercase();
            words.pop();
            Some(code)
        }
        _ => None,
    };
    (words.join(" ").trim_end_matches(',').to_owned(), state)
}

/// What the mail says — headers only, through the house (ADR 0064/0058).
///
/// Endora holds no mail credential. The app password lives in Home Assistant's IMAP
/// integration, and this reads the entity that integration publishes, over the connection
/// that already exists. One credential, one place — and nothing here can send.
///
/// **Third-party by construction.** Mail is other people's words, so any turn that reads it
/// stops being able to act (ADR 0064): an email that says "turn off the alarm" reaches a
/// butler that may answer and may not obey. That rule existed before this skill did, which
/// is the only reason this skill may exist.
///
/// Headers, never bodies. *"Anything I need to deal with?"* is answered by who wrote and
/// what about; a body is where one-time codes and reset links live, and this repository has
/// no business carrying those into a context that persists.
pub(super) struct MailCapability;

impl Capability for MailCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "mail",
            name: "What the mail says",
            description: "Who has written and what about, from the mailbox connected to the \
                          house. Headers only — never message bodies — and it cannot send.",
            category: "information",
            reaches_external: false,
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            // Rides the house's connection rather than asking for the token twice.
            wants_place: false,
            borrows_from: Some("home_assistant"),
            third_party: true,
            input_schema: None,
        }
    }

    fn invoke(
        &self,
        _input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let home =
            crate::home_assistant::HomeAssistant::from_settings(settings).ok_or_else(|| {
                CapabilityError::Unavailable(
                    "the mailbox is read through Home Assistant — set its URL and token in \
                     the Home Assistant skill"
                        .to_owned(),
                )
            })?;
        let entities = home.entities().map_err(CapabilityError::Unavailable)?;
        let said = crate::home_assistant::mail_in(&entities);
        Ok(serde_json::json!({
            "mail": said
                .iter()
                .map(|m| serde_json::json!({
                    "mailbox": m.mailbox,
                    "waiting": m.waiting,
                    "subject": m.subject,
                    "from": m.from,
                    "when": m.when,
                }))
                .collect::<Vec<_>>(),
        }))
    }

    fn summarize(&self, output: &Value) -> String {
        let said: Vec<crate::home_assistant::MailSaid> = output
            .get("mail")
            .and_then(Value::as_array)
            .map(|all| {
                all.iter()
                    .map(|m| crate::home_assistant::MailSaid {
                        mailbox: m["mailbox"].as_str().unwrap_or_default().to_owned(),
                        waiting: m["waiting"].as_u64().and_then(|n| u32::try_from(n).ok()),
                        subject: m["subject"].as_str().map(ToOwned::to_owned),
                        from: m["from"].as_str().map(ToOwned::to_owned),
                        when: m["when"].as_str().map(ToOwned::to_owned),
                    })
                    .collect()
            })
            .unwrap_or_default();
        crate::home_assistant::describe_mail(&said)
    }
}

pub(super) struct CityMeetingsCapability;

impl Capability for CityMeetingsCapability {
    fn for_people(&self) -> Option<&'static str> {
        Some("Your city's council, planning and zoning meetings, from its own agenda.")
    }

    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "city_meetings",
            name: "What the city is doing",
            description: "Council, committee, planning and zoning meetings from the city's \
                          own agenda. Civic business ONLY — it does not know about concerts, \
                          sport, shows or anything at a venue.",
            category: "information",
            reaches_external: true,
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[SettingSpec {
                key: "legistar_client",
                label: "your city's Legistar name (e.g. seattle)",
                secret: false,
                optional: false,
            }],
            wants_place: false,
            borrows_from: None,
            third_party: true,
            input_schema: None,
        }
    }

    fn invoke(
        &self,
        _input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let client = settings
            .get("legistar_client")
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CapabilityError::Unavailable(
                    "tell me your city's Legistar name — the part before .legistar.com in its \
                     meetings page address"
                        .to_owned(),
                )
            })?;
        // Only what has not happened yet. Legistar holds a decade of history and answering
        // "what is on this week" with 2015 would be worse than answering nothing.
        let from = crate::infrastructure::today_utc();
        let url = format!(
            "https://webapi.legistar.com/v1/{client}/events?$filter=EventDate%20ge%20datetime'{from}'&$orderby=EventDate&$top=25"
        );
        let body = http_get_text_ua(
            &url,
            "Endora personal butler (github.com/RusticPotatos/Endora)",
            512 * 1024,
        )?;
        let meetings = meetings_in(&body);
        Ok(serde_json::json!({
            "meetings": meetings
                .iter()
                .map(|m| serde_json::json!({
                    "on": m.on, "at": m.at, "who": m.who, "place": m.place,
                }))
                .collect::<Vec<_>>(),
        }))
    }

    fn summarize(&self, output: &Value) -> String {
        let meetings: Vec<PublicMeeting> = output
            .get("meetings")
            .and_then(Value::as_array)
            .map(|all| {
                all.iter()
                    .map(|m| PublicMeeting {
                        on: m["on"].as_str().unwrap_or_default().to_owned(),
                        at: m["at"].as_str().unwrap_or_default().to_owned(),
                        who: m["who"].as_str().unwrap_or_default().to_owned(),
                        place: m["place"].as_str().unwrap_or_default().to_owned(),
                    })
                    .collect()
            })
            .unwrap_or_default();
        if meetings.is_empty() {
            return "Nothing on the city's public calendar just now.".to_owned();
        }
        said_proportionately(
            meetings.iter().map(describe_meeting).collect(),
            "on the city's public calendar",
        )
    }
}
scaffold!(
    FlightSearchCapability,
    "flights",
    "Flight search",
    "Find and compare flights for a trip.",
    "travel",
    true,
    Reversibility::Irreversible, // booking spends money and can't be undone
    "a flights API key (booking stays a human decision)"
);
scaffold!(
    LocationLogCapability,
    "location",
    "Location tracking",
    "Keep a private log of where you are while travelling, so the butler has context.",
    "presence",
    false,
    Reversibility::OutwardReversible, // a private log you can delete
    "your opt-in and a location source (kept private to you)"
);
/// The "guard dog": active public-safety alerts near a place. Real for the US via
/// the National Weather Service (no key); elsewhere it reports no coverage.
pub(super) struct SafetyAlertsCapability;

impl Capability for SafetyAlertsCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "safety_alerts",
            name: "Guard dog",
            description: "Active safety alerts near you — severe weather and public warnings (US National Weather Service).",
            category: "safety",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0053.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            // Needs where, and said it needed nothing.
            wants_place: true,
            borrows_from: None,
            third_party: false,
            input_schema: Some(
                r#"{"type":"object","properties":{"location":{"type":"string","description":"a town or postcode"},"lat":{"type":"number"},"lon":{"type":"number"}},"required":["location"]}"#,
            ),
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
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

    fn summarize(&self, o: &Value) -> String {
        let place = o["place"]
            .as_str()
            .filter(|s| !s.is_empty())
            .unwrap_or("there");
        let alerts: Vec<String> = o["alerts"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|x| {
                        let event = x["event"].as_str()?;
                        let sev = x["severity"].as_str().unwrap_or("");
                        Some(if sev.is_empty() {
                            event.to_owned()
                        } else {
                            format!("{event} ({sev})")
                        })
                    })
                    .collect()
            })
            .unwrap_or_default();
        if alerts.is_empty() {
            return format!("No active safety alerts for {place} right now.");
        }
        format!("Active safety alerts for {place}: {}.", alerts.join("; "))
    }
}

scaffold!(
    IncidentScannerCapability,
    "incident_scanner",
    "Incident scanner",
    "Surface public emergency/incident alerts nearby (fire, rescue, major incidents).",
    "safety",
    true,
    Reversibility::Reversible, // read-only lookup
    "a public incident/emergency feed for your area"
);
