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
    pub(super) config: Arc<endora_capabilities::ConfigStore>,
    pub(super) clock: Arc<SystemClock>,
    /// The catalogue this skill gathers FROM — built without this skill in it, so
    /// the brief can never recurse into itself by construction.
    pub(super) base: Arc<Vec<Arc<dyn endora_capabilities::Capability>>>,
    pub(super) mcp: Arc<std::sync::RwLock<Arc<endora_capabilities::McpRunner>>>,
}

impl endora_capabilities::Capability for TheWholeBrief {
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
            self.understanding.as_ref(),
            self.understanding.as_ref(),
            self.understanding.as_ref(),
            self.config.as_ref(),
            self.chat.as_ref(),
            &runner,
            self.config.as_ref(),
            self.clock.as_ref(),
        )
        .map_err(|e| unavailable(e.to_string()))?;
        let mut activity = Vec::new();
        let facts = usecases::assembled_brief_facts(
            &runner,
            self.config.as_ref(),
            &context,
            self.clock.now().unix_millis(),
            &mut activity,
        );
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

/// Notes what is not answering in each service Endora has direct reach into (ADR 0056).
///
/// The whole reason this runs at all: without it there is no *since when*, and without a
/// duration there is no problem statement — only a status line saying thirteen things are
/// unavailable, which is a chore rather than a service.
///
/// Reads through the channel rather than the tool surface, because a hidden or broken
/// entity is exactly what a tool surface leaves out ([0054](../../docs/adr/0054-other-peoples-services.md)),
/// and those are the ones this is about. A service that cannot be reached this tick is
/// skipped rather than treated as everything being fine — inventing recovery from a failed
/// read would clear every clock in the house on one bad network moment.
pub(super) fn watch_the_world(state: &AppState) {
    let config = state.config.clone();
    // Everything this pass records, for the wake decision at the end (ADR 0063).
    let mut this_pass: Vec<endora_capabilities::Transition> = Vec::new();
    // Every thing a native channel reported, changed or not. Filtering the second pass by
    // what *changed* was not enough and made it subtler: the two paths keep separate stored
    // priors, so suppressing the duplicate stopped `skills::` updating its own prior and it
    // simply fired one pass later with a stale value. The house is watched once because the
    // second pass never sees its entities at all.
    let mut the_house_reported: std::collections::HashSet<String> =
        std::collections::HashSet::new();
    let events = state.events.clone();
    let clock = state.clock.clone();
    let now_ms = state.clock.now().unix_millis();
    for (server, channel) in native_channels(config.as_ref()) {
        match channel.states() {
            Ok(reading) => {
                the_house_reported.extend(reading.iter().map(|(thing, _)| thing.clone()));
                if let Err(e) = endora_capabilities::watch_for_trouble(
                    config.as_ref(),
                    &server,
                    &reading,
                    now_ms,
                ) {
                    eprintln!("watching {server}: could not record what is wrong: {e}");
                }
                // The same reading, asked a second question. `watch_for_trouble` only ever
                // asks "is this answering?", so a house could change all day and Endora would
                // carry nothing out of it (ADR 0058).
                match endora_capabilities::watch_for_change(
                    config.as_ref(),
                    &server,
                    &reading,
                    now_ms,
                ) {
                    Ok(moved) => {
                        for change in moved {
                            record_event(
                                events.as_ref(),
                                clock.as_ref(),
                                &format!(
                                    "{} went from {} to {}",
                                    change.key, change.from, change.to
                                ),
                            );
                            this_pass.push(change);
                        }
                    }
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
        if let Err(e) =
            endora_capabilities::watch_for_trouble(config.as_ref(), "skills", &reading, now_ms)
        {
            eprintln!("watching skills: could not record what is wrong: {e}");
        }
        match endora_capabilities::watch_for_change(config.as_ref(), "skills", &reading, now_ms) {
            Ok(moved) => {
                for change in moved {
                    record_event(
                        events.as_ref(),
                        clock.as_ref(),
                        &format!("{} went from {} to {}", change.key, change.from, change.to),
                    );
                    this_pass.push(change);
                }
            }
            Err(e) => eprintln!("watching skills: could not record what moved: {e}"),
        }
    }
    wake_on_the_unusual(state, &this_pass, now_ms);
}

/// How long after one wake before another may happen (ADR 0063). In memory: a restart
/// forgets it, which is at worst one extra consideration per restart — accepted over a
/// stored timestamp whose only job is to be one.
pub(super) const WAKE_COOLDOWN_MS: i64 = 60 * 60 * 1_000;
pub(super) static LAST_WAKE_MS: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);

/// A rare change spends the check-in budget early (ADR 0063).
///
/// This adds no new way to speak. It pulls the existing check-in's `next_at` to now, so
/// the next heartbeat runs the same `consider_reaching_out` a scheduled check-in runs —
/// same agentic turn, same reversible-only clamp, same silence-is-the-default. The
/// transition reaches that turn through the fact stream, so the butler wakes already
/// knowing what woke it. No schedule means no waking: the person's one dial governs both.
pub(super) fn wake_on_the_unusual(
    state: &AppState,
    this_pass: &[endora_capabilities::Transition],
    now_ms: i64,
) {
    use std::sync::atomic::Ordering;
    if this_pass.is_empty() {
        return;
    }
    let last = LAST_WAKE_MS.load(Ordering::Relaxed);
    if now_ms.saturating_sub(last) < WAKE_COOLDOWN_MS {
        return;
    }
    // The whole window, including what this pass just recorded.
    let Ok(history) = endora_capabilities::TransitionLog::since(
        state.config.as_ref(),
        now_ms - endora_capabilities::KEEP_TRANSITIONS_FOR_MS,
    ) else {
        return;
    };
    // Endora's own conduct is not the world changing (ADR 0052's oldest rule, in a place it
    // had never reached). The entity it reaches the person through is a `notify` whose state
    // is the timestamp of the last thing IT sent — so sending a message changed the house,
    // rarely, and woke it to consider sending another. A butler that wakes itself up is not
    // attentive, it is pacing.
    //
    // Seen live: three transitions on that entity, all of them Endora's own notifications,
    // and one of them woke a check-in.
    let its_own_doing: std::collections::HashSet<String> = settings_map(state.config.as_ref())
        .values()
        .filter_map(|s| s.get("notify_service").map(|v| v.trim().to_owned()))
        .filter(|v| !v.is_empty())
        // Stored as `notify.thing` or bare `thing` depending on how it was set.
        .flat_map(|v| {
            let bare = v.rsplit('.').next().unwrap_or(&v).to_owned();
            [format!("notify.{bare}"), v]
        })
        .collect();
    let world_changed: Vec<endora_capabilities::Transition> = this_pass
        .iter()
        .filter(|t| {
            let thing = t.key.split_once("::").map_or(t.key.as_str(), |(_, x)| x);
            !its_own_doing.contains(thing)
        })
        .cloned()
        .collect();
    let Some(unusual) = endora_capabilities::worth_waking_for(&world_changed, &history) else {
        return;
    };
    // Only advance a budget that exists: check-ins off means waking off, same switch.
    let Ok(Some(mut schedule)) =
        endora_scheduling::CheckinRepository::get(state.schedules.as_ref())
    else {
        return;
    };
    if !schedule.enabled {
        return;
    }
    schedule.next_at = endora_application::Timestamp::from_unix_millis(now_ms);
    if endora_scheduling::CheckinRepository::set(state.schedules.as_ref(), &schedule).is_err() {
        return;
    }
    LAST_WAKE_MS.store(now_ms, Ordering::Relaxed);
    // Said in the trail, so "why did it message me?" has an answer even when the
    // consideration ends in silence.
    record_event(
        state.events.as_ref(),
        state.clock.as_ref(),
        &format!(
            "Something unusual: {} went from {} to {} — having a look now instead of \
             waiting for the next check-in",
            unusual.key, unusual.from, unusual.to
        ),
    );
}

/// Re-reads stored beliefs against the rules as they stand today (ADR 0052).
///
/// Runs on the heartbeat rather than on read, so the store converges once and the screen
/// stays a plain rendering of what is held. Failures are logged and dropped: this is
/// housekeeping, and a butler that cannot tidy is still a butler.
pub(super) fn tidy_understanding(state: &AppState) {
    let understanding = state.understanding.clone();
    let clock = state.clock.clone();
    match usecases::tidy_understanding(understanding.as_ref(), clock.as_ref()) {
        Ok(0) => {}
        Ok(n) => eprintln!("understanding: retired {n} belief(s) the rules no longer form"),
        Err(e) => eprintln!("understanding: could not tidy: {e}"),
    }
}

/// Spawns the butler's **heartbeat**: a background loop that periodically checks
/// whether a proactive check-in is due (per the person's cadence) and, if so, has
/// the butler post one. Only messages — nothing consequential — so it stays on the
/// safe side of the autonomy model (ADR 0051/0019). The blocking store work runs
/// on a worker thread; a posted check-in nudges the change stream.
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
                tidy_understanding(&state);
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
            let config = state.config.clone();
            let audit = state.audit.clone();
            let ids = state.ids.clone();
            let clock = state.clock.clone();
            let capabilities = state.capabilities.clone();
            let butler = state.butler.clone();
            let mcp = mcp_snapshot(&state);
            // Take the turn lock so a proactive brief/check-in never interleaves
            // with a chat reply; held across the blocking work below.
            let own_work = state.own_work;
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
                    understanding.as_ref(),
                    understanding.as_ref(),
                    understanding.as_ref(),
                    config.as_ref(),
                    chat.as_ref(),
                    &runner,
                    config.as_ref(),
                    clock.as_ref(),
                )?;
                // The butler decides whether it has a reason to speak; the schedule
                // only bounds how often it may (ADR 0056).
                let posted = usecases::consider_reaching_out(
                    chat.as_ref(),
                    schedules.as_ref(),
                    understanding.as_ref(),
                    understanding.as_ref(),
                    &runner,
                    butler.as_ref(),
                    audit.as_ref(),
                    ids.as_ref(),
                    clock.as_ref(),
                    &context,
                )?;
                if let Some((_, activity)) = &posted {
                    for item in activity {
                        record_event(events.as_ref(), clock.as_ref(), item);
                    }
                }
                // A daily brief, if one is due (reversible skills only).
                let briefed = usecases::run_due_brief(
                    chat.as_ref(),
                    config.as_ref(),
                    schedules.as_ref(),
                    &runner,
                    butler.as_ref(),
                    ids.as_ref(),
                    clock.as_ref(),
                    &context,
                )?;
                if let Some((_, activity)) = &briefed {
                    for item in activity {
                        record_event(events.as_ref(), clock.as_ref(), item);
                    }
                    record_event(events.as_ref(), clock.as_ref(), "Prepared your daily brief");
                }
                // The nightly self-improvement loop (ADR 0051), if due: review the
                // day and reflect, within the reversible band — never anything
                // irreversible. Serialized under the turn lock like the brief.
                // The fortnight's transitions — the sensing the thinking cites from.
                let fortnight = endora_capabilities::TransitionLog::since(
                    config.as_ref(),
                    clock.now().unix_millis() - endora_capabilities::KEEP_TRANSITIONS_FOR_MS,
                )
                .unwrap_or_default();
                let reflected = usecases::run_due_nightly_loop(
                    chat.as_ref(),
                    understanding.as_ref(),
                    understanding.as_ref(),
                    understanding.as_ref(),
                    understanding.as_ref(),
                    understanding.as_ref(),
                    understanding.as_ref(),
                    &fortnight,
                    schedules.as_ref(),
                    &runner,
                    // Which things the services say are the person's own rather than the
                    // household's, so a reading can be told apart from the house (ADR 0057).
                    // Empty when nothing can say, which attributes nothing.
                    &native_channels(config.as_ref())
                        .iter()
                        .flat_map(|(_, channel)| channel.belongs_to_the_person())
                        .collect::<Vec<_>>(),
                    butler.as_ref(),
                    audit.as_ref(),
                    ids.as_ref(),
                    clock.as_ref(),
                    &context,
                    own_work,
                )?;
                if let Some((_, activity)) = &reflected {
                    for item in activity {
                        record_event(events.as_ref(), clock.as_ref(), item);
                    }
                    record_event(
                        events.as_ref(),
                        clock.as_ref(),
                        "Ran the nightly self-improvement loop",
                    );
                }
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
                for message in [posted.as_ref(), briefed.as_ref()].into_iter().flatten() {
                    reach_out(config.as_ref(), &message.0);
                }
                Ok::<_, AppError>(posted.is_some() || briefed.is_some() || reflected.is_some())
            })
            .await;
            if let Ok(Ok(true)) = posted {
                let _ = state.changes.send(());
            }

            // Nightly self-improving model tune (ADR 0055), if scheduled + due.
            // Marked fired first (so it can't double-run), then run DETACHED and
            // without the turn lock — it's long and competes on the GPU, which is
            // why the schedule points at an off-hour.
            let due = {
                let config = state.config.clone();
                let clock = state.clock.clone();
                tokio::task::spawn_blocking(move || {
                    let now = clock.now().unix_millis();
                    match ModelTuneScheduleRepository::get(config.as_ref()) {
                        Ok(mut s) if s.is_due(now) => {
                            s.last_ms = now;
                            let _ = ModelTuneScheduleRepository::set(config.as_ref(), &s);
                            true
                        }
                        _ => false,
                    }
                })
                .await
                .unwrap_or(false)
            };
            if due {
                let config = state.config.clone();
                let events = state.events.clone();
                let clock = state.clock.clone();
                let butler = state.butler.clone();
                let model_url = tune_model_url();
                tokio::task::spawn_blocking(move || {
                    run_model_tune(
                        butler.as_ref(),
                        config.as_ref(),
                        events.as_ref(),
                        clock.as_ref(),
                        &model_url,
                    );
                });
            }
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
                    understanding.as_ref(),
                    understanding.as_ref(),
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

/// Looks for something worth looking into and, if everything allows it, starts it. Returns
/// whether it started something. A normal outcome is that it does not (ADR 0094).
///
/// Everything it needs is read from stored records, and the decision is the pure
/// [`endora_application::own_work::next_work`]. The turn lock is only checked, not held: a
/// chat turn in progress means "not now", and the next check is five minutes away.
pub(super) fn start_own_work_once(state: &AppState) -> bool {
    use endora_application::own_work::{Situation, next_work};
    if state.turn_lock.try_lock().is_err() {
        return false;
    }
    let now = state.clock.now();
    let Ok(troubles) =
        endora_capabilities::StandingTroubleRepository::troubles(state.config.as_ref())
    else {
        return false;
    };
    // Sixty days is longer than any item's cooldown, so every attempt that still matters is
    // inside it; an older one no longer counts against an item.
    let since = endora_application::Timestamp::from_unix_millis(
        now.unix_millis() - 60 * 24 * 60 * 60 * 1000,
    );
    let Ok(history) = state.store.own_goals_since(since) else {
        return false;
    };
    let Ok(open) = state.store.active_goals() else {
        return false;
    };
    // Unreadable means nothing to offer, not a reason to stop looking at the rest.
    let specimens =
        endora_understanding::SpecimenRepository::open_specimens(state.understanding.as_ref())
            .unwrap_or_default();
    let focus = usecases::belief_to_look_into(state.understanding.as_ref(), state.clock.as_ref())
        .ok()
        .flatten();
    let situation = Situation {
        troubles: &troubles,
        specimens: &specimens,
        focus: focus.as_ref(),
        history: &history,
        a_request_is_open: open
            .iter()
            .any(|goal| goal.origin() == endora_application::GoalOrigin::Person),
        person_is_quiet: quiet_enough_for_own_work(state, now),
        now,
    };
    let Some(item) = next_work(&situation) else {
        return false;
    };
    let objective = item.objective.clone();
    let Ok(record) = item.into_record(now) else {
        return false;
    };
    // `false` means the same decision was already made; an error means the lane filled in
    // the meantime. Either way, nothing new began.
    if !state.store.create_goal(&record).unwrap_or(false) {
        return false;
    }
    let _ = endora_application::EventLog::record(
        state.events.as_ref(),
        now,
        &format!("Started looking into \"{objective}\" on my own"),
    );
    let _ = state.changes.send(());
    true
}

/// Says what the house does at about the same time most days, when that has changed
/// (ADR 0096). Returns whether it said anything.
///
/// Counting over the transition log, no model. The trail is where it is kept: the picture is
/// compared with the last line of this kind there, and said again only if it differs and the
/// last was at least six hours ago, so a pattern wavering at the edge of the bar does not
/// fill the trail.
pub(super) fn notice_rhythms_once(state: &AppState) -> bool {
    const SAID_AT_MOST_EVERY_MS: i64 = 6 * 60 * 60 * 1000;
    const HEADING: &str = "Regular patterns in the house";
    const MOST_SAID: usize = 5;
    let now = state.clock.now().unix_millis();
    let Ok(transitions) = endora_capabilities::TransitionLog::since(
        state.config.as_ref(),
        now - endora_capabilities::KEEP_TRANSITIONS_FOR_MS,
    ) else {
        return false;
    };
    let rhythms = endora_capabilities::find_rhythms(&transitions, state.utc_offset_minutes);
    if rhythms.is_empty() {
        return false;
    }
    let said = format!(
        "{HEADING}: {}",
        rhythms
            .iter()
            .take(MOST_SAID)
            .map(endora_capabilities::Rhythm::describe)
            .collect::<Vec<_>>()
            .join("; ")
    );
    let last = endora_application::EventLog::recent(state.events.as_ref(), 500)
        .unwrap_or_default()
        .into_iter()
        .find(|event| event.summary.starts_with(HEADING));
    if let Some(last) = last {
        if last.summary == said || now - last.at.unix_millis() < SAID_AT_MOST_EVERY_MS {
            return false;
        }
    }
    let _ = endora_application::EventLog::record(state.events.as_ref(), state.clock.now(), &said);
    let _ = state.changes.send(());
    true
}

/// Starts looking into things unasked, on the slow cadence ADR 0094 sets. Run only when the
/// owner has turned it on (`ENDORA_OWN_WORK`); until then nothing creates this kind of work.
pub fn spawn_own_work(state: AppState) {
    tokio::spawn(async move {
        let period = std::time::Duration::from_millis(
            endora_application::own_work::START_CHECK_EVERY_MS.unsigned_abs(),
        );
        let mut ticker = tokio::time::interval(period);
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            start_own_work_once(&state);
            notice_rhythms_once(&state);
        }
    });
}

/// Whether the person has been quiet long enough for a goal Endora started for itself to take
/// a pass (ADR 0094).
fn quiet_enough_for_own_work(state: &AppState, now: endora_application::Timestamp) -> bool {
    // Upper bound open: a message dated after `now` is found too, and the rule refuses it.
    let from = now
        .unix_millis()
        .saturating_sub(endora_application::record_lane::QUIET_WINDOW_MS);
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
    endora_application::record_lane::quiet_enough(now, last_spoke)
}

/// Resumes one persisted deterministic goal at a time. The worker is deliberately
/// small: SQLite is the queue, the goal repository is the ownership boundary, and
/// the existing turn lock prevents it from competing with foreground chat.
pub fn spawn_goal_worker(state: AppState) {
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(5));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let pending_record = state
                .store
                .pending_goal_completions()
                .ok()
                .and_then(|pending| pending.into_iter().next());
            if let Some(record) = pending_record {
                // Work Endora started for itself does not post to chat (ADR 0094). Its ending
                // goes to the activity trail; it is spent here, once, with no message and no
                // turn lock to wait for.
                if record.origin() == endora_application::GoalOrigin::Record {
                    if state
                        .store
                        .mark_goal_announced(record.request_key())
                        .is_ok()
                    {
                        let _ = endora_application::EventLog::record(
                            state.events.as_ref(),
                            state.clock.now(),
                            &endora_application::own_work::trail_line(&record),
                        );
                        let _ = state.changes.send(());
                    }
                    continue;
                }
                let Ok(_turn) = state.turn_lock.clone().try_lock_owned() else {
                    continue;
                };
                // Every way a goal stops on its own is announced, with what was found
                // and what is missing; a partial result used to settle and vanish.
                let notice = endora_application::ChatMessage::new(
                    endora_application::MessageId::new(state.ids.new_id()),
                    endora_application::MessageRole::Butler,
                    &record.goal().outcome_notice(),
                    state.clock.now(),
                );
                if let Ok(notice) = notice {
                    if state
                        .store
                        .publish_goal_completion(&record, &notice)
                        .unwrap_or(false)
                    {
                        let _ = state.changes.send(());
                    }
                }
                continue;
            }
            let Ok(goals) = state.store.active_goals() else {
                continue;
            };
            if goals.is_empty() {
                continue;
            }
            // The turn lock first: a foreground chat turn holds it, and reserving an
            // attempt before finding it busy spent the goal's budget for nothing.
            let Ok(_turn) = state.turn_lock.clone().try_lock_owned() else {
                continue;
            };
            let now = state.clock.now();
            // Person goals come first in this order, so reaching one Endora started for
            // itself means no request is claimable. It still waits for quiet (ADR 0094):
            // the first goal of its own that is reached asks once, and the answer holds for
            // the rest of this tick.
            let mut quiet: Option<bool> = None;
            let Some(lease) = goals.into_iter().find_map(|goal| {
                if goal.origin() == endora_application::GoalOrigin::Record
                    && !*quiet.get_or_insert_with(|| quiet_enough_for_own_work(&state, now))
                {
                    return None;
                }
                state
                    .store
                    .claim_goal(goal.request_key(), now)
                    .ok()
                    .flatten()
            }) else {
                continue;
            };
            let Ok(Some(record)) = state.store.get_goal(lease.request_key()) else {
                let _ = state.store.release_goal(&lease);
                continue;
            };
            let store = state.store.clone();
            let config = state.config.clone();
            let understanding = state.understanding.clone();
            let capabilities = state.capabilities.clone();
            let clock = state.clock.clone();
            let butler = state.butler.clone();
            let audit = state.audit.clone();
            let ids = state.ids.clone();
            let chat = state.chat.clone();
            let mcp = mcp_snapshot(&state);
            let worker_lease = lease.clone();
            let events = state.events.clone();
            let mut task = tokio::task::spawn_blocking(move || {
                // Work derived from a record is finished by that record, not by a pass: read
                // it again first (ADR 0094). A trouble that cleared completes the goal with
                // no model call; one the person called fine, an ask taken off the shelf, or a
                // belief no longer held stops it. If the records cannot be read, skip the
                // pass and leave the goal as it was: working blind is not worth a pass.
                let mut replay_of = None;
                if record.origin() == endora_application::GoalOrigin::Record {
                    let troubles =
                        endora_capabilities::StandingTroubleRepository::troubles(config.as_ref());
                    let open = endora_understanding::SpecimenRepository::open_specimens(
                        understanding.as_ref(),
                    );
                    let held = usecases::beliefs_still_held(understanding.as_ref(), clock.as_ref());
                    let (Ok(troubles), Ok(open), Ok(held)) = (troubles, open, held) else {
                        return Ok(false);
                    };
                    let records = endora_application::own_work::Records {
                        troubles: &troubles,
                        open_specimens: &open,
                        active_beliefs: &held,
                    };
                    match endora_application::own_work::recheck(&record, &records, clock.now()) {
                        endora_application::own_work::Recheck::Resolved(done) => {
                            return store.checkpoint_goal(&done, &worker_lease, clock.now());
                        }
                        endora_application::own_work::Recheck::Withdrawn(why) => {
                            let at = clock.now();
                            let _ = store.cancel_goal(record.request_key(), &why, at);
                            let _ = endora_application::EventLog::record(
                                events.as_ref(),
                                at,
                                &format!(
                                    "Stopped looking into \"{}\": {why}",
                                    record.goal().objective()
                                ),
                            );
                            return Ok(true);
                        }
                        endora_application::own_work::Recheck::Unchanged => {}
                    }
                    // An ask that failed is not worked on by the open-ended loop: it is asked
                    // again, once, and judged by the verdict that filed it (ADR 0095).
                    if let Some(endora_application::goal_request::GoalCriteria::Resolve {
                        source: endora_application::goal_request::WorkSource::Specimen { id },
                    }) = record.goal().criteria()
                    {
                        replay_of = open.iter().find(|s| &s.id == id).cloned();
                    }
                }
                // Clamped to the reversible bands, because nobody is watching. This
                // worker used to build the unclamped runner, which was survivable
                // only because the one thing it could resume was a schedule read;
                // now the model chooses the tool, so the clamp is load-bearing —
                // a goal may gather and can never actuate unattended.
                let runner = build_reversible_only_runner(
                    config.as_ref(),
                    capabilities,
                    mcp,
                    proven_now(understanding.as_ref()),
                );
                let runner = endora_application::goal_execution::OwnedGoalRunner::new(
                    &runner,
                    store.as_ref(),
                    &worker_lease,
                    clock.as_ref(),
                );
                // Without the context the pass would run blind; skip it and leave the
                // goal exactly as it was, so the next tick tries again. Failing to
                // read context is a reason not to work, never a reason to end the
                // objective.
                let Ok(context) = usecases::butler_context(
                    understanding.as_ref(),
                    understanding.as_ref(),
                    understanding.as_ref(),
                    config.as_ref(),
                    chat.as_ref(),
                    &runner,
                    config.as_ref(),
                    clock.as_ref(),
                ) else {
                    return Ok(false);
                };
                let updated = if let Some(specimen) = &replay_of {
                    // The result is recorded on the specimen first; if that fails, the goal
                    // is left as it was and the next tick asks again.
                    match usecases::replay_specimen(
                        &record,
                        specimen,
                        butler.as_ref(),
                        &runner,
                        audit.as_ref(),
                        understanding.as_ref(),
                        understanding.as_ref(),
                        understanding.as_ref(),
                        ids.as_ref(),
                        clock.as_ref(),
                        &context,
                    ) {
                        Ok(done) => done,
                        Err(_) => return Ok(false),
                    }
                } else {
                    usecases::resume_goal(
                        &record,
                        butler.as_ref(),
                        &runner,
                        audit.as_ref(),
                        understanding.as_ref(),
                        understanding.as_ref(),
                        ids.as_ref(),
                        clock.as_ref(),
                        &context,
                    )
                };
                store.checkpoint_goal(&updated, &worker_lease, clock.now())
            });
            let mut heartbeat = tokio::time::interval(std::time::Duration::from_secs(10));
            heartbeat.tick().await;
            let result = loop {
                tokio::select! {
                    result = &mut task => break result,
                    _ = heartbeat.tick() => {
                        if !state.store.renew_goal(&lease, state.clock.now()).unwrap_or(false) {
                            // A dispatched operation keeps its transport deadline;
                            // the guarded runner rejects every subsequent dispatch.
                            break task.await;
                        }
                    }
                }
            };
            if matches!(result, Ok(Ok(true))) {
                let _ = state.changes.send(());
            }
            let _ = state.store.release_goal(&lease);
        }
    });
}
