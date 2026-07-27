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
    /// What sort of thing it is: its domain (`light`) and its device class where it has
    /// one. The vocabulary of *kinds*, as opposed to names (ADR 0054).
    pub kinds: Vec<String>,
}

/// A configured connection to a Home Assistant instance.
pub struct HomeAssistant {
    base: String,
    token: String,
    /// Whether the person has allowed Endora to write names back (ADR 0054). Seeing and
    /// acting are one grant; editing the service's own configuration is another, and it
    /// is off until deliberately turned on.
    may_write: bool,
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
        let base = settings.get("url")?.trim().trim_end_matches('/').to_owned();
        let token = settings.get("token")?.trim().to_owned();
        let may_write = settings
            .get("write_names")
            .map(|v| v.trim().to_lowercase())
            .is_some_and(|v| ["on", "yes", "true", "1"].contains(&v.as_str()));
        (!base.is_empty() && !token.is_empty()).then_some(Self {
            base,
            token,
            may_write,
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
        let states: Value = serde_json::from_str(&body).map_err(|e| e.to_string())?;
        let entities = states
            .as_array()
            .ok_or("Home Assistant did not return a list of states")?
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
                    kinds,
                })
            })
            .collect();
        Ok(entities)
    }

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

/// One person's presence, in words.
fn describe_presence(name: &str, state: &str) -> String {
    match state {
        "home" => format!("{name} is home"),
        "not_home" | "unknown" | "unavailable" => format!("{name} is not home"),
        place => format!("{name} is at {place}"),
    }
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
                    .map(|n| format!("names: {n}\n  state: {}", e.state))
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

    fn about_the_person(&self) -> Option<String> {
        // A house knows whether someone is in it. Home Assistant keeps that as `person.*`
        // entities whose state is `home`, `not_home`, or the name of a place — which is
        // already in the reading Endora fetches, and was going unused.
        let people: Vec<String> = self
            .entities()
            .ok()?
            .into_iter()
            .filter(|e| e.id.starts_with("person."))
            .map(|e| describe_presence(&e.name, &e.state))
            .collect();
        (!people.is_empty()).then(|| people.join("; "))
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
            }
        }))
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
                }),
        )
    }

    fn undo(&self, write: &crate::domain::ConfigWrite) -> Option<Result<String, String>> {
        if !self.may_write {
            return None;
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
            kinds: vec!["light".to_owned()],
        }
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
        assert_eq!(describe_presence("rustic", "home"), "rustic is home");
        assert_eq!(
            describe_presence("rustic", "not_home"),
            "rustic is not home"
        );
        // A named zone is a place, and reads better as one than as "not_home".
        assert_eq!(describe_presence("rustic", "Office"), "rustic is at Office");
        // A tracker that has lost the person says so plainly rather than inventing a place.
        assert_eq!(
            describe_presence("rustic", "unavailable"),
            "rustic is not home"
        );
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
