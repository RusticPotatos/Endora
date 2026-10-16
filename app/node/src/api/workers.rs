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

/// Sends one line to whatever the person nominated as how to reach them (ADR 0056).
///
/// Best-effort and silent on failure: a notification that cannot be delivered must never
/// break the turn that produced it, the same rule verification has followed since ADR 0053.
///
/// Only the first sentence travels. The whole point is to say *there is something*, and a
/// notification long enough to be the message itself is one the person stops reading.
pub(super) fn reach_out(
    config: &endora_capabilities::ConfigStore,
    message: &endora_application::ChatMessage,
) {
    const ENOUGH_TO_KNOW_IT_MATTERS: usize = 140;
    let text = message.text().trim();
    let line: String = text.chars().take(ENOUGH_TO_KNOW_IT_MATTERS).collect();
    let body = if text.chars().count() > ENOUGH_TO_KNOW_IT_MATTERS {
        format!("{line}…")
    } else {
        line
    };
    for (server, channel) in native_channels(config) {
        match channel.notify("Endora", &body) {
            Some(Ok(())) => {}
            Some(Err(why)) => eprintln!("reaching out via {server}: {why}"),
            None => {} // nothing nominated here, which is the default
        }
    }
}

/// One pass over everything that can say what it currently knows: the native channels (Home
/// Assistant and the like), then anything else the runner can read. What moved is kept in the
/// transition log, which is what lets a chat turn say "the back door opened ten minutes ago"
/// (ADR 0058). A service that cannot be reached this tick is skipped.
pub(super) fn watch_the_world(state: &AppState) {
    let config = state.config.clone();
    // Every thing a native channel reported, changed or not. Filtering the second pass by
    // what *changed* was not enough and made it subtler: the two paths keep separate stored
    // priors, so suppressing the duplicate stopped `skills::` updating its own prior and it
    // simply fired one pass later with a stale value. The house is watched once because the
    // second pass never sees its entities at all.
    let mut the_house_reported: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let now_ms = state.clock.now().unix_millis();
    for (server, channel) in native_channels(config.as_ref()) {
        match channel.states() {
            Ok(reading) => {
                the_house_reported.extend(reading.iter().map(|(thing, _)| thing.clone()));
                // What moved, so the turn can say "the back door opened ten minutes ago"
                // (ADR 0058).
                match endora_capabilities::watch_for_change(
                    config.as_ref(),
                    &server,
                    &reading,
                    now_ms,
                ) {
                    // Kept for the chat's "what changed lately", not written to Activity:
                    // Activity records what Endora did, and a sensor tick is not that.
                    Ok(_) => {}
                    Err(e) => eprintln!("watching {server}: could not record what moved: {e}"),
                }
            }
            Err(e) => eprintln!("watching {server}: could not read it this time: {e}"),
        }
    }

    // And everything else that can say what it currently knows.
    //
    // The loop above only ever saw native channels, so an MCP server publishing resources fed
    // notions but was never *watched* — nothing noticed when it changed, and nothing raised it
    // when it stopped answering. Adding an integration meant wiring it into this function by
    // hand, which is the thing that does not scale.
    //
    // Now anything implementing `current_states` is watched by virtue of implementing it.
    // Keys already carry their source, so `watch_for_change` can namespace as it always has.
    let runner = build_runner(
        config.as_ref(),
        state.capabilities.clone(),
        state
            .mcp
            .read()
            .map_or_else(|p| p.into_inner().clone(), |g| g.clone()),
        proven_now(state.understanding.as_ref()),
    );
    let reading = endora_capabilities::CapabilityRunner::current_states(&runner);
    // Minus whatever a native channel already reported this pass.
    //
    // A service that is BOTH a native channel and reachable through the runner was watched
    // twice — the same entity recorded under `home-assistant::` and again under `skills::`,
    // so every change became two transitions, two events, and two candidates to wake on.
    // ADR 0059's promise is one fact source with many consumers, not one fact counted
    // twice; the thing-part of the key is what identifies it, whichever route it arrived by.
    let reading: Vec<(String, String)> = reading
        .into_iter()
        .filter(|(key, _)| {
            let thing = key.split_once("::").map_or(key.as_str(), |(_, t)| t);
            !the_house_reported.contains(thing)
        })
        .collect();
    if !reading.is_empty() {
        match endora_capabilities::watch_for_change(config.as_ref(), "skills", &reading, now_ms) {
            Ok(_) => {}
            Err(e) => eprintln!("watching skills: could not record what moved: {e}"),
        }
    }
}

/// Spawns the butler's **heartbeat**: a background loop that keeps its connections and its
/// watch on the house current and prepares the daily brief when it is due. Only messages,
/// nothing consequential, so it stays on the safe side of the autonomy model (ADR 0051). The
/// blocking store work runs on a worker thread.
pub fn spawn_heartbeat(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(30));
        let mut ticks: u64 = 0;
        loop {
            ticker.tick().await;
            ticks += 1;
            // An enabled server that came up with NO tools connected to nothing, so try
            // again. Observed after a reboot: every container started at once, Endora
            // reached the Home Assistant server before it was listening, got an empty
            // catalogue, and never retried — so for twenty minutes the butler had no way
            // to act and narrated "turning on the kitchen light" with nothing behind it.
            //
            // `connect_mcp` runs once at startup, which is exactly wrong on a machine
            // where everything boots together. Every second minute is often enough to
            // ride out a boot race and cheap enough to leave running for a server that
            // is genuinely gone.
            if ticks.is_multiple_of(4) {
                reconnect_empty_mcp_servers(&state);
                withdraw_what_never_works(&state);
                watch_the_world(&state);
            }
            // Explicit commissioned work has the next model slot. Sensing and
            // housekeeping above remain live, while schedule reads below are deferred
            // so a skipped heartbeat does not consume a due cadence (ADR 0077).
            if state
                .understanding
                .active_research()
                .ok()
                .flatten()
                .is_some()
            {
                continue;
            }
            let events = state.events.clone();
            let chat = state.chat.clone();
            let schedules = state.schedules.clone();
            let understanding = state.understanding.clone();
            let knowledge = state.knowledge.clone();
            let config = state.config.clone();
            let ids = state.ids.clone();
            let clock = state.clock.clone();
            let capabilities = state.capabilities.clone();
            let butler = state.butler.clone();
            let mcp = mcp_snapshot(&state);
            // Take the turn lock so the brief never interleaves with a chat reply; held
            // across the blocking work below.
            let offset_now = state
                .time_zone
                .offset_minutes_at(state.clock.now().unix_millis());
            let mail_lines = brief_lines(&state);
            let _turn = state.turn_lock.clone().lock_owned().await;
            let posted = tokio::task::spawn_blocking(move || {
                // Unattended: nobody is here to confirm anything, so only the reversible
                // bands may act — regardless of what the person opened for chat turns.
                let runner = build_reversible_only_runner(
                    config.as_ref(),
                    capabilities,
                    mcp,
                    proven_now(understanding.as_ref()),
                );
                let context = usecases::butler_context(
                    knowledge.as_ref(),
                    knowledge.as_ref(),
                    understanding.as_ref(),
                    config.as_ref(),
                    chat.as_ref(),
                    &runner,
                    config.as_ref(),
                    clock.as_ref(),
                )?;
                // No check-in: Endora does not write to the person just to check in (owner,
                // 2026-10-16). The agenda loop will speak when it has something to say.
                // A clock change moves the brief with it, before it is checked for being due.
                if usecases::keep_hours_on_the_clock(
                    schedules.as_ref(),
                    config.as_ref(),
                    offset_now,
                )? {
                    record_event(
                        events.as_ref(),
                        clock.as_ref(),
                        "The clocks changed: kept the brief at the same hour",
                    );
                }
                // A daily brief, if one is due (reversible skills only).
                let briefed = usecases::run_due_brief(
                    chat.as_ref(),
                    schedules.as_ref(),
                    &runner,
                    butler.as_ref(),
                    ids.as_ref(),
                    clock.as_ref(),
                    &context,
                    &mail_lines,
                )?;
                if let Some((_, activity)) = &briefed {
                    for item in activity {
                        record_event(events.as_ref(), clock.as_ref(), item);
                    }
                    record_event(events.as_ref(), clock.as_ref(), "Prepared your daily brief");
                }
                // No nightly loop: what it produced (beliefs, notions, intentions) was noise
                // (owner, 2026-10-16).
                // Reach the person, if they nominated a way to be reached (ADR 0056).
                //
                // Every one of these is a message they did not ask for, which is exactly
                // what a notification is for — and the rate limit is already the schedule
                // they set, so this cannot become a firehose without them widening it.
                //
                // Deliberately NOT gated on presence. That would mean parsing free text a
                // service wrote ("john is not home") to decide whether to interrupt, and
                // a wrong guess either wakes someone or silently drops the alert they
                // wanted. The schedule is the honest limit.
                if let Some(message) = briefed.as_ref() {
                    reach_out(config.as_ref(), &message.0);
                }
                Ok::<_, AppError>(briefed.is_some())
            })
            .await;
            if let Ok(Ok(true)) = posted {
                let _ = state.changes.send(());
            }

            // The nightly model self-tune is off until a model comparison run on purpose
            // replaces it (owner, 2026-10-16).
        }
    });
}

/// Advances the one explicit research assignment without holding the model for
/// longer than one bounded turn. SQLite state makes the work survive restarts.
pub fn spawn_research_worker(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let Ok(Some(work)) = state.understanding.active_research() else {
                continue;
            };
            let work_id = work.intention_id();
            let lease_key = format!("research:{}", work.request_key());
            // Take the turn lock BEFORE spending an attempt. Chat holds it for a whole
            // turn, so reserving first burned one of the 16 attempts every 5 seconds the
            // person was talking, until the assignment could never run again.
            let Ok(_turn) = state.turn_lock.clone().try_lock_owned() else {
                continue;
            };
            let reserved_at = state.clock.now();
            if !state
                .store
                .reserve_execution_attempt(&lease_key, reserved_at, 16)
                .unwrap_or(false)
            {
                continue;
            }
            let lease_until_ms = reserved_at.unix_millis() + endora_application::EXECUTION_LEASE_MS;
            let understanding = state.understanding.clone();
            let understanding_after = state.understanding.clone();
            let knowledge = state.knowledge.clone();
            let delivery = state.store.clone();
            let chat = state.chat.clone();
            let config = state.config.clone();
            let audit = state.audit.clone();
            let ids = state.ids.clone();
            let clock = state.clock.clone();
            let butler = state.butler.clone();
            let capabilities = state.capabilities.clone();
            let mcp = mcp_snapshot(&state);
            let result = tokio::task::spawn_blocking(move || {
                let runner = build_reversible_only_runner(
                    config.as_ref(),
                    capabilities,
                    mcp,
                    proven_now(understanding.as_ref()),
                );
                let context = usecases::butler_context(
                    knowledge.as_ref(),
                    knowledge.as_ref(),
                    understanding.as_ref(),
                    config.as_ref(),
                    chat.as_ref(),
                    &runner,
                    config.as_ref(),
                    clock.as_ref(),
                )?;
                endora_application::conduct_research(
                    understanding.as_ref(),
                    delivery.as_ref(),
                    understanding.as_ref(),
                    &runner,
                    butler.as_ref(),
                    audit.as_ref(),
                    ids.as_ref(),
                    clock.as_ref(),
                    &context,
                    work,
                )
            })
            .await;
            if !matches!(result, Ok(Ok(_))) {
                if let Ok(Some(mut active)) = understanding_after.research(work_id) {
                    active.retry(
                        "temporary_failure",
                        "A research step failed and will be retried within the assignment budget.",
                        state.clock.now(),
                    );
                    let _ = understanding_after.save_research(&active);
                }
            }
            if let Ok(Some(terminal)) = understanding_after.research(work_id) {
                if terminal.status().terminal() && terminal.status() != ResearchStatus::Delivered {
                    if let Ok(Some(mut intention)) =
                        understanding_after.get(terminal.intention_id())
                    {
                        intention.abandon();
                        let _ = understanding_after.save(&intention);
                    }
                }
            }
            let _ = state
                .store
                .release_execution_attempt(&lease_key, lease_until_ms);
            let _ = state.changes.send(());
        }
    });
}

/// Reads the mail, one message at a time, in quiet time (ADR 0097). Indexes what has arrived
/// (headers only, no model) on every tick; reads one message only when the person has been
/// quiet and no turn is running, holding the turn lock for that one message so a chat turn
/// waits a few seconds at most. What a message means for the owner is the agenda loop's to say
/// (ADR 0103). Run only when the node has the mail and the lane is on.
pub fn spawn_mail_reader(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(20));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            // After a message is read, the next is tried at once: the backlog is read as fast as
            // quiet time allows. The turn lock is let go between messages, so a chat turn waits
            // one message at most. When there is nothing to read, or it is not the time, wait.
            if read_mail_once(&state).await.is_none() {
                ticker.tick().await;
            }
        }
    });
}

/// Folds the brain's pages in quiet time (ADR 0101): every ten minutes, while the person is
/// quiet and no turn is running, the local model writes one grown page's summary again and,
/// after it, what Endora knows best. Each fold is a line in Activity.
pub fn spawn_brain_folding(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(10 * 60));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let _ = fold_once(&state).await;
        }
    });
}

/// One tick of [`spawn_brain_folding`]. Returns the page it folded, if any.
pub(super) async fn fold_once(state: &AppState) -> Option<endora_application::PagePath> {
    let now = state.clock.now();
    if !quiet_enough(state, now) {
        return None;
    }
    let Ok(turn) = state.turn_lock.clone().try_lock_owned() else {
        return None;
    };
    let knowledge = state.knowledge.clone();
    let writer = state.local_writer.clone();
    let clock = state.clock.clone();
    let folded = tokio::task::spawn_blocking(move || {
        let _turn = turn;
        usecases::fold_one_page(
            knowledge.as_ref(),
            writer.as_ref(),
            &usecases::today(clock.as_ref()),
        )
    })
    .await
    .ok()?;
    let page = match folded {
        Ok(Some(page)) => page,
        Ok(None) => return None,
        Err(error) => {
            eprintln!("brain: could not fold a page ({error})");
            return None;
        }
    };
    let _ = endora_application::EventLog::record(
        state.events.as_ref(),
        now,
        &format!("Wrote the summary of {page} again, from what it has noted"),
    );
    let _ = state.changes.send(());
    Some(page)
}

/// One tick of [`spawn_mail_reader`]. Returns what it read, if it read anything.
pub(super) async fn read_mail_once(state: &AppState) -> Option<endora_mail::Digest> {
    let (Some(copy), Some(reader)) = (state.mail_copy.clone(), state.mail_reader.clone()) else {
        return None;
    };
    let store = state.mail_store.clone();
    let indexing = (copy.clone(), store.clone());
    let _ = tokio::task::spawn_blocking(move || usecases::index_new_mail(&indexing.0, &indexing.1))
        .await;
    let now = state.clock.now();
    if !quiet_enough(state, now) {
        return None;
    }
    let Ok(turn) = state.turn_lock.clone().try_lock_owned() else {
        return None;
    };
    let clock = state.clock.clone();
    let calendar = state.calendar.clone();
    let read = tokio::task::spawn_blocking(move || {
        let _turn = turn;
        usecases::read_one(
            &copy,
            &store,
            reader.as_ref(),
            Some(calendar.as_ref()),
            clock.as_ref(),
        )
    })
    .await
    .ok()?;
    let digest = match read {
        Ok(Some(digest)) => digest,
        Ok(None) => return None,
        Err(error) => {
            // The model was unreachable: the message stays unread and is tried again. Said
            // without the message, which is not in the error.
            eprintln!("mail: not read this time ({error})");
            return None;
        }
    };
    // What it means for the owner is the agenda's to say (ADR 0103): an item, and its line in
    // Activity, on the loop's next look.
    let _ = state.changes.send(());
    Some(digest)
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

/// Whether the person has been quiet long enough for background work (reading mail) to
/// take the model without competing with them.
fn quiet_enough(state: &AppState, now: endora_application::Timestamp) -> bool {
    // Upper bound open: a message dated after `now` is found too, and the rule refuses it.
    let from = now
        .unix_millis()
        .saturating_sub(endora_application::quiet::QUIET_WINDOW_MS);
    let recent = endora_conversation::ChatRepository::between(state.chat.as_ref(), from, i64::MAX);
    quiet_given(now, recent)
}

/// The decision, given what the chat said. A chat that cannot be read counts as not quiet: the
/// lane waits when it cannot tell, because waiting costs nothing.
pub(super) fn quiet_given(
    now: endora_application::Timestamp,
    recent: Result<Vec<endora_application::ChatMessage>, endora_application::RepositoryError>,
) -> bool {
    let Ok(recent) = recent else {
        return false;
    };
    let last_spoke = recent
        .iter()
        .filter(|message| message.role() == endora_application::MessageRole::User)
        .map(|message| message.at().unix_millis())
        .max()
        .map(endora_application::Timestamp::from_unix_millis);
    endora_application::quiet::quiet_enough(now, last_spoke)
}
