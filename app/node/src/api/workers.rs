use super::*;

/// Endora's own recent work, as a capability the butler can reach for (ADR 0056).
///
/// Asked "did you do anything while I was out?", the butler answered with the state of some
/// lights and then said "No specific activities were recorded today" — four hours after
/// posting a real morning brief. Every part of the true answer was stored; nothing could
/// reach it from a turn.
///
/// A skill rather than something appended to every turn's context. A digest injected into
/// every conversation costs context on every conversation, which is precisely the failure
/// that made a clock reading arrive with five kilobytes of house attached
/// ([0053](../../docs/adr/0053-honesty-about-what-it-did.md)). This is asked for when it is
/// wanted.
pub(super) struct WhatIHaveBeenDoing {
    pub(super) chat: Arc<endora_conversation::ChatStore>,
    pub(super) understanding: Arc<endora_understanding::UnderstandingStore>,
    pub(super) config: Arc<endora_capabilities::ConfigStore>,
    pub(super) clock: Arc<dyn endora_application::Clock + Send + Sync>,
}

impl endora_capabilities::Capability for WhatIHaveBeenDoing {
    fn for_people(&self) -> Option<&'static str> {
        Some(
            "What Endora has done lately: messages it started, what it did and whether it \
             worked, and settings it changed. Ask it what it did while you were out.",
        )
    }

    fn info(&self) -> endora_capabilities::CapabilityInfo {
        endora_capabilities::CapabilityInfo {
            id: "own_activity",
            name: "What I have been doing",
            description: "What Endora itself has done recently — messages it started, \
                          actions it took and whether they changed anything, settings it \
                          changed, and things it noticed stop answering. Use this for any \
                          question about what YOU did or whether anything happened, \
                          including \"what did you do while I was out\" and \"has anything \
                          happened today\".",
            category: "endora",
            // Its own database. Nothing leaves the house to answer this.
            reaches_external: false,
            reversibility: endora_application::Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            wants_place: false,
            borrows_from: None,
            third_party: false,
            input_schema: None,
        }
    }

    fn invoke(
        &self,
        input: &serde_json::Value,
        _settings: &endora_capabilities::CapabilitySettings,
    ) -> Result<serde_json::Value, endora_capabilities::CapabilityError> {
        let now_ms = self.clock.now().unix_millis();
        // A window may be asked for; a day is what "while I was out" means by default.
        let hours = input["hours"].as_i64().filter(|h| *h > 0).unwrap_or(24);
        let since_ms = now_ms - hours.saturating_mul(3_600_000);
        let did = usecases::what_it_has_been_doing(
            self.chat.as_ref(),
            self.understanding.as_ref(),
            self.config.as_ref(),
            since_ms,
            now_ms,
        )
        .map_err(|e| endora_capabilities::CapabilityError::Unavailable(e.to_string()))?;
        Ok(json!({ "hours": hours, "did": did }))
    }

    fn summarize(&self, output: &serde_json::Value) -> String {
        // The digest is already written for a person to read; handing the model raw JSON
        // is what a small local model relays worst.
        output["did"].as_str().unwrap_or_default().to_owned()
    }
}

/// One page of the brain, read whole (ADR 0101). Every turn carries memory and a line per
/// page; this is how the butler gets a page's detail when the conversation needs it.
pub(super) struct ReadAPage {
    pub(super) knowledge: Arc<endora_understanding::BrainFolder>,
}

impl endora_capabilities::Capability for ReadAPage {
    fn for_people(&self) -> Option<&'static str> {
        Some(
            "Reads one page of what Endora knows about you, when a conversation needs its \
             detail. Pages are in About you.",
        )
    }

    fn info(&self) -> endora_capabilities::CapabilityInfo {
        endora_capabilities::CapabilityInfo {
            id: "read_page",
            name: "Read a page of your brain",
            description: "Read one page of what you know about the person (your brain), by \
                          its name from the list of pages, such as hobbies/watches. Use it \
                          when the conversation needs a page's detail.",
            category: "endora",
            // Its own folder. Nothing leaves the house to answer this.
            reaches_external: false,
            reversibility: endora_application::Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            wants_place: false,
            borrows_from: None,
            // The person's own pages, not a stranger's words.
            third_party: false,
            input_schema: Some(
                r#"{"type":"object","properties":{"page":{"type":"string","description":"the page's name, such as hobbies/watches"}},"required":["page"]}"#,
            ),
        }
    }

    fn invoke(
        &self,
        input: &serde_json::Value,
        _settings: &endora_capabilities::CapabilitySettings,
    ) -> Result<serde_json::Value, endora_capabilities::CapabilityError> {
        let name = input["page"].as_str().unwrap_or_default();
        let text = usecases::read_page(self.knowledge.as_ref(), name)
            .map_err(|e| endora_capabilities::CapabilityError::Unavailable(e.to_string()))?;
        Ok(json!({ "page": name, "text": text }))
    }

    fn summarize(&self, output: &serde_json::Value) -> String {
        output["text"].as_str().unwrap_or_default().to_owned()
    }
}

/// The person's brief, as a skill the model can reach for (ADR 0074).
///
/// Defined HERE because assembling it crosses contexts — capabilities, standing
/// trouble, scheduling's idea of a brief — and the composition root is the only
/// place allowed to know all of them (ADR 0050). The assembly itself is
/// `usecases::assembled_brief_facts`; this is only the wiring, exactly as
/// [`WhatIHaveBeenDoing`] wires the activity digest.
///
/// Asked at 5 PM it reads the world at 5 PM: every section is gathered fresh at
/// call time, and the weather section carries its own "as of" clock.
pub(super) struct TheWholeBrief {
    pub(super) chat: Arc<endora_conversation::ChatStore>,
    pub(super) understanding: Arc<endora_understanding::UnderstandingStore>,
    pub(super) knowledge: Arc<endora_understanding::BrainFolder>,
    pub(super) config: Arc<endora_capabilities::ConfigStore>,
    pub(super) clock: Arc<SystemClock>,
    /// The catalogue this skill gathers FROM — built without this skill in it, so
    /// the brief can never recurse into itself by construction.
    pub(super) base: Arc<Vec<Arc<dyn endora_capabilities::Capability>>>,
    pub(super) mcp: Arc<std::sync::RwLock<Arc<endora_capabilities::McpRunner>>>,
}

impl endora_capabilities::Capability for TheWholeBrief {
    fn for_people(&self) -> Option<&'static str> {
        Some(
            "Your brief in one go: the weather, the drive, headlines, today's calendar, local \
             events, and anything in the house not answering. Ask for your brief.",
        )
    }

    fn info(&self) -> endora_capabilities::CapabilityInfo {
        endora_capabilities::CapabilityInfo {
            id: "brief",
            name: "Your brief",
            description: "The person's whole brief, assembled from their own skills — \
                          today's weather and whether they need an umbrella or jacket, \
                          the drive, the top headlines, today's calendar, local events, \
                          and anything in the house not answering. Use it whenever they \
                          ask for their brief, rundown, or morning/afternoon/evening \
                          update — it is ONE call that gathers everything.",
            category: "endora",
            reaches_external: true,
            reversibility: endora_application::Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            wants_place: false,
            borrows_from: None,
            // Headlines are other people's words; a brief that carries them makes
            // the turn that read it unable to act on its own (ADR 0064).
            third_party: true,
            input_schema: None,
        }
    }

    fn invoke(
        &self,
        _input: &serde_json::Value,
        _settings: &endora_capabilities::CapabilitySettings,
    ) -> Result<serde_json::Value, endora_capabilities::CapabilityError> {
        let unavailable = |e: String| endora_capabilities::CapabilityError::Unavailable(e);
        let mcp = self
            .mcp
            .read()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        // Reversible-only for the same reason the heartbeat's is: a brief gathers,
        // it never actuates — enforced, not documented.
        let runner = build_reversible_only_runner(
            self.config.as_ref(),
            self.base.clone(),
            mcp,
            proven_now(self.understanding.as_ref()),
        );
        let context = usecases::butler_context(
            self.knowledge.as_ref(),
            self.knowledge.as_ref(),
            self.understanding.as_ref(),
            self.config.as_ref(),
            self.chat.as_ref(),
            &runner,
            self.config.as_ref(),
            self.clock.as_ref(),
        )
        .map_err(|e| unavailable(e.to_string()))?;
        let mut activity = Vec::new();
        let facts = usecases::assembled_brief_facts(&runner, &context, &mut activity);
        Ok(json!({
            "brief": facts.unwrap_or_else(|| {
                "There's nothing to brief yet — set a home location and enable some \
                 skills, and this fills in."
                    .to_owned()
            }),
        }))
    }

    fn summarize(&self, output: &serde_json::Value) -> String {
        // Already written for a person; raw JSON is what a small model relays worst.
        output["brief"].as_str().unwrap_or_default().to_owned()
    }
}

/// Keeps the tool servers usable for the team's bridge (ADR 0108): every second minute an
/// enabled server that came up with no tools is tried again, and a tool that never works is
/// withdrawn. Observed after a reboot: every container started at once, Endora reached the Home
/// Assistant server before it was listening, got an empty catalogue, and never retried.
/// `connect_mcp` runs once at startup, which is exactly wrong on a machine where everything
/// boots together. What the butler's heartbeat also did (the daily brief, watching the house for
/// the chat) is retired with the chat (ADR 0136).
pub fn spawn_tool_server_upkeep(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(120));
        // The first tick is at once: the servers have just been connected.
        ticker.tick().await;
        loop {
            ticker.tick().await;
            reconnect_empty_mcp_servers(&state);
            withdraw_what_never_works(&state);
        }
    });
}

/// What the brief adds after its wording: the mail section, then what the agenda is keeping an
/// eye on (ADR 0103).
pub(super) fn brief_lines(state: &AppState) -> Vec<String> {
    let mut lines = mail_brief_lines(state);
    lines.extend(super::agenda::brief_lines(state));
    lines
}

/// The mail section for the morning brief, or nothing.
pub(super) fn mail_brief_lines(state: &AppState) -> Vec<String> {
    usecases::mail_for_the_brief(
        state.mail_store.as_ref(),
        Some(state.calendar.as_ref()),
        state.clock.as_ref(),
    )
    .unwrap_or_default()
}

/// The owner's calendar, through the house's own Home Assistant connection: whatever calendars
/// it is connected to, read-only, with no credential of its own. Nothing when Home Assistant is
/// not set up.
pub(crate) struct HouseCalendar(pub(crate) Arc<endora_capabilities::ConfigStore>);

impl endora_application::Calendar for HouseCalendar {
    fn entries(&self, from_ms: i64, to_ms: i64) -> Result<Vec<(String, String)>, String> {
        let settings = super::settings_map(self.0.as_ref());
        let home = settings
            .get("home_assistant")
            .and_then(endora_capabilities::HomeAssistant::from_settings)
            .ok_or_else(|| "Home Assistant is not set up".to_owned())?;
        home.calendar_entries(from_ms, to_ms)
    }
}
