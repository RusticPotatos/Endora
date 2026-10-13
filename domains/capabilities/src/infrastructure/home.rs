use super::*;

// ---- Home Assistant (read-only; learn the home's routines) -----------------

pub(super) const HA_SETTINGS: &[SettingSpec] = &[
    SettingSpec {
        key: "url",
        label: "Home Assistant URL (e.g. http://homeassistant.local:8123)",
        secret: false,
        optional: false,
    },
    SettingSpec {
        key: "token",
        label: "Long-lived access token",
        secret: true,
        optional: false,
    },
    SettingSpec {
        key: "mcp_server",
        label: "Name of the matching MCP server (blank = home-assistant)",
        secret: false,
        optional: true,
    },
    SettingSpec {
        key: "write_names",
        label: "Let Endora write names back into Home Assistant (on/off)",
        secret: false,
        optional: false,
    },
    SettingSpec {
        key: "busy_entity",
        label: "How Endora can tell you don't want interrupting — an entity that's on when \
                you're busy, e.g. binary_sensor.yourphone_focus (blank = always reach you)",
        secret: false,
        optional: true,
    },
    SettingSpec {
        key: "open_on_tap",
        label: "Where a tapped notification takes you — the address you reach Endora at, \
                e.g. https://192.168.1.10:8787 (blank = it just opens Home Assistant)",
        secret: false,
        optional: true,
    },
    SettingSpec {
        key: "notify_service",
        label: "How Endora reaches you when you're away — a notify service, e.g.                 mobile_app_yourphone (blank = never)",
        secret: false,
        optional: true,
    },
];

/// Presentation and plumbing, which say nothing about the thing itself.
///
/// `friendly_name` is already the name; `supported_features` is a bitmask; `attribution` and
/// `description` are boilerplate a service attaches to every reading.
pub(super) const NOT_ABOUT_THE_THING: &[&str] = &[
    "friendly_name",
    "supported_features",
    "attribution",
    "icon",
    "entity_picture",
    "editable",
    "device_class",
    "hidden_by",
    "assumed_state",
    "restored",
    "id",
    "description",
];

/// The attributes of a thing that carry its meaning, when its **state does not**.
///
/// Connecting a calendar achieved nothing until this existed. A calendar's state is `off`,
/// and its event — *"Jane Doe & John Doe at 18:30"* — lives entirely in its attributes,
/// so Endora read `Family: off` and had nothing to say about the person's evening. The same
/// holds for weather (`clear-night`, with the temperature in an attribute) and for a media
/// player (`playing`, with what is playing in one).
///
/// Deliberately **not a list of which attributes matter per kind of thing** — that would be
/// per-integration knowledge, and there are hundreds of kinds. Three generic rules do it:
///
/// - a **scalar**, so lists and nested objects are left out (a TV's `source_list` is forty
///   app names, and a forecast is an array of days);
/// - **not empty**, so a calendar with no location does not report an empty one;
/// - **short**, and dropped rather than truncated when it is not — a 500-character
///   `description` clipped to sixty is still sixty characters of nothing.
///
/// Capped per thing, because everything here is paid for on every turn that reads it — the
/// same budget that made a clock reading arriving with five kilobytes of house a bug
/// ([0053](../../docs/adr/0053-honesty-about-what-it-did.md)).
pub(crate) fn facts_worth_reading(attributes: &Value) -> serde_json::Map<String, Value> {
    /// Long enough for a time, a title or a temperature; short enough that nothing
    /// discursive gets in.
    const SHORT_ENOUGH: usize = 60;
    /// Enough to describe a thing, few enough that sixty things still fit in a reading.
    const ENOUGH_PER_THING: usize = 8;
    let Some(all) = attributes.as_object() else {
        return serde_json::Map::new();
    };
    all.iter()
        .filter(|(key, _)| !NOT_ABOUT_THE_THING.contains(&key.as_str()))
        .filter(|(_, value)| !value.is_array() && !value.is_object() && !value.is_null())
        .filter(|(_, value)| {
            let shown = value
                .as_str()
                .map_or_else(|| value.to_string(), ToOwned::to_owned);
            !shown.trim().is_empty() && shown.len() <= SHORT_ENOUGH
        })
        .take(ENOUGH_PER_THING)
        .map(|(k, v)| (k.clone(), v.clone()))
        .collect()
}

/// Reads Home Assistant state so the butler can learn the home's routines (lights,
/// presence, sensors). Read-only and reversible — it observes, it does not actuate;
/// controlling devices/scripts is a separate, confirm-gated capability (ADR 0051).
pub(super) struct HomeAssistantCapability;

impl Capability for HomeAssistantCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "home_assistant",
            name: "Home Assistant",
            description: "Read your home's current state — every light, switch, presence \
                          and sensor it has, including ones hidden from voice assistants — \
                          to answer what is on, off, or offline right now.",
            category: "presence",
            reaches_external: true,
            // Reads state; changes nothing. Policy-identical to Reversible
            // (both are Act), but it lets the turn tell an observation from a
            // receipt — see ADR 0053.
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "your Home Assistant URL and a long-lived access token",
            settings: HA_SETTINGS,
            wants_place: false,
            borrows_from: None,
            third_party: false,
            input_schema: Some(
                r#"{"type":"object","properties":{
                     "domain":{"type":"string","description":"filter what it can see: light, person, sensor…"},
                     "history_of":{"type":"string","description":"an entity id, to get what it DID over recent hours from the house's own recorder"},
                     "hours":{"type":"integer","description":"how far back to look (with history_of), default 24"}},
                   "required":[]}"#,
            ),
        }
    }

    /// Proves the whole chain in one press: it can reach Home Assistant, it can read, and
    /// — if a notify service has been nominated — it can reach the person.
    ///
    /// The notification is sent **because** the button was pressed, which is the only
    /// honest way to test that path: a nominated service that is misspelled fails silently
    /// forever otherwise, and the failure looks exactly like "nothing worth saying
    /// happened" (ADR 0056).
    fn self_test(&self, settings: &CapabilitySettings) -> Result<String, CapabilityError> {
        let reading = self.invoke(&Value::Object(serde_json::Map::new()), settings)?;
        let seen = reading
            .get("count")
            .and_then(Value::as_u64)
            .map_or_else(|| self.summarize(&reading), |n| format!("{n} things"));
        let Some(home) = crate::home_assistant::HomeAssistant::from_settings(settings) else {
            return Ok(format!("Connected. Endora can see {seen}."));
        };
        match home.notify("Endora", "Test from Endora — this is how I'll reach you.") {
            None => Ok(format!(
                "Connected. Endora can see {seen}. No notify service is set, so it has no \
                 way to reach you when you're away."
            )),
            Some(Ok(())) => Ok(format!(
                "Connected. Endora can see {seen}, and it just sent a test notification — \
                 check your phone."
            )),
            Some(Err(why)) => Err(CapabilityError::Unavailable(format!(
                "Endora can see {seen}, but the notify service did not work: {why}"
            ))),
        }
    }

    /// Home Assistant is the one service Endora needs a relationship with rather than answers
    /// from — it watches every entity for the world to change, takes presence from it into
    /// every turn, writes names back into it and can undo them.
    ///
    /// The alias filtering lives here rather than in the caller: which confirmed names belong
    /// to this server is Home-Assistant knowledge, and putting it in shared registration code
    /// is precisely the per-integration patch ADR 0054 was written about.
    fn channel(
        &self,
        settings: &CapabilitySettings,
        aliases: &[crate::domain::TargetAlias],
    ) -> Option<(String, std::sync::Arc<dyn NativeChannel>)> {
        let home = crate::home_assistant::HomeAssistant::from_settings(settings)?;
        let server = crate::home_assistant::paired_server(settings);
        // Every name a thing answers to, not just the service's own (ADR 0054) — the same
        // confirmed aliases the retry uses.
        let named: Vec<(String, String)> = aliases
            .iter()
            .filter(|a| a.server == server)
            .map(|a| (a.said.clone(), a.means.clone()))
            .collect();
        Some((server, std::sync::Arc::new(home.also_known_as(named))))
    }

    fn invoke(
        &self,
        input: &Value,
        settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let base = settings
            .get("url")
            .map(|s| as_url(s))
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CapabilityError::Unavailable(
                    "set the Home Assistant URL in this skill's settings".to_owned(),
                )
            })?;
        let token = settings
            .get("token")
            .map(String::as_str)
            .filter(|s| !s.is_empty())
            .ok_or_else(|| {
                CapabilityError::Unavailable(
                    "set a Home Assistant access token in this skill's settings".to_owned(),
                )
            })?;
        // Asked about history — what one thing DID, from the service's own recorder
        // (ADR 0058: answers over a connection that already exists; months of state the
        // house was keeping right beside Endora's fortnight of changes, never asked).
        if let Some(thing) = input.get("history_of").and_then(Value::as_str) {
            let hours = input
                .get("hours")
                .and_then(Value::as_u64)
                .map_or(24, |h| u32::try_from(h.clamp(1, 24 * 14)).unwrap_or(24));
            let home =
                crate::home_assistant::HomeAssistant::from_settings(settings).ok_or_else(|| {
                    CapabilityError::Unavailable(
                        "set the Home Assistant URL and token in this skill's settings".to_owned(),
                    )
                })?;
            let past = home
                .history_of(thing.trim(), hours)
                .map_err(CapabilityError::Unavailable)?;
            return Ok(json!({
                "thing": thing,
                "hours": hours,
                "changes": past.len().saturating_sub(1),
                "history": past
                    .iter()
                    .map(|(state, when)| json!({ "state": state, "at": when }))
                    .collect::<Vec<_>>(),
            }));
        }
        // Optional {domain} filter (e.g. "light", "person", "sensor", "media_player").
        let domain = input.get("domain").and_then(Value::as_str).unwrap_or("");
        // HA is the person's own local service — direct agent (not proxied/guarded).
        use std::io::Read;
        let mut resp = agent()
            .get(&format!("{base}/api/states"))
            .header("Authorization", &format!("Bearer {token}"))
            .call()
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let mut buf = Vec::new();
        resp.body_mut()
            .as_reader()
            .take(1024 * 1024)
            .read_to_end(&mut buf)
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let states: Value = serde_json::from_str(&String::from_utf8_lossy(&buf))
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let entities: Vec<Value> = states
            .as_array()
            .map(|arr| {
                arr.iter()
                    .filter(|e| {
                        domain.is_empty()
                            || e["entity_id"]
                                .as_str()
                                .is_some_and(|id| id.starts_with(&format!("{domain}.")))
                    })
                    .filter_map(|e| {
                        let id = e["entity_id"].as_str()?;
                        let name = e["attributes"]["friendly_name"].as_str().unwrap_or(id);
                        let mut about = json!({
                            "entity": id,
                            "name": name,
                            "state": e["state"].as_str().unwrap_or("?"),
                            "changed": e["last_changed"].as_str().unwrap_or(""),
                        });
                        let facts = facts_worth_reading(&e["attributes"]);
                        if !facts.is_empty() {
                            about["about"] = Value::Object(facts);
                        }
                        Some(about)
                    })
                    .take(60)
                    .collect()
            })
            .unwrap_or_default();
        Ok(json!({ "domain": domain, "count": entities.len(), "entities": entities }))
    }

    fn summarize(&self, o: &Value) -> String {
        let entities: Vec<&Value> = o["entities"]
            .as_array()
            .map(|a| a.iter().collect())
            .unwrap_or_default();
        if entities.is_empty() {
            return "No matching Home Assistant entities found.".to_owned();
        }
        let list = entities
            .iter()
            .take(30)
            .map(|e| {
                let said = format!(
                    "{}: {}",
                    e["name"].as_str().unwrap_or("?"),
                    e["state"].as_str().unwrap_or("?")
                );
                // The facts travel with it, because for many things the state alone says
                // nothing: `Family: off` is a calendar with an event in an hour.
                match e["about"].as_object().filter(|a| !a.is_empty()) {
                    None => said,
                    Some(about) => {
                        let detail = about
                            .iter()
                            .map(|(k, v)| {
                                format!(
                                    "{k}={}",
                                    v.as_str().map_or_else(|| v.to_string(), ToOwned::to_owned)
                                )
                            })
                            .collect::<Vec<_>>()
                            .join(", ");
                        format!("{said} ({detail})")
                    }
                }
            })
            .collect::<Vec<_>>()
            .join("; ");
        format!(
            "Home state ({} entities): {list}",
            o["count"].as_i64().unwrap_or(0)
        )
    }
}
