//! Direct reach into Home Assistant — the named per-integration adapter ADR 0054 called
//! for, and the exact reach ADR 0054 grants.
//!
//! Everything Endora has done with this house so far has gone through the MCP surface
//! Home Assistant offers, which is the **voice assistant** surface: sixteen Assist
//! intents that take fuzzy names and no identifiers. Every failure this week was a name
//! failing to match — "table", "table light", "ceiling light", "main light" — and the
//! answers built for it (aliases, then a search, then a ranked search) are all
//! workarounds for a question that does not arise on the other side of the same
//! service's own API, where a light is `light.kitchen_table` and cannot be mistaken for
//! anything else.
//!
//! So this speaks to Home Assistant the way Home Assistant's own front end does: read
//! every entity from `/api/states`, act through `/api/services` **by entity id**.
//!
//! The Home-Assistant-specific knowledge is all here, behind this integration's own
//! boundary, and nothing above it learns what Home Assistant is: the runner sees only a
//! [`NativeChannel`](crate::infrastructure::NativeChannel).

use serde_json::{Value, json};

/// One thing in the house: what the service calls it, and what a person calls it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entity {
    /// The unambiguous identifier — `light.kitchen_table`. What actions are aimed at.
    pub id: String,
    /// The name a person would say — `Kitchen Table`. What a search matches against.
    pub name: String,
    /// Its state right now, for the reading.
    pub state: String,
    /// When it last changed, as the service reports it. Empty when it does not say.
    ///
    /// A reading without time can answer "is it on?" and never "how long has it been on?"
    /// — and a butler asked the second question improvises rather than declining. The
    /// service already carries this; Endora was discarding it.
    pub since: String,
    /// What sort of thing it is: its domain (`light`) and its device class where it has
    /// one. The vocabulary of *kinds*, as opposed to names (ADR 0054).
    pub kinds: Vec<String>,
    /// The facts its state does not carry — a calendar's event, a forecast's temperature.
    ///
    /// A calendar's state is `off` whether the evening is empty or not, so an entity
    /// without these is unreadable for a whole class of thing.
    pub facts: serde_json::Map<String, Value>,
}
/// `2026-08-03T04:00:00Z` from unix millis — the recorder's own date shape.
fn format_utc(ms: i64) -> String {
    let secs = ms.div_euclid(1000);
    let days = secs.div_euclid(86_400);
    let tod = secs.rem_euclid(86_400);
    let date = crate::infrastructure::civil_from_days(days);
    format!(
        "{date}T{:02}:{:02}:{:02}Z",
        tod / 3600,
        (tod % 3600) / 60,
        tod % 60
    )
}

/// The `(state, when)` pairs in a recorder answer.
///
/// Parsed, never trusted, and a renamed field yields nothing rather than a half-built row —
/// the same posture as every other read of somebody else's service. The recorder answers an
/// array per entity; `minimal_response` strips attributes, and the first entry of each run
/// carries `last_changed` while later ones may not.
#[must_use]
pub fn states_in_history(body: &str) -> Vec<(String, String)> {
    let Ok(serde_json::Value::Array(per_entity)) = serde_json::from_str(body) else {
        return Vec::new();
    };
    per_entity
        .iter()
        .filter_map(|rows| rows.as_array())
        .flatten()
        .filter_map(|row| {
            let state = row.get("state")?.as_str()?.trim();
            if state.is_empty() {
                return None;
            }
            let when = row
                .get("last_changed")
                .or_else(|| row.get("last_updated"))
                .and_then(serde_json::Value::as_str)
                .unwrap_or_default();
            Some((state.to_owned(), when.to_owned()))
        })
        .collect()
}

/// A configured connection to a Home Assistant instance.
pub struct HomeAssistant {
    base: String,
    token: String,
    /// Whether the person has allowed Endora to write names back (ADR 0054). Seeing and
    /// acting are one grant; editing the service's own configuration is another, and it
    /// is off until deliberately turned on.
    may_write: bool,
    /// The entity the person nominated as meaning "not now", empty when they did not.
    ///
    /// Notifications were deliberately **not** gated on presence when they were built,
    /// because the only signal available was free text a service had written — *"john is
    /// not home"* — and being wrong either wakes somebody or silently swallows the alert
    /// they wanted. A **boolean entity** removes that objection entirely: their phone's
    /// Focus mode is already on/off, already in the house, and already means exactly this.
    busy_entity: String,
    /// Where a tapped notification should take the person, empty when they have not said.
    ///
    /// Without it the push arrives, they tap it, and Home Assistant opens showing nothing —
    /// because `notify.mobile_app_*` sends a notification and leaves no record anywhere. The
    /// record lives in Endora, so that is where a tap belongs.
    opens_at: String,
    /// The notify service the person nominated as how to reach them, without its
    /// `notify.` prefix. Empty means Endora never interrupts them through this service.
    notify_service: String,
    /// The other names the person has confirmed for things here, as `(said, means)`.
    ///
    /// A thing answers to more than one name, and Endora only knew the service's own
    /// (ADR 0054). Live: writing aliases into Home Assistant made its Assist view list
    /// them *instead of* the friendly name, so `Kitchen Table` — the very name the
    /// confirmed alias resolved to — stopped being a name the service recognised.
    aliases: Vec<(String, String)>,
}

impl HomeAssistant {
    /// Builds a connection from the URL and long-lived token the person stored against
    /// the Home Assistant skill. `None` when either is missing — the honest default, and
    /// the whole feature simply stays off.
    #[must_use]
    pub fn from_settings(settings: &crate::infrastructure::CapabilitySettings) -> Option<Self> {
        let base = crate::infrastructure::as_url(settings.get("url")?);
        let token = settings.get("token")?.trim().to_owned();
        // Which notify service to reach the person through. Blank means never — being able
        // to interrupt somebody is granted, not inferred.
        let busy_entity = settings
            .get("busy_entity")
            .map(|v| v.trim().to_owned())
            .unwrap_or_default();
        let opens_at = settings
            .get("open_on_tap")
            .map(|v| v.trim().trim_end_matches('/').to_owned())
            .unwrap_or_default();
        let notify_service = settings
            .get("notify_service")
            .map(|v| v.trim().trim_start_matches("notify.").to_owned())
            .unwrap_or_default();
        let may_write = settings
            .get("write_names")
            .map(|v| v.trim().to_lowercase())
            .is_some_and(|v| ["on", "yes", "true", "1"].contains(&v.as_str()));
        (!base.is_empty() && !token.is_empty()).then_some(Self {
            base,
            token,
            may_write,
            busy_entity,
            opens_at,
            notify_service,
            aliases: Vec::new(),
        })
    }

    /// Adds the names the person has confirmed, so this channel knows a thing by every
    /// name it answers to rather than only the one the service prints.
    #[must_use]
    pub fn also_known_as(mut self, aliases: Vec<(String, String)>) -> Self {
        self.aliases = aliases;
        self
    }

    /// Every name each entity answers to: the service's own, plus any the person
    /// confirmed for it. Ordered so the service's name comes first.
    fn names_of(&self, entity: &Entity) -> Vec<String> {
        let mut names = vec![entity.name.clone()];
        for (said, means) in &self.aliases {
            if means.eq_ignore_ascii_case(&entity.name)
                && !names.iter().any(|n| n.eq_ignore_ascii_case(said))
            {
                names.push(said.clone());
            }
        }
        names
    }

    /// Every entity Home Assistant knows about — **not** only the ones exposed to
    /// Assist, which is what the MCP reading is limited to. Something the model could
    /// never find because it was hidden from the voice surface is visible here.
    ///
    /// # Errors
    /// A human-readable message if Home Assistant cannot be reached or replies badly.
    pub fn entities(&self) -> Result<Vec<Entity>, String> {
        let body = self.get("/api/states")?;
        entities_in(&body)
            .ok_or_else(|| "Home Assistant did not return a list of states".to_owned())
    }
}

/// The entities in a `/api/states` answer — separated from the fetch so the shapes this
/// house actually returns can be asserted without a house.
#[must_use]
pub fn entities_in(body: &str) -> Option<Vec<Entity>> {
    let states: Value = serde_json::from_str(body).ok()?;
    let entities = states
        .as_array()?
        .iter()
        .filter_map(|e| {
            let id = e["entity_id"].as_str()?;
            let mut kinds = Vec::new();
            if let Some((domain, _)) = id.split_once('.') {
                kinds.push(domain.to_owned());
            }
            if let Some(class) = e["attributes"]["device_class"].as_str() {
                kinds.push(class.to_owned());
            }
            Some(Entity {
                id: id.to_owned(),
                name: e["attributes"]["friendly_name"]
                    .as_str()
                    .unwrap_or(id)
                    .to_owned(),
                state: e["state"].as_str().unwrap_or("?").to_owned(),
                since: e["last_changed"].as_str().unwrap_or_default().to_owned(),
                facts: crate::infrastructure::facts_worth_reading(&e["attributes"]),
                kinds,
            })
        })
        .collect();
    Some(entities)
}

impl HomeAssistant {
    /// Calls a service on exactly one entity, and reports **what happened** in words.
    ///
    /// Home Assistant answers a service call with the list of states it changed, so a
    /// successful call that changed nothing replies `[]`. Handing that back verbatim was
    /// observed live: the tool result was the two characters `[]`, and the butler — with
    /// nothing else to go on — told the person "I'm not sure how to help with that yet"
    /// about an action that had just succeeded.
    ///
    /// So the reply is read rather than forwarded. `[]` is not a failure; it is "already
    /// in that state", which is worth saying to the model, the record and the person.
    ///
    /// # Errors
    /// A human-readable message if the call fails.
    pub fn call_service(
        &self,
        domain: &str,
        service: &str,
        entity: &str,
    ) -> Result<String, String> {
        let path = format!("/api/services/{domain}/{service}");
        let body = self.post(&path, &json!({ "entity_id": entity }))?;
        Ok(describe_service_result(&body, service, entity))
    }

    /// What one thing did over the last stretch of hours, from the service's own recorder.
    ///
    /// The house keeps months of state history behind a query API, and Endora kept its own
    /// fortnight of changes beside it without ever asking. This is the ask. Answers, not a
    /// relationship (ADR 0058) — one authenticated GET against a connection that already
    /// exists, so nothing new is configured and nothing new is trusted.
    ///
    /// # Errors
    /// A human-readable message if the recorder cannot be read.
    pub fn history_of(&self, entity: &str, hours: u32) -> Result<Vec<(String, String)>, String> {
        let start_ms = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| e.to_string())?
            .as_millis()
            .saturating_sub(u128::from(hours) * 3_600_000);
        let start = format_utc(i64::try_from(start_ms).unwrap_or_default());
        let body = self.get(&format!(
            "/api/history/period/{start}?filter_entity_id={entity}&minimal_response"
        ))?;
        Ok(states_in_history(&body))
    }

    fn get(&self, path: &str) -> Result<String, String> {
        use std::io::Read;
        let mut resp = crate::infrastructure::agent()
            .get(&format!("{}{path}", self.base))
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        resp.body_mut()
            .as_reader()
            .take(4 * 1024 * 1024)
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn delete(&self, path: &str) -> Result<String, String> {
        use std::io::Read;
        let mut resp = crate::infrastructure::agent()
            .delete(&format!("{}{path}", self.base))
            .header("Authorization", &format!("Bearer {}", self.token))
            .call()
            .map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        resp.body_mut()
            .as_reader()
            .take(64 * 1024)
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }

    fn post(&self, path: &str, body: &Value) -> Result<String, String> {
        use std::io::Read;
        let mut resp = crate::infrastructure::agent()
            .post(&format!("{}{path}", self.base))
            .header("Authorization", &format!("Bearer {}", self.token))
            .header("Content-Type", "application/json")
            .send(body.to_string())
            .map_err(|e| e.to_string())?;
        let mut buf = Vec::new();
        resp.body_mut()
            .as_reader()
            .take(256 * 1024)
            .read_to_end(&mut buf)
            .map_err(|e| e.to_string())?;
        Ok(String::from_utf8_lossy(&buf).into_owned())
    }
}

/// What an alias write changed, kept so it can be put back (ADR 0054).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AliasWrite {
    /// The entity whose names were edited.
    pub entity: String,
    /// The alias that was added.
    pub added: String,
    /// Every alias it had **before** — the undo.
    pub was: Vec<String>,
}

impl HomeAssistant {
    /// Teaches Home Assistant that `alias` is another name for `entity`, so the service
    /// itself resolves it from then on — for every client, voice assistants included, and
    /// not only inside Endora (ADR 0054).
    ///
    /// Strictly **additive**. Existing aliases are read first and preserved, the new one
    /// is appended, and the prior list is returned so the edit can be undone. A rename or
    /// a removal is a different, destructive thing and this cannot do it.
    ///
    /// # Errors
    /// A human-readable message if Home Assistant cannot be reached, refuses the edit, or
    /// does not know the entity.
    pub fn add_alias(&self, entity: &str, alias: &str) -> Result<AliasWrite, String> {
        let alias = alias.trim();
        if alias.is_empty() {
            return Err("an empty alias is not a name".to_owned());
        }
        let mut socket = self.connect_ws()?;
        let entry = ws_call(
            &mut socket,
            1,
            &json!({ "type": "config/entity_registry/get", "entity_id": entity }),
        )?;
        let was: Vec<String> = entry["aliases"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        if was.iter().any(|a| a.eq_ignore_ascii_case(alias)) {
            return Ok(AliasWrite {
                entity: entity.to_owned(),
                added: alias.to_owned(),
                was,
            });
        }
        let mut now = was.clone();
        now.push(alias.to_owned());
        ws_call(
            &mut socket,
            2,
            &json!({
                "type": "config/entity_registry/update",
                "entity_id": entity,
                "aliases": now,
            }),
        )?;
        Ok(AliasWrite {
            entity: entity.to_owned(),
            added: alias.to_owned(),
            was,
        })
    }

    /// Whether the person has said, through their own service, that this is not the moment.
    ///
    /// **False on any doubt** — unset, unreachable, unreadable. Refusing to interrupt on a
    /// failed read would mean a broken sensor silently cancelling every alert, and a
    /// notification nobody receives is worse than one that arrives at a bad time.
    fn says_not_now(&self) -> bool {
        if self.busy_entity.is_empty() {
            return false;
        }
        self.get(&format!("/api/states/{}", self.busy_entity))
            .ok()
            .and_then(|raw| serde_json::from_str::<Value>(&raw).ok())
            .and_then(|state| state["state"].as_str().map(str::to_owned))
            .is_some_and(|state| state.eq_ignore_ascii_case("on"))
    }

    /// Removes a setup form that was started and not finished, so an abandoned attempt does
    /// not sit in the person's own interface waiting for them.
    ///
    /// # Errors
    /// A human-readable message if Home Assistant cannot be reached.
    pub fn abandon_setup(&self, form: &str) -> Result<(), String> {
        self.delete(&format!("/api/config/config_entries/flow/{form}"))?;
        Ok(())
    }

    /// Finds an entity id by the name the registry holds for it, including hidden ones.
    ///
    /// Separate from the ordinary reading because that reading is a list of *states*, and
    /// a hidden entity has no entry in it — so showing something again could never find
    /// its target if it went looking there.
    ///
    /// # Errors
    /// A human-readable message if Home Assistant cannot be reached.
    fn registry_entry_named(&self, name: &str) -> Result<Option<String>, String> {
        let wanted = name.trim();
        let mut socket = self.connect_ws()?;
        let listed = ws_call(
            &mut socket,
            1,
            &json!({ "type": "config/entity_registry/list" }),
        )?;
        // Both naming spaces, reconciled here — see `entity_named`. The reading is fetched
        // only when the registry has no answer, so the ordinary case costs nothing extra and
        // a Home Assistant that will not list states cannot break un-hiding.
        if let Some(found) = entity_named(&listed, &[], wanted) {
            return Ok(Some(found));
        }
        Ok(entity_named(
            &Value::Null,
            &self.entities().unwrap_or_default(),
            wanted,
        ))
    }

    /// Hides an entity from Home Assistant's own interface, or shows it again. Returns
    /// **whether it was hidden before**, so the change can be put back exactly.
    ///
    /// # Errors
    /// A human-readable message if Home Assistant cannot be reached or refuses the edit.
    fn set_hidden(&self, entity: &str, hidden: bool) -> Result<bool, String> {
        let mut socket = self.connect_ws()?;
        let entry = ws_call(
            &mut socket,
            1,
            &json!({ "type": "config/entity_registry/get", "entity_id": entity }),
        )?;
        let was = !entry["hidden_by"].is_null();
        ws_call(
            &mut socket,
            2,
            &json!({
                "type": "config/entity_registry/update",
                "entity_id": entity,
                // `user` rather than `integration`: this is a person's decision, recorded
                // as one, and it stays undoable from their own interface as well as ours.
                "hidden_by": if hidden { json!("user") } else { Value::Null },
            }),
        )?;
        Ok(was)
    }

    /// Removes one alias, leaving the rest. The prior list comes back with it, so the
    /// removal undoes exactly as an addition does.
    ///
    /// # Errors
    /// A human-readable message if Home Assistant cannot be reached or refuses the edit.
    pub fn remove_alias(&self, entity: &str, alias: &str) -> Result<AliasWrite, String> {
        let alias = alias.trim();
        let mut socket = self.connect_ws()?;
        let entry = ws_call(
            &mut socket,
            1,
            &json!({ "type": "config/entity_registry/get", "entity_id": entity }),
        )?;
        let was: Vec<String> = entry["aliases"]
            .as_array()
            .map(|a| {
                a.iter()
                    .filter_map(|v| v.as_str().map(str::to_owned))
                    .collect()
            })
            .unwrap_or_default();
        if !was.iter().any(|a| a.eq_ignore_ascii_case(alias)) {
            return Err(format!("{entity} does not answer to '{alias}'"));
        }
        let now: Vec<&String> = was
            .iter()
            .filter(|a| !a.eq_ignore_ascii_case(alias))
            .collect();
        ws_call(
            &mut socket,
            2,
            &json!({
                "type": "config/entity_registry/update",
                "entity_id": entity,
                "aliases": now,
            }),
        )?;
        Ok(AliasWrite {
            entity: entity.to_owned(),
            added: alias.to_owned(),
            was,
        })
    }

    /// Creates a **group helper** holding the given entities, and returns its new id.
    ///
    /// Home Assistant makes helpers through a config *flow* — a short conversation rather
    /// than a single write: start the flow, choose the sort of group, then supply the name
    /// and members. Three round trips on the socket already open for the registry.
    ///
    /// # Errors
    /// A human-readable message if Home Assistant cannot be reached or refuses a step.
    pub fn create_group(&self, name: &str, entities: &[String]) -> Result<String, String> {
        let name = name.trim();
        if name.is_empty() {
            return Err("a group needs a name".to_owned());
        }
        if entities.is_empty() {
            return Err("a group of nothing is not a group".to_owned());
        }
        // Every member has to be the same sort of thing, and that sort decides which
        // group Home Assistant makes. Taken from the members themselves rather than
        // guessed.
        let domain = entities
            .first()
            .and_then(|e| e.split_once('.'))
            .map(|(d, _)| d.to_owned())
            .ok_or("those do not look like entity ids")?;
        if let Some(odd) = entities
            .iter()
            .find(|e| !e.starts_with(&format!("{domain}.")))
        {
            return Err(format!(
                "a group holds one sort of thing, and {odd} is not a {domain}"
            ));
        }
        // Config flows are REST, not the socket the registry uses — a helper is created
        // by walking a short form, not by writing a row.
        let started = self.post(
            "/api/config/config_entries/flow",
            &json!({ "handler": "group", "show_advanced_options": false }),
        )?;
        let started: Value = serde_json::from_str(&started).map_err(|e| e.to_string())?;
        let flow = started["flow_id"]
            .as_str()
            .ok_or_else(|| format!("Home Assistant did not start a group flow: {started}"))?
            .to_owned();
        let step = format!("/api/config/config_entries/flow/{flow}");
        // Pick the kind of group; the menu is keyed by domain.
        self.post(&step, &json!({ "next_step_id": domain }))?;
        let made = self.post(
            &step,
            &json!({ "name": name, "entities": entities, "hide_members": false }),
        )?;
        let made: Value = serde_json::from_str(&made).map_err(|e| e.to_string())?;
        made["result"]["entry_id"]
            .as_str()
            .map(ToOwned::to_owned)
            .ok_or_else(|| format!("Home Assistant made no group: {made}"))
    }

    /// Removes a helper Endora created, by its config entry id.
    ///
    /// # Errors
    /// A human-readable message if Home Assistant cannot be reached or refuses.
    pub fn remove_entry(&self, entry_id: &str) -> Result<(), String> {
        self.delete(&format!("/api/config/config_entries/entry/{entry_id}"))?;
        Ok(())
    }

    /// Puts an entity's aliases back exactly as they were — the undo for
    /// [`add_alias`](Self::add_alias).
    ///
    /// # Errors
    /// A human-readable message if Home Assistant cannot be reached or refuses the edit.
    pub fn restore_aliases(&self, undo: &AliasWrite) -> Result<(), String> {
        let mut socket = self.connect_ws()?;
        ws_call(
            &mut socket,
            1,
            &json!({
                "type": "config/entity_registry/update",
                "entity_id": undo.entity,
                "aliases": undo.was,
            }),
        )?;
        Ok(())
    }

    /// Opens an authenticated WebSocket to Home Assistant.
    ///
    /// The entity registry is **not** on the REST API — the naming of things lives behind
    /// the same socket Home Assistant's own front end uses, which is why owning the names
    /// needs this and reading and acting did not.
    fn connect_ws(&self) -> Result<Socket, String> {
        let url = ws_url(&self.base);
        let (mut socket, _) = tungstenite::connect(&url).map_err(|e| format!("{url}: {e}"))?;
        // Home Assistant greets first, then wants the token, then says ok.
        let greeting = read_json(&mut socket)?;
        if greeting["type"] != "auth_required" {
            return Err(format!(
                "expected an auth request from Home Assistant, got {}",
                greeting["type"]
            ));
        }
        send_json(
            &mut socket,
            &json!({ "type": "auth", "access_token": self.token }),
        )?;
        let accepted = read_json(&mut socket)?;
        if accepted["type"] != "auth_ok" {
            return Err("Home Assistant refused the access token".to_owned());
        }
        Ok(socket)
    }
}

type Socket = tungstenite::WebSocket<tungstenite::stream::MaybeTlsStream<std::net::TcpStream>>;

/// The WebSocket address for a Home Assistant base URL.
fn ws_url(base: &str) -> String {
    let base = base.trim_end_matches('/');
    let swapped = match base.split_once("://") {
        Some(("https", rest)) => format!("wss://{rest}"),
        Some((_, rest)) => format!("ws://{rest}"),
        None => format!("ws://{base}"),
    };
    format!("{swapped}/api/websocket")
}

/// Sends one command and returns its `result`, skipping the events Home Assistant
/// interleaves on the same socket.
fn ws_call(socket: &mut Socket, id: u64, command: &Value) -> Result<Value, String> {
    let mut command = command.clone();
    command["id"] = json!(id);
    send_json(socket, &command)?;
    // Bounded: a reply that never arrives must not hang a turn.
    for _ in 0..32 {
        let message = read_json(socket)?;
        if message["id"].as_u64() != Some(id) || message["type"] != "result" {
            continue;
        }
        if message["success"] == json!(false) {
            let why = message["error"]["message"]
                .as_str()
                .unwrap_or("Home Assistant refused it");
            return Err(why.to_owned());
        }
        return Ok(message["result"].clone());
    }
    Err("Home Assistant never answered".to_owned())
}

fn send_json(socket: &mut Socket, value: &Value) -> Result<(), String> {
    socket
        .send(tungstenite::Message::Text(value.to_string().into()))
        .map_err(|e| e.to_string())
}

fn read_json(socket: &mut Socket) -> Result<Value, String> {
    loop {
        let message = socket.read().map_err(|e| e.to_string())?;
        let tungstenite::Message::Text(text) = message else {
            continue; // pings and frames Home Assistant sends for its own reasons
        };
        return serde_json::from_str(&text).map_err(|e| e.to_string());
    }
}

/// Turns Home Assistant's answer to a service call into a sentence.
///
/// The answer is the list of entities whose state it changed. Empty means the call was
/// accepted and nothing moved — almost always because it was already that way.
fn describe_service_result(body: &str, service: &str, entity: &str) -> String {
    let changed = serde_json::from_str::<Value>(body)
        .ok()
        .and_then(|v| v.as_array().map(Vec::len));
    let did = service.replace('_', " ");
    match changed {
        // An empty list is NOT "already as asked", which is what this said until a light
        // that demonstrably turned off was recorded as having changed nothing. Home
        // Assistant answers the call before its integrations report back, so an empty
        // list means "accepted, nothing had changed YET" — a statement about timing, not
        // about the outcome. Reading it as an outcome put a false claim in the record.
        Some(0) => format!(
            "Home Assistant accepted '{did}' on {entity}. Its reply says nothing about \
             the result — state arrives separately — so the read-back decides."
        ),
        Some(n) => format!("Home Assistant did '{did}' on {entity} ({n} changed)."),
        // Not a list at all: report it, rather than inventing a reading of it.
        None => format!("Home Assistant accepted '{did}' on {entity}. It replied: {body}"),
    }
}

/// The domains `homeassistant.turn_on` / `turn_off` can actually operate.
///
/// Everything else a house holds — sensors, diagnostics, configuration entries, update
/// and connectivity indicators — reports or configures rather than switches. They matter
/// because they are **named after the device they belong to**: `Kitchen Main Light LED`
/// and `Kitchen Main Light Cloud connection` sit beside `Kitchen Main Light` and tie with
/// it in any search for that name.
const SWITCHABLE: &[&str] = &[
    "light",
    "switch",
    "fan",
    "cover",
    "climate",
    "media_player",
    "scene",
    "script",
    "automation",
    "input_boolean",
    "humidifier",
    "vacuum",
    "lock",
    "siren",
    "valve",
    "water_heater",
];

/// The time-of-day part of an ISO timestamp, which is all a reading needs.
///
/// A full timestamp is noise in a prompt a local model has to read; `14:32` says the same
/// thing in five characters. Anything that is not a timestamp comes back empty rather than
/// mangled, so a service reporting something unexpected simply contributes no time.
fn clock_time(iso: &str) -> String {
    let Some((date, rest)) = iso.split_once('T') else {
        return String::new();
    };
    if date.len() != 10 || rest.len() < 5 {
        return String::new();
    }
    rest.chars().take(5).collect()
}

/// One person's presence, in words.
fn describe_presence(name: &str, state: &str) -> String {
    match state {
        "home" => format!("{name} is home"),
        "not_home" | "unknown" | "unavailable" => format!("{name} is not home"),
        place => format!("{name} is at {place}"),
    }
}

/// What it is like outside, from the service's own forecast.
///
/// The weather skill takes a location as a *call argument*, so the model has to pass one and
/// does not — it failed with `missing 'location'` while a correct forecast for this address
/// sat unused in the house. A fact the butler reads needs neither.
///
/// The temperature is an attribute, not the state: a weather entity's state is `clear-night`,
/// which is why this was invisible until entities carried their facts.
fn describe_weather(e: &Entity) -> Option<String> {
    if !e.id.starts_with("weather.") {
        return None;
    }
    let degrees = e.facts.get("temperature")?;
    // The service's own unit, because it knows which one this person reads in.
    let unit = e
        .facts
        .get("temperature_unit")
        .and_then(Value::as_str)
        .unwrap_or_default();
    Some(format!(
        "outside it is {degrees}{unit} and {}",
        e.state.replace('-', " ")
    ))
}

/// Finds the entity a name refers to, across **both** of Home Assistant's naming spaces.
///
/// The registry stores an entity's own name. The states reading shows a **composed** display
/// name — the device's name and the entity's together, `Pixel` + `Kiosk Brightness` — and
/// that composed form is what every screen says, what standing trouble is raised with, and
/// what the person taps.
///
/// Resolving only against the registry meant anything belonging to a named device could be
/// complained about and never acted on: *"nothing here is called Pixel Kiosk Brightness"*,
/// about an entity sitting right there. Neither name is wrong; they are answers to different
/// questions, and this is the one place both have to reconcile.
///
/// **The registry is still tried first**, and that ordering is load-bearing rather than
/// arbitrary: a hidden entity drops out of the reading entirely, so un-hiding one can only
/// ever be resolved there.
fn entity_named(registry: &Value, reading: &[Entity], wanted: &str) -> Option<String> {
    let wanted = wanted.trim();
    let in_registry = registry.as_array().and_then(|all| {
        all.iter()
            .find(|e| {
                [e["name"].as_str(), e["original_name"].as_str()]
                    .into_iter()
                    .flatten()
                    .any(|n| n.eq_ignore_ascii_case(wanted))
            })
            .and_then(|e| e["entity_id"].as_str().map(str::to_owned))
    });
    in_registry.or_else(|| {
        reading
            .iter()
            .find(|e| e.name.trim().eq_ignore_ascii_case(wanted))
            .map(|e| e.id.clone())
    })
}

/// What is still outstanding on one of the person's lists, in a sentence.
///
/// Endora has held add, complete, remove and read on these lists since the Home Assistant
/// server was registered, and has never once touched them. Not for want of a tool — it could
/// not *see* that anything was on a list, so it had no reason to reach for one.
///
/// The morning brief taught this already: one instruction to "reach for whatever's relevant"
/// produced four days of briefs about the kitchen lights, because every fact worth having was
/// in the turn and all of it was left to the model to go and fetch. So this is **stated**, not
/// fetched.
///
/// A todo entity's state is the number of things left on it. `None` for anything that is not a
/// list, for an empty one — the same judgement [`describe_engagement`] makes about an empty
/// day — and for a list that is not answering, since "unavailable things on your list" is
/// worse than silence and a broken entity belongs in standing trouble.
fn describe_outstanding(e: &Entity) -> Option<String> {
    if !e.id.starts_with("todo.") {
        return None;
    }
    let left: u32 = e.state.trim().parse().ok()?;
    if left == 0 {
        return None;
    }
    Some(format!(
        "{left} thing{} on your {} list",
        if left == 1 { "" } else { "s" },
        e.name
    ))
}

/// What a calendar entry says about the day, in a sentence.
///
/// `None` for anything that is not a calendar, and for a calendar with nothing on it — an
/// empty day is not a fact worth a line in every turn.
fn describe_engagement(e: &Entity) -> Option<String> {
    if !e.id.starts_with("calendar.") {
        return None;
    }
    let what = e.facts.get("message")?.as_str()?.trim();
    if what.is_empty() {
        return None;
    }
    let when = e
        .facts
        .get("start_time")
        .and_then(Value::as_str)
        .unwrap_or_default();
    // The service's own words for the time, not a reformatting of them: Endora has no
    // business deciding what "18:30" means in somebody's timezone when the service already
    // wrote it in theirs.
    Some(match when.is_empty() {
        true => format!("on the {} calendar: {what}", e.name),
        false => format!("on the {} calendar: {what} at {when}", e.name),
    })
}

/// The entities that belong to **the person themselves**, rather than to the household.
///
/// A hallway light is the house's; a phone in their pocket is not. That difference decides
/// whether a reading may ever become a belief about them (ADR 0057), so it is **read from the
/// service, never inferred**: Home Assistant already holds the mapping because the person set
/// it up, and a `person` entity lists the trackers its presence is computed from.
///
/// **More than one person means Endora will not guess.** With two `person` entities nothing
/// here can say which one Endora serves, and the cost of being wrong is a false belief about
/// somebody rather than a bad reading. Attributing nothing until somebody says is the honest
/// answer, and it is the direction that fails safely as a household grows.
#[must_use]
pub fn the_persons_own_things(states_body: &str) -> Vec<String> {
    let Ok(states) = serde_json::from_str::<Value>(states_body) else {
        return Vec::new();
    };
    let Some(states) = states.as_array() else {
        return Vec::new();
    };
    let people: Vec<&Value> = states
        .iter()
        .filter(|e| {
            e.get("entity_id")
                .and_then(Value::as_str)
                .is_some_and(|id| id.starts_with("person."))
        })
        .collect();
    let [person] = people[..] else {
        return Vec::new(); // nobody, or more than one and no way to tell which
    };
    let mut theirs: Vec<String> = person
        .get("attributes")
        .and_then(|a| a.get("device_trackers"))
        .and_then(Value::as_array)
        .map(|all| {
            all.iter()
                .filter_map(Value::as_str)
                .map(str::to_owned)
                .collect()
        })
        .unwrap_or_default();
    // The person entity itself is theirs too — it is the thing their presence *is*.
    if let Some(id) = person.get("entity_id").and_then(Value::as_str) {
        theirs.push(id.to_owned());
    }
    theirs.sort();
    theirs.dedup();
    theirs
}

/// The integrations Home Assistant already has configured, by domain, sorted and deduplicated.
///
/// Two Hue bridges or two calendars are **one** integration here: the question this answers is
/// "is this connected?", which has a single answer however many accounts sit behind it.
///
/// Anything unreadable yields no integrations rather than an error. This only ever decorates a
/// screen, and a service answering oddly should cost the person a label, never the ability to
/// connect something.
#[must_use]
pub fn configured_integrations(body: &str) -> Vec<String> {
    let Ok(entries) = serde_json::from_str::<Value>(body) else {
        return Vec::new();
    };
    let Some(entries) = entries.as_array() else {
        return Vec::new();
    };
    let mut found: Vec<String> = entries
        .iter()
        .filter_map(|e| e.get("domain").and_then(Value::as_str))
        .map(str::to_owned)
        .collect();
    found.sort();
    found.dedup();
    found
}

/// The MCP server this instance is the direct counterpart of.
///
/// Data rather than a constant in the wiring: whoever registers the MCP server chooses
/// its name, and a name hardcoded in Endora would be exactly the per-integration guessing
/// ADR 0054 rules out. Defaults to the conventional name so nobody has to fill it in.
#[must_use]
pub fn paired_server(settings: &crate::infrastructure::CapabilitySettings) -> String {
    settings
        .get("mcp_server")
        .map(|s| s.trim())
        .filter(|s| !s.is_empty())
        .unwrap_or("home-assistant")
        .to_owned()
}

/// The service that does what an Assist intent was trying to do.
///
/// The one piece of genuine Home-Assistant knowledge in this file, and the reason the
/// file exists: `HassTurnOn` and `homeassistant.turn_on` are the same act expressed
/// against two different surfaces, one of which cannot mis-aim.
///
/// Only switching is mapped. Brightness and colour carry arguments this does not attempt
/// to translate, and a half-translated action is worse than none — those fall back to the
/// existing retry.
#[must_use]
pub fn service_for(tool: &str) -> Option<(&'static str, &'static str)> {
    match tool.rsplit('.').next()? {
        "HassTurnOn" => Some(("homeassistant", "turn_on")),
        "HassTurnOff" => Some(("homeassistant", "turn_off")),
        _ => None,
    }
}

/// Reads one step of a config flow into the shape the rest of the system speaks.
///
/// `Ok(None)` when the service says it is finished — a created entry. Anything with a form
/// in it is another question to put to the person.
fn read_form(raw: &str) -> Result<Option<crate::domain::SetupForm>, String> {
    use crate::domain::{SetupField, SetupForm};
    let step: Value = serde_json::from_str(raw).map_err(|e| e.to_string())?;
    // An abort is the service declining, and its reason is the person's to read.
    if step["type"] == "abort" {
        let why = step["reason"]
            .as_str()
            .unwrap_or("it stopped without saying why");
        return Err(format!("Home Assistant stopped: {why}"));
    }
    if step["type"] != "form" {
        return Ok(None);
    }
    // Errors on a form are per-field complaints about what was just sent — surfaced rather
    // than swallowed, because "wrong password" is the whole reason someone is looking.
    if let Some(errors) = step["errors"].as_object().filter(|e| !e.is_empty()) {
        let said: Vec<String> = errors
            .iter()
            .map(|(field, why)| format!("{field}: {}", why.as_str().unwrap_or("not accepted")))
            .collect();
        return Err(said.join(" · "));
    }
    let fields = step["data_schema"]
        .as_array()
        .map(|all| {
            all.iter()
                .filter_map(|f| {
                    let name = f["name"].as_str()?.to_owned();
                    Some(SetupField {
                        secret: SetupField::looks_secret(&name),
                        kind: f["type"].as_str().unwrap_or("string").to_owned(),
                        required: f["required"].as_bool().unwrap_or(false),
                        default: f["default"].as_str().map(ToOwned::to_owned),
                        name,
                    })
                })
                .collect()
        })
        .unwrap_or_default();
    Ok(Some(SetupForm {
        id: step["flow_id"].as_str().unwrap_or_default().to_owned(),
        step: step["step_id"].as_str().unwrap_or_default().to_owned(),
        fields,
    }))
}

impl crate::infrastructure::NativeChannel for HomeAssistant {
    fn known(&self) -> Result<Vec<(String, String)>, String> {
        Ok(self
            .entities()?
            .into_iter()
            .flat_map(|e| {
                self.names_of(&e)
                    .into_iter()
                    .map(move |n| (e.id.clone(), n))
                    .collect::<Vec<_>>()
            })
            .collect())
    }

    fn reading(&self) -> Result<String, String> {
        // Rendered for a text search, in the same shape a reader tool returns, so nothing
        // above this has to care where a reading came from.
        //
        // Ids are deliberately NOT in the text. `light.kitchen_table` contains the words
        // "kitchen" and "table", so it would compete with `Kitchen Table` as a candidate
        // and turn an unambiguous match into a tie that refuses to act. The id is looked
        // up separately, through `known`, which is exactly what it is for.
        // One line per NAME, not per thing. A thing that answers to three names has three
        // chances to be matched exactly, and none of them is diluted by the others — the
        // failure this fixes was `table light` losing because it was read as part of
        // "kitchen table light, table, table light".
        Ok(self
            .entities()?
            .iter()
            .flat_map(|e| {
                self.names_of(e)
                    .into_iter()
                    .map(|n| {
                        let since = clock_time(&e.since);
                        if since.is_empty() {
                            format!("names: {n}\n  state: {}", e.state)
                        } else {
                            format!("names: {n}\n  state: {}\n  since: {since}", e.state)
                        }
                    })
                    .collect::<Vec<_>>()
            })
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn actionable(&self, tool: &str, entity: &str) -> bool {
        if service_for(tool).is_none() {
            return true; // not a tool this channel expresses; it narrows nothing
        }
        entity
            .split_once('.')
            .is_some_and(|(domain, _)| SWITCHABLE.contains(&domain))
    }

    fn states(&self) -> Result<Vec<(String, String)>, String> {
        // Keyed by **entity id**, never by friendly name.
        //
        // Friendly names are not unique and this house proves it: a device tracker and a
        // timestamp sensor both answer to one name, so their readings collapsed into a
        // single key and every pass flipped it between a place and a date. That is a
        // permanent fake transition — it recorded a change every two minutes forever, it
        // was "rare" by every measure that counts distinct values, and it is what woke the
        // butler for nothing.
        //
        // ADR 0059 already argued this one level up: two servers may publish the same
        // resource name, so keys carry their source. Two entities may share a friendly
        // name for exactly the same reason, and the fix is the same — use the id that
        // cannot collide. The name stays where names belong, in prose.
        Ok(self
            .entities()?
            .into_iter()
            .map(|e| (e.id, e.state))
            .collect())
    }

    fn about_the_person(&self) -> Option<String> {
        // A house knows whether someone is in it. Home Assistant keeps that as `person.*`
        // entities whose state is `home`, `not_home`, or the name of a place — which is
        // already in the reading Endora fetches, and was going unused.
        let everything = self.entities().ok()?;
        let mut said: Vec<String> = everything
            .iter()
            .filter(|e| e.id.starts_with("person."))
            .map(|e| describe_presence(&e.name, &e.state))
            .collect();
        // What is on today. A calendar is the plainest fact about somebody's day and it was
        // invisible: its state is `off` whether the evening is empty or not, so a connected
        // calendar changed nothing until it travelled as a fact the butler reads.
        //
        // Stated rather than fetched, for the reason three placements of the activity
        // account established: asked what was on tonight with the event in the house, the
        // model answered about the living room lights (ADR 0056).
        said.extend(everything.iter().filter_map(describe_engagement));
        // What is still outstanding. Endora could add to these lists and complete things on
        // them from the day it connected, and never did — because nothing ever told it there
        // was anything on one.
        said.extend(everything.iter().filter_map(describe_outstanding));
        said.extend(everything.iter().filter_map(describe_weather));
        (!said.is_empty()).then(|| said.join("; "))
    }

    fn refuse(&self, tool: &str, input_json: &str) -> Option<String> {
        // `HassLightSet` changes brightness or colour. Asked to switch a light on or off,
        // the model reaches for it repeatedly, and with no brightness or colour given the
        // call is accepted and does nothing — reporting success while the light stays as
        // it was. Refusing says what happened and points at the tools that would work.
        if tool.rsplit('.').next()? != "HassLightSet" {
            return None;
        }
        let args: Value = serde_json::from_str(input_json).ok()?;
        let sets_something = ["brightness", "color", "temperature"]
            .iter()
            .any(|k| args.get(*k).is_some_and(|v| !v.is_null()));
        if sets_something {
            return None;
        }
        Some(
            "HassLightSet only changes brightness or colour, and none were given, so this \
             would have done nothing. To switch a light on or off use HassTurnOn or \
             HassTurnOff instead."
                .to_owned(),
        )
    }

    fn tighten(&self, input_json: &str) -> Option<String> {
        let mut args: Value = serde_json::from_str(input_json).ok()?;
        let obj = args.as_object_mut()?;
        // `entity_id` is Home Assistant's identifier field — the one piece of naming
        // knowledge this needs, and it lives here rather than in the shared runner.
        let pinned = obj
            .get("entity_id")
            .and_then(Value::as_str)
            .is_some_and(|v| !v.trim().is_empty());
        if !pinned {
            return None;
        }
        let before = obj.len();
        // Everything else that says WHICH thing. Kind filters stay: they narrow too, and
        // dropping them is the widening this exists to prevent.
        obj.retain(|field, _| !["area", "floor", "name"].contains(&field.as_str()));
        (obj.len() < before).then(|| args.to_string())
    }

    fn categories(&self) -> Result<Vec<String>, String> {
        // Read off the house itself rather than from a list in Endora's source: every
        // domain it actually has, and every device class it actually uses.
        let mut kinds: Vec<String> = self
            .entities()?
            .into_iter()
            .flat_map(|e| e.kinds)
            .map(|k| k.to_lowercase())
            .collect();
        kinds.sort();
        kinds.dedup();
        Ok(kinds)
    }

    fn act(&self, tool: &str, entity: &str) -> Option<Result<String, String>> {
        let (domain, service) = service_for(tool)?;
        Some(self.call_service(domain, service, entity))
    }

    /// Which entities are the person's own, straight from Home Assistant's `person` mapping.
    ///
    /// Failure is silence, which attributes nothing — the safe direction.
    fn belongs_to_the_person(&self) -> Vec<String> {
        self.get("/api/states")
            .map(|body| the_persons_own_things(&body))
            .unwrap_or_default()
    }

    /// The integrations Home Assistant already holds, so the Connect screen can say what is
    /// there rather than offering to set it all up again.
    ///
    /// Failure is silence: this decorates a screen, and a Home Assistant that will not answer
    /// should cost a label rather than the ability to connect something.
    fn already_connected(&self) -> Vec<String> {
        self.get("/api/config/config_entries/entry")
            .map(|body| configured_integrations(&body))
            .unwrap_or_default()
    }

    fn begin_setup(&self, kind: &str) -> Option<Result<crate::domain::SetupForm, String>> {
        if !self.may_write {
            return None;
        }
        Some(
            self.post(
                "/api/config/config_entries/flow",
                &json!({ "handler": kind.trim(), "show_advanced_options": false }),
            )
            .and_then(|raw| read_form(&raw))
            .and_then(|form| {
                form.ok_or_else(|| format!("{kind} had nothing to ask, so nothing was set up"))
            }),
        )
    }

    fn finish_setup(
        &self,
        form: &str,
        answers: &[(String, String)],
    ) -> Option<Result<Option<crate::domain::SetupForm>, String>> {
        if !self.may_write {
            return None;
        }
        let body: serde_json::Map<String, Value> = answers
            .iter()
            .map(|(k, v)| (k.clone(), Value::String(v.clone())))
            .collect();
        Some(
            self.post(
                &format!("/api/config/config_entries/flow/{form}"),
                &Value::Object(body),
            )
            .and_then(|raw| read_form(&raw)),
        )
    }

    fn notify(&self, title: &str, body: &str) -> Option<Result<(), String>> {
        if self.notify_service.is_empty() {
            return None;
        }
        // Nominated, and read at the moment of interrupting rather than remembered. A
        // service that cannot be reached is NOT taken as "busy": failing to read it must
        // not silently swallow the alert, which is the failure mode that makes people stop
        // trusting notifications (ADR 0056).
        if self.says_not_now() {
            return None;
        }
        // Home Assistant's own notify service, which is already delivering to this
        // person's phone. Endora adds no push stack of its own.
        Some(
            self.post(&format!("/api/services/notify/{}", self.notify_service), &{
                let mut payload = json!({ "title": title, "message": body });
                // Somewhere to go. The companion app opens this on tap; without it a
                // person taps the notification and lands on a screen that knows nothing
                // about the message they just read.
                if !self.opens_at.is_empty() {
                    payload["data"] = json!({ "url": self.opens_at });
                }
                payload
            })
            .map(|_| ()),
        )
    }

    fn hide(&self, name: &str, hidden: bool) -> Option<Result<crate::domain::ConfigWrite, String>> {
        if !self.may_write {
            return None;
        }
        // Hidden entities drop out of `/api/states`, so the thing being un-hidden is not
        // in the ordinary reading — the registry is the only place it still exists, and
        // the only place both directions can be resolved from.
        let entity = match self.registry_entry_named(name) {
            Ok(Some(found)) => found,
            Ok(None) => return Some(Err(format!("nothing here is called {name}"))),
            Err(e) => return Some(Err(e)),
        };
        Some(
            self.set_hidden(&entity, hidden)
                .map(|was| crate::domain::ConfigWrite {
                    id: 0, // the caller stamps identity and time; the adapter knows neither
                    at_ms: 0,
                    server: String::new(),
                    target: entity,
                    added: if hidden { "hidden" } else { "shown" }.to_owned(),
                    // What it was, so the undo is a stored fact rather than an assumption
                    // that everything started out visible (ADR 0054).
                    was: vec![if was { "hidden" } else { "shown" }.to_owned()],
                    undone: false,
                    kind: crate::domain::WriteKind::Hidden,
                }),
        )
    }

    fn teach(&self, name: &str, alias: &str) -> Option<Result<crate::domain::ConfigWrite, String>> {
        if !self.may_write {
            return None;
        }
        let entity = match self.entities() {
            Ok(all) => all
                .into_iter()
                .find(|e| e.name.eq_ignore_ascii_case(name.trim()))?,
            Err(e) => return Some(Err(e)),
        };
        // Already knowing the name is not a change, so it is not logged as one — an undo
        // log full of no-ops is a log nobody reads.
        Some(self.add_alias(&entity.id, alias).map(|write| {
            crate::domain::ConfigWrite {
                id: 0, // the caller stamps identity and time; the adapter knows neither
                at_ms: 0,
                server: String::new(),
                target: write.entity,
                added: write.added,
                was: write.was,
                undone: false,
                kind: crate::domain::WriteKind::Name,
            }
        }))
    }

    fn collect(
        &self,
        name: &str,
        ids: &[String],
    ) -> Option<Result<crate::domain::ConfigWrite, String>> {
        if !self.may_write {
            return None;
        }
        Some(
            self.create_group(name, ids)
                .map(|entry_id| crate::domain::ConfigWrite {
                    id: 0,
                    at_ms: 0,
                    server: String::new(),
                    // The entry id, because that is what removing it needs. The members
                    // are the prior value: nothing existed before, and they are what the
                    // collection was made of.
                    target: entry_id,
                    added: name.to_owned(),
                    was: ids.to_vec(),
                    undone: false,
                    kind: crate::domain::WriteKind::Collection,
                }),
        )
    }

    fn forget(
        &self,
        name: &str,
        alias: &str,
    ) -> Option<Result<crate::domain::ConfigWrite, String>> {
        if !self.may_write {
            return None;
        }
        let entity = match self.entities() {
            Ok(all) => all
                .into_iter()
                .find(|e| e.name.eq_ignore_ascii_case(name.trim()))?,
            Err(e) => return Some(Err(e)),
        };
        Some(
            self.remove_alias(&entity.id, alias)
                .map(|write| crate::domain::ConfigWrite {
                    id: 0,
                    at_ms: 0,
                    server: String::new(),
                    target: write.entity,
                    added: write.added,
                    was: write.was,
                    undone: false,
                    kind: crate::domain::WriteKind::Name,
                }),
        )
    }

    fn undo(&self, write: &crate::domain::ConfigWrite) -> Option<Result<String, String>> {
        if !self.may_write {
            return None;
        }
        // A collection is undone by removing it. Replaying its members as names would
        // strip every name off whatever it points at — which is why the kind is stored
        // rather than guessed from the prior value.
        if write.kind == crate::domain::WriteKind::Collection {
            return Some(
                self.remove_entry(&write.target)
                    .map(|()| format!("'{}' is gone.", write.added)),
            );
        }
        let restore = AliasWrite {
            entity: write.target.clone(),
            added: write.added.clone(),
            was: write.was.clone(),
        };
        Some(
            self.restore_aliases(&restore)
                .map(|()| format!("{} no longer answers to '{}'.", write.target, write.added)),
        )
    }
}

#[cfg(test)]
mod tests {

    #[test]
    fn a_reading_is_keyed_by_id_because_names_collide() {
        // The live failure: a device tracker and a timestamp sensor answering to one
        // friendly name collapsed into a single key, and every pass flipped it between a
        // place and a date — a permanent fake transition, recorded every two minutes,
        // "rare" by every measure that counts distinct values, and the reason the butler
        // woke for nothing.
        let states = super::entities_in(
            r#"[{"entity_id":"device_tracker.pixel","state":"not_home",
                 "attributes":{"friendly_name":"Pixel"}},
                {"entity_id":"sensor.pixel_seen","state":"2026-08-02T11:01:36+00:00",
                 "attributes":{"friendly_name":"Pixel"}}]"#,
        )
        .expect("a list of states parses");
        let keys: Vec<&str> = states.iter().map(|e| e.id.as_str()).collect();
        assert_eq!(keys, vec!["device_tracker.pixel", "sensor.pixel_seen"]);
        // Both answer to the same name, which is exactly why the name cannot be the key.
        assert_eq!(states[0].name, states[1].name);
    }

    #[test]
    fn the_recorder_answer_is_parsed_never_trusted() {
        // Shaped as /api/history/period answers with minimal_response: an array per
        // entity; later entries of a run may carry last_updated instead of last_changed.
        let body = r#"[[
            {"state":"off","last_changed":"2026-08-03T01:00:00+00:00"},
            {"state":"on","last_updated":"2026-08-03T02:30:00+00:00"},
            {"state":"","last_changed":"2026-08-03T03:00:00+00:00"}
        ]]"#;
        let past = super::states_in_history(body);
        assert_eq!(past.len(), 2, "a blank state is not a reading");
        assert_eq!(past[0].0, "off");
        assert_eq!(
            past[1],
            ("on".to_owned(), "2026-08-03T02:30:00+00:00".to_owned())
        );
        // A renamed field yields nothing rather than a half-built row.
        assert!(super::states_in_history(r#"[[{"status":"on"}]]"#).is_empty());
        assert!(super::states_in_history("not json").is_empty());
    }

    #[test]
    fn the_recorder_is_asked_in_its_own_date_shape() {
        // 2026-08-03 00:00:00 UTC.
        assert_eq!(super::format_utc(1_785_715_200_000), "2026-08-03T00:00:00Z");
    }
    use super::*;

    #[test]
    fn a_missing_url_or_token_means_no_direct_reach() {
        let mut settings = crate::infrastructure::CapabilitySettings::new();
        assert!(HomeAssistant::from_settings(&settings).is_none());
        settings.insert("url".to_owned(), "http://ha.local:8123".to_owned());
        assert!(
            HomeAssistant::from_settings(&settings).is_none(),
            "a URL alone is not a connection"
        );
        settings.insert("token".to_owned(), "  ".to_owned());
        assert!(
            HomeAssistant::from_settings(&settings).is_none(),
            "a blank token is not a token"
        );
        settings.insert("token".to_owned(), "abc".to_owned());
        assert!(HomeAssistant::from_settings(&settings).is_some());
    }

    #[test]
    fn the_socket_address_follows_the_base_url() {
        assert_eq!(
            ws_url("http://ha.local:8123"),
            "ws://ha.local:8123/api/websocket"
        );
        assert_eq!(
            ws_url("https://ha.example.com/"),
            "wss://ha.example.com/api/websocket"
        );
        assert_eq!(ws_url("ha.local:8123"), "ws://ha.local:8123/api/websocket");
    }

    #[test]
    fn an_empty_alias_is_refused_before_anything_is_opened() {
        let mut settings = crate::infrastructure::CapabilitySettings::new();
        settings.insert("url".to_owned(), "http://ha.local:8123".to_owned());
        settings.insert("token".to_owned(), "abc".to_owned());
        let home = HomeAssistant::from_settings(&settings).unwrap();
        assert!(home.add_alias("light.x", "   ").is_err());
    }

    #[test]
    fn an_empty_reply_claims_nothing_about_the_outcome() {
        // Two live lessons in one test. The reply `[]` must not be forwarded raw — the
        // butler answered "I'm not sure how to help with that yet" about an action that
        // had succeeded — and it must not be read as "already as asked" either: a light
        // that verifiably turned off was recorded as having changed nothing.
        let said = describe_service_result("[]", "turn_off", "light.kitchen_table");
        assert!(said.contains("light.kitchen_table"), "{said}");
        assert!(said.contains("accepted"), "{said}");
        assert!(
            !said.contains("already") && !said.contains("changed nothing"),
            "claimed an outcome the reply does not report: {said}"
        );
    }

    #[test]
    fn a_real_change_says_what_it_did() {
        let said = describe_service_result(
            r#"[{"entity_id":"light.kitchen_table"}]"#,
            "turn_on",
            "light.kitchen_table",
        );
        assert!(said.contains("turn on"), "{said}");
        assert!(said.contains("light.kitchen_table"), "{said}");
    }

    #[test]
    fn an_answer_that_is_not_a_list_is_reported_not_interpreted() {
        let said = describe_service_result("service not found", "turn_on", "light.x");
        assert!(said.contains("service not found"), "{said}");
    }

    #[test]
    fn the_paired_server_is_data_with_a_sensible_default() {
        let mut settings = crate::infrastructure::CapabilitySettings::new();
        assert_eq!(paired_server(&settings), "home-assistant");
        settings.insert("mcp_server".to_owned(), "  ".to_owned());
        assert_eq!(
            paired_server(&settings),
            "home-assistant",
            "blank is not a name"
        );
        settings.insert("mcp_server".to_owned(), "house".to_owned());
        assert_eq!(paired_server(&settings), "house");
    }

    #[test]
    fn switching_maps_to_a_service_and_everything_else_falls_back() {
        assert_eq!(
            service_for("home-assistant.HassTurnOn"),
            Some(("homeassistant", "turn_on"))
        );
        assert_eq!(
            service_for("home-assistant.HassTurnOff"),
            Some(("homeassistant", "turn_off"))
        );
        // Brightness carries arguments this does not translate; a half-translated action
        // is worse than none.
        assert_eq!(service_for("home-assistant.HassLightSet"), None);
        assert_eq!(service_for("home-assistant.HassBroadcast"), None);
    }

    fn house() -> HomeAssistant {
        let mut settings = crate::infrastructure::CapabilitySettings::new();
        settings.insert("url".to_owned(), "http://ha.local:8123".to_owned());
        settings.insert("token".to_owned(), "abc".to_owned());
        HomeAssistant::from_settings(&settings)
            .unwrap()
            .also_known_as(vec![
                ("table light".to_owned(), "Kitchen Table".to_owned()),
                ("table".to_owned(), "Kitchen Table".to_owned()),
                ("ceiling light".to_owned(), "Kitchen Main Light".to_owned()),
            ])
    }

    fn entity(name: &str) -> Entity {
        Entity {
            id: "light.x".to_owned(),
            name: name.to_owned(),
            state: "on".to_owned(),
            since: String::new(),
            kinds: vec!["light".to_owned()],
            facts: serde_json::Map::new(),
        }
    }

    #[test]
    fn a_reading_carries_when_something_last_changed() {
        // A reading without time can answer "is it on?" and never "how long has it been
        // on?" — and asked the second question, the butler improvises rather than saying
        // it cannot tell.
        assert_eq!(clock_time("2026-07-28T14:32:07.123456+00:00"), "14:32");
        // Anything that is not a timestamp contributes nothing rather than being mangled.
        assert_eq!(clock_time("unavailable"), "");
        assert_eq!(clock_time(""), "");
        assert_eq!(clock_time("2026-07-28T"), "");
    }

    #[test]
    fn a_thing_is_known_by_every_name_it_answers_to() {
        // Live: writing aliases into Home Assistant made its Assist view list them
        // INSTEAD of the friendly name, so `Kitchen Table` — the name the confirmed alias
        // resolves to — stopped being a name the service recognised. Endora has to hold
        // both, or the retry substitutes a name nothing answers to.
        let names = house().names_of(&entity("Kitchen Table"));
        assert_eq!(
            names[0], "Kitchen Table",
            "the service's own name comes first"
        );
        assert!(names.contains(&"table light".to_owned()), "{names:?}");
        assert!(names.contains(&"table".to_owned()), "{names:?}");
        assert!(
            !names.contains(&"ceiling light".to_owned()),
            "took another thing's name"
        );
    }

    #[test]
    fn a_thing_nobody_renamed_keeps_exactly_one_name() {
        assert_eq!(
            house().names_of(&entity("Garage Main")),
            vec!["Garage Main".to_owned()]
        );
    }

    #[test]
    fn a_confirmed_name_matching_the_service_name_is_not_listed_twice() {
        let names = house().names_of(&entity("table light"));
        assert_eq!(names.len(), 1, "{names:?}");
    }

    #[test]
    fn presence_reads_as_a_sentence_not_a_state_string() {
        assert_eq!(describe_presence("john", "home"), "john is home");
        assert_eq!(describe_presence("john", "not_home"), "john is not home");
        // A named zone is a place, and reads better as one than as "not_home".
        assert_eq!(describe_presence("john", "Office"), "john is at Office");
        // A tracker that has lost the person says so plainly rather than inventing a place.
        assert_eq!(describe_presence("john", "unavailable"), "john is not home");
    }

    #[test]
    fn an_exact_id_makes_a_room_redundant() {
        // Live, and it succeeded — which is why nothing caught it. A request for ONE light
        // arrived naming the light and the whole kitchen; Home Assistant matched the room
        // and switched off both kitchen lights, reporting success.
        use crate::infrastructure::NativeChannel;
        let out = house()
            .tighten(r#"{"entity_id":"light.kitchen_table","area":"kitchen","domain":["light"]}"#)
            .expect("left the room in the call");
        let v: Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["entity_id"], "light.kitchen_table");
        assert!(v.get("area").is_none(), "kept the room: {out}");
        assert_eq!(
            v["domain"],
            serde_json::json!(["light"]),
            "dropped a kind filter: {out}"
        );
    }

    #[test]
    fn a_call_without_an_id_is_left_exactly_as_it_was() {
        // Narrowing only applies where something already pins the target. Everything else
        // is the recovery path's business, after a failure.
        use crate::infrastructure::NativeChannel;
        assert!(
            house()
                .tighten(r#"{"name":"table light","area":"kitchen"}"#)
                .is_none()
        );
        assert!(
            house()
                .tighten(r#"{"entity_id":"  ","area":"kitchen"}"#)
                .is_none()
        );
        assert!(house().tighten("not json").is_none());
    }

    #[test]
    fn an_id_on_its_own_needs_no_narrowing() {
        use crate::infrastructure::NativeChannel;
        assert!(
            house()
                .tighten(r#"{"entity_id":"light.kitchen_table"}"#)
                .is_none()
        );
    }

    #[test]
    fn a_light_set_that_sets_nothing_is_refused_before_it_is_sent() {
        use crate::infrastructure::NativeChannel;
        let why = house()
            .refuse("home-assistant.HassLightSet", r#"{"area":"kitchen"}"#)
            .expect("a call that can do nothing was sent");
        assert!(why.contains("HassTurnOn"), "{why}");
    }

    #[test]
    fn a_light_set_that_actually_sets_something_goes_through() {
        use crate::infrastructure::NativeChannel;
        for args in [
            r#"{"area":"kitchen","brightness":50}"#,
            r#"{"area":"kitchen","color":"warm white"}"#,
            r#"{"name":"Kitchen Table","temperature":2700}"#,
        ] {
            assert!(
                house()
                    .refuse("home-assistant.HassLightSet", args)
                    .is_none(),
                "should have been allowed: {args}"
            );
        }
        // A null is not a value.
        assert!(
            house()
                .refuse("home-assistant.HassLightSet", r#"{"brightness":null}"#)
                .is_some()
        );
    }

    #[test]
    fn the_refusal_is_scoped_to_the_one_tool_that_needs_it() {
        use crate::infrastructure::NativeChannel;
        // Switching something off with no extra parameters is exactly right for these.
        for tool in [
            "home-assistant.HassTurnOff",
            "home-assistant.HassTurnOn",
            "notes.search",
        ] {
            assert!(
                house().refuse(tool, r#"{"area":"kitchen"}"#).is_none(),
                "{tool}"
            );
        }
        // Unparseable input is passed on rather than guessed at.
        assert!(
            house()
                .refuse("home-assistant.HassLightSet", "{bad")
                .is_none()
        );
    }
}

#[cfg(test)]
mod where_a_tapped_notification_goes {
    use crate::infrastructure::CapabilitySettings;

    fn settings(pairs: &[(&str, &str)]) -> CapabilitySettings {
        pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect()
    }

    #[test]
    fn the_address_is_read_and_tidied() {
        // A trailing slash would make the tapped URL "…8787//", which some clients refuse.
        let home = super::HomeAssistant::from_settings(&settings(&[
            ("url", "http://ha.local:8123"),
            ("token", "t"),
            ("open_on_tap", "  https://192.168.1.10:8787/  "),
        ]))
        .expect("configured");
        assert_eq!(home.opens_at, "https://192.168.1.10:8787");
    }

    #[test]
    fn no_address_means_no_link_rather_than_a_broken_one() {
        // Blank is the shipped state, and it must not become "data": {"url": ""} — a tap
        // target of nowhere is worse than none, because the app tries to follow it.
        let home = super::HomeAssistant::from_settings(&settings(&[
            ("url", "http://ha.local:8123"),
            ("token", "t"),
        ]))
        .expect("configured");
        assert!(home.opens_at.is_empty());
    }
}

#[cfg(test)]
mod what_is_on_today {
    use super::{Entity, describe_engagement};

    fn calendar(name: &str, message: &str, start: &str) -> Entity {
        let mut facts = serde_json::Map::new();
        if !message.is_empty() {
            facts.insert(
                "message".to_owned(),
                serde_json::Value::String(message.to_owned()),
            );
        }
        if !start.is_empty() {
            facts.insert(
                "start_time".to_owned(),
                serde_json::Value::String(start.to_owned()),
            );
        }
        Entity {
            id: format!("calendar.{}", name.to_lowercase()),
            name: name.to_owned(),
            state: "off".to_owned(),
            since: String::new(),
            kinds: vec!["calendar".to_owned()],
            facts,
        }
    }

    #[test]
    fn an_engagement_is_read_from_the_facts_not_the_state() {
        // The live one. A calendar's state is `off` whether the evening is empty or not, so
        // a connected calendar told the butler nothing until this existed.
        let tonight = calendar("Family", "Jane Doe & John Doe", "2026-07-31 18:30:00");
        let said = describe_engagement(&tonight).expect("an engagement");
        assert!(said.contains("Jane Doe & John Doe"), "{said}");
        assert!(said.contains("18:30"), "{said}");
        // The service's own words for the time — Endora has no business deciding what
        // "18:30" means in somebody's timezone when the service wrote it in theirs.
        assert!(said.contains("2026-07-31 18:30:00"), "{said}");
    }

    #[test]
    fn an_empty_day_is_not_a_line_in_every_turn() {
        assert_eq!(describe_engagement(&calendar("Home", "", "")), None);
    }

    #[test]
    fn only_a_calendar_is_read_this_way() {
        let mut lamp = calendar("Family", "something", "now");
        lamp.id = "light.kitchen".to_owned();
        assert_eq!(describe_engagement(&lamp), None);
    }
}

#[cfg(test)]
mod one_thing_with_two_names {
    //! Resolving a name back to an entity (ADR 0054).
    //!
    //! Live: the person tapped **It's gone** on a card Endora had raised, naming
    //! `Pixel Kiosk Brightness`, and got back *"nothing here is called Pixel Kiosk
    //! Brightness"*. The entity was right there.
    //!
    //! Home Assistant has two naming spaces. The **states reading** shows a composed display
    //! name — the device's name plus the entity's, `Pixel` + `Kiosk Brightness` — while the
    //! **registry** stores only the entity's own part. Trouble is raised from the reading and
    //! was resolved against the registry, so anything belonging to a named device could be
    //! complained about and never acted on.

    use super::{Entity, entity_named};
    use serde_json::json;

    fn reading(id: &str, name: &str) -> Entity {
        Entity {
            id: id.to_owned(),
            name: name.to_owned(),
            state: "unavailable".to_owned(),
            since: String::new(),
            kinds: vec![],
            facts: serde_json::Map::new(),
        }
    }

    #[test]
    fn the_registry_answers_when_it_holds_the_whole_name() {
        let registry = json!([
            {"entity_id":"light.lamp","name":"living room lamp","original_name":null}
        ]);
        assert_eq!(
            entity_named(&registry, &[], "living room lamp"),
            Some("light.lamp".to_owned())
        );
    }

    #[test]
    fn a_name_the_person_never_renamed_still_resolves() {
        let registry = json!([
            {"entity_id":"light.lamp","name":null,"original_name":"Living Room Lamp"}
        ]);
        assert_eq!(
            entity_named(&registry, &[], "living room lamp"),
            Some("light.lamp".to_owned())
        );
    }

    #[test]
    fn a_device_prefixed_name_falls_back_to_what_the_reading_shows() {
        // The live bug, exactly. The registry knows it as "Kiosk Brightness"; every screen —
        // and the trouble card the person tapped — calls it "Pixel Kiosk Brightness".
        let registry = json!([
            {"entity_id":"sensor.pixel_kiosk_brightness","name":null,
             "original_name":"Kiosk Brightness"}
        ]);
        let states = [reading(
            "sensor.pixel_kiosk_brightness",
            "Pixel Kiosk Brightness",
        )];
        assert_eq!(
            entity_named(&registry, &states, "Pixel Kiosk Brightness"),
            Some("sensor.pixel_kiosk_brightness".to_owned())
        );
    }

    #[test]
    fn the_registry_wins_when_both_could_answer() {
        // Un-hiding needs the registry: a hidden entity drops out of the reading entirely, so
        // the registry must stay the first place looked rather than a fallback of a fallback.
        let registry = json!([
            {"entity_id":"light.from_registry","name":"the lamp","original_name":null}
        ]);
        let states = [reading("light.from_reading", "the lamp")];
        assert_eq!(
            entity_named(&registry, &states, "the lamp"),
            Some("light.from_registry".to_owned())
        );
    }

    #[test]
    fn something_genuinely_absent_is_still_absent() {
        let registry = json!([{"entity_id":"light.lamp","name":"the lamp"}]);
        assert_eq!(entity_named(&registry, &[], "a thing nobody has"), None);
    }

    #[test]
    fn a_registry_that_answers_oddly_does_not_take_the_reading_down_with_it() {
        let states = [reading("sensor.x", "Pixel Kiosk Brightness")];
        assert_eq!(
            entity_named(&json!({"error": "nope"}), &states, "Pixel Kiosk Brightness"),
            Some("sensor.x".to_owned())
        );
    }
}

#[cfg(test)]
mod what_is_still_outstanding {
    //! Putting the person's lists into the turn (ADR 0056).
    //!
    //! Endora has held full read/write on the todo lists since the day the Home Assistant
    //! server was registered — add, complete, remove, read — and has never once touched them.
    //! Not for want of a tool: it could not *see* that anything was on a list, so it had no
    //! reason to reach for one.
    //!
    //! This is the lesson the morning brief already taught. One instruction to "reach for
    //! whatever's relevant" produced four days of briefs about the kitchen lights, because
    //! every fact worth having was in the turn and all of it was left to the model to go and
    //! fetch. So this is stated, not fetched.

    use super::{Entity, describe_outstanding};

    fn list(name: &str, id: &str, state: &str) -> Entity {
        Entity {
            id: format!("todo.{id}"),
            name: name.to_owned(),
            state: state.to_owned(),
            since: String::new(),
            kinds: vec!["todo".to_owned()],
            facts: serde_json::Map::new(),
        }
    }

    #[test]
    fn a_list_with_things_on_it_says_how_many() {
        assert_eq!(
            describe_outstanding(&list("Reminders", "reminders", "2")),
            Some("2 things on your Reminders list".to_owned())
        );
    }

    #[test]
    fn one_thing_is_not_two_things() {
        assert_eq!(
            describe_outstanding(&list("Family", "family", "1")),
            Some("1 thing on your Family list".to_owned())
        );
    }

    #[test]
    fn an_empty_list_is_not_worth_a_line_in_every_turn() {
        // The same judgement `describe_engagement` makes about an empty day. A butler that
        // reports nothing-to-do on every single turn has made the context noisier, not more
        // useful.
        assert_eq!(
            describe_outstanding(&list("Reminders", "reminders", "0")),
            None
        );
    }

    #[test]
    fn a_list_that_is_not_answering_says_nothing_rather_than_guessing() {
        // `unavailable` is not a count, and reporting "unavailable things on your list" is
        // worse than silence. Standing trouble is where a broken entity belongs.
        for state in ["unavailable", "unknown", "", "lots"] {
            assert_eq!(
                describe_outstanding(&list("Reminders", "reminders", state)),
                None,
                "expected silence for {state:?}"
            );
        }
    }

    #[test]
    fn nothing_that_is_not_a_list_is_described() {
        let mut light = list("Kitchen", "x", "3");
        light.id = "light.kitchen".to_owned();
        assert_eq!(describe_outstanding(&light), None);
    }
}

#[cfg(test)]
mod which_things_are_the_persons_own {
    //! Telling the person's own devices from the household's (ADR 0057).
    //!
    //! A hallway light belongs to the house, which has other people in it. A phone in their
    //! pocket does not. That difference is the whole of attribution, and getting it wrong does
    //! not produce a wrong reading — it produces a wrong belief about somebody.
    //!
    //! **So it is read, never inferred.** Home Assistant already holds the mapping, because
    //! the person set it up themselves: a `person` entity lists the trackers it is computed
    //! from.

    use super::the_persons_own_things;

    fn body(entities: &str) -> String {
        format!("[{entities}]")
    }

    #[test]
    fn a_persons_own_trackers_are_theirs_and_so_is_the_person_entity() {
        let states = body(
            r#"{"entity_id":"person.john","attributes":{"device_trackers":["device_tracker.pixel","device_tracker.watch"]}},
               {"entity_id":"light.hall","attributes":{}}"#,
        );
        assert_eq!(
            the_persons_own_things(&states),
            vec![
                "device_tracker.pixel",
                "device_tracker.watch",
                "person.john",
            ]
        );
    }

    #[test]
    fn more_than_one_person_means_endora_will_not_guess() {
        // The house has other people in it. With two `person` entities nothing here can say
        // which one Endora serves — and ADR 0057 rejects attributing by guesswork outright,
        // because the cost of being wrong is a false belief about somebody rather than a bad
        // reading. Attributing nothing is the honest answer until somebody says.
        let states = body(
            r#"{"entity_id":"person.john","attributes":{"device_trackers":["device_tracker.pixel"]}},
               {"entity_id":"person.jane","attributes":{"device_trackers":["device_tracker.jane_phone"]}}"#,
        );
        assert!(the_persons_own_things(&states).is_empty());
    }

    #[test]
    fn a_person_with_no_trackers_still_counts_as_themselves() {
        let states = body(r#"{"entity_id":"person.john","attributes":{}}"#);
        assert_eq!(the_persons_own_things(&states), vec!["person.john"]);
    }

    #[test]
    fn a_house_with_nobody_in_it_attributes_nothing() {
        for states in ["[]", "", "not json", r#"[{"entity_id":"light.hall"}]"#] {
            assert!(
                the_persons_own_things(states).is_empty(),
                "expected nothing from {states:?}"
            );
        }
    }
}

#[cfg(test)]
mod what_is_already_connected {
    //! Reading Home Assistant's own list of configured integrations.
    //!
    //! The Connect screen offered every service with a **Connect** button and knew nothing
    //! about what was already set up — so somebody with CalDAV working saw "Connect" beside
    //! it, and had nowhere on the screen to find out whether it had worked. Offering to do
    //! something already done is worse than not offering: it reads as though the last attempt
    //! failed.

    use super::configured_integrations;

    #[test]
    fn it_names_each_configured_integration() {
        let body = r#"[
            {"entry_id":"a","domain":"caldav","title":"Calendar"},
            {"entry_id":"b","domain":"hue","title":"Hue"}
        ]"#;
        assert_eq!(configured_integrations(body), vec!["caldav", "hue"]);
    }

    #[test]
    fn two_of_the_same_thing_are_one_integration() {
        // Two Hue bridges, or two calendars on different accounts. The person asked "is this
        // connected?", which is answered once.
        let body = r#"[
            {"entry_id":"a","domain":"hue"},
            {"entry_id":"b","domain":"hue"},
            {"entry_id":"c","domain":"caldav"}
        ]"#;
        assert_eq!(configured_integrations(body), vec!["caldav", "hue"]);
    }

    #[test]
    fn an_entry_without_a_domain_is_skipped_rather_than_guessed() {
        let body = r#"[{"entry_id":"a"},{"entry_id":"b","domain":"caldav"}]"#;
        assert_eq!(configured_integrations(body), vec!["caldav"]);
    }

    #[test]
    fn anything_unreadable_is_no_integrations_rather_than_an_error() {
        // This only ever decorates a screen. A service that answers oddly should cost the
        // person a label, never the ability to connect something.
        for body in ["", "not json", "{}", "[]", r#"{"error":"nope"}"#] {
            assert!(
                configured_integrations(body).is_empty(),
                "expected nothing from {body:?}"
            );
        }
    }
}

#[cfg(test)]
mod a_form_the_service_asked_for {
    use super::read_form;
    use crate::domain::SetupField;

    /// The real first step of Home Assistant's CalDAV flow, captured from a live house.
    const CALDAV_STEP: &str = r#"{
        "type": "form", "flow_id": "01KYTTT477W5ZGQKJSGD2X29VF", "handler": "caldav",
        "step_id": "user", "errors": {},
        "data_schema": [
            { "name": "url", "type": "string", "required": true },
            { "name": "username", "type": "string", "required": true },
            { "name": "password", "type": "string", "required": false, "default": "" },
            { "name": "verify_ssl", "type": "boolean", "required": false, "default": true }
        ]
    }"#;

    #[test]
    fn the_form_is_read_from_what_the_service_declares() {
        // Endora knows nothing about calendars. This is the whole mechanism: a kind of
        // thing nobody here has heard of works exactly like one that ships today.
        let form = read_form(CALDAV_STEP).unwrap().expect("a form");
        assert_eq!(form.step, "user");
        let names: Vec<&str> = form.fields.iter().map(|f| f.name.as_str()).collect();
        assert_eq!(names, vec!["url", "username", "password", "verify_ssl"]);
        assert!(form.fields[0].required);
        assert!(!form.fields[3].required);
    }

    #[test]
    fn a_credential_is_recognised_so_it_is_never_shown() {
        // A form Endora did not design can call a secret anything, so this is a heuristic —
        // and it fails safe: a field wrongly treated as secret is masked and still submitted
        // correctly, while the reverse puts somebody's password on a screen.
        let form = read_form(CALDAV_STEP).unwrap().expect("a form");
        let password = form.fields.iter().find(|f| f.name == "password").unwrap();
        assert!(password.secret, "a password must never be echoed back");
        assert!(!form.fields[0].secret, "a url is not a credential");

        for named in [
            "password",
            "api_key",
            "APIKey",
            "access_token",
            "client_secret",
        ] {
            assert!(SetupField::looks_secret(named), "{named}");
        }
        for named in ["url", "username", "host", "port", "calendar_name"] {
            assert!(!SetupField::looks_secret(named), "{named}");
        }
    }

    #[test]
    fn a_finished_flow_is_not_another_question() {
        // A created entry ends the conversation; anything else would loop the person
        // through a form they already answered.
        let made = r#"{ "type": "create_entry", "title": "iCloud",
                        "result": { "entry_id": "abc" } }"#;
        assert_eq!(read_form(made).unwrap(), None);
    }

    #[test]
    fn what_the_service_refused_is_said_in_its_own_words() {
        // "wrong password" is the entire reason somebody is looking at this screen, so a
        // per-field complaint is surfaced rather than swallowed into "it didn't work".
        let refused = r#"{ "type": "form", "flow_id": "1", "step_id": "user",
                           "errors": { "base": "invalid_auth" }, "data_schema": [] }"#;
        let why = read_form(refused).unwrap_err();
        assert!(why.contains("invalid_auth"), "{why}");

        let aborted = r#"{ "type": "abort", "reason": "already_configured" }"#;
        assert!(
            read_form(aborted)
                .unwrap_err()
                .contains("already_configured")
        );
    }
}
