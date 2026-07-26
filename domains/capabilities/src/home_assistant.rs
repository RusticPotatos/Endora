//! Direct reach into Home Assistant — the named per-integration adapter ADR 0038 called
//! for, and the exact reach ADR 0042 grants.
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
}

/// A configured connection to a Home Assistant instance.
pub struct HomeAssistant {
    base: String,
    token: String,
}

impl HomeAssistant {
    /// Builds a connection from the URL and long-lived token the person stored against
    /// the Home Assistant skill. `None` when either is missing — the honest default, and
    /// the whole feature simply stays off.
    #[must_use]
    pub fn from_settings(settings: &crate::infrastructure::CapabilitySettings) -> Option<Self> {
        let base = settings.get("url")?.trim().trim_end_matches('/').to_owned();
        let token = settings.get("token")?.trim().to_owned();
        (!base.is_empty() && !token.is_empty()).then_some(Self { base, token })
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
                Some(Entity {
                    id: id.to_owned(),
                    name: e["attributes"]["friendly_name"]
                        .as_str()
                        .unwrap_or(id)
                        .to_owned(),
                    state: e["state"].as_str().unwrap_or("?").to_owned(),
                })
            })
            .collect();
        Ok(entities)
    }

    /// Calls a service on exactly one entity.
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
        self.post(&path, &json!({ "entity_id": entity }))
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

/// The MCP server this instance is the direct counterpart of.
///
/// Data rather than a constant in the wiring: whoever registers the MCP server chooses
/// its name, and a name hardcoded in Endora would be exactly the per-integration guessing
/// ADR 0038 rules out. Defaults to the conventional name so nobody has to fill it in.
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
            .map(|e| (e.id, e.name))
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
        Ok(self
            .entities()?
            .into_iter()
            .map(|e| format!("names: {}\n  state: {}", e.name, e.state))
            .collect::<Vec<_>>()
            .join("\n"))
    }

    fn act(&self, tool: &str, entity: &str) -> Option<Result<String, String>> {
        let (domain, service) = service_for(tool)?;
        Some(self.call_service(domain, service, entity))
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
}
