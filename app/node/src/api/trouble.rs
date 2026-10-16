use super::*;

#[derive(Deserialize)]
pub(super) struct CollectionRequest {
    server: String,
    name: String,
    /// What the collection stands for — matched against the names the service knows, so
    /// the caller says "every light" in the words a person uses rather than in ids.
    of: Vec<String>,
}

#[derive(Deserialize)]
pub(super) struct WorthKnowingRequest {
    fits_gb: Option<u32>,
}

/// The card to assume when nobody has said. Small enough that what it suggests will run
/// almost anywhere, since suggesting something unrunnable is the worse mistake.
pub(super) const DEFAULT_VRAM_GB: u32 = 8;

/// Models the hub has that would fit this machine (ADR 0055).
///
/// Reports; it does not fetch. Endora does not manage the model runtime — it says what
/// exists and hands over the command, and the person runs it. Downloading gigabytes onto
/// someone's box unasked is a far worse failure than a slow reply.
///
/// Asked for on demand rather than on a schedule: a weekly "there are new models" message
/// is noise in an inbox meant for things that matter.
pub(super) async fn models_worth_knowing(
    Query(req): Query<WorthKnowingRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    // Asked for rather than guessed: Endora runs in a container and cannot see the card,
    // and inventing a ceiling would either hide models that fit or suggest ones that
    // cannot run. The console asks the person once and remembers.
    let vram = req.fits_gb.unwrap_or(DEFAULT_VRAM_GB);
    let seen = blocking(move || {
        endora_infrastructure::model_watch::worth_knowing_about(vram)
            .map_err(|message| AppError::Model { message })
    })
    .await?;
    Ok(Json(json!({
        "fits_gb": vram,
        "models": seen
            .iter()
            .map(|m| json!({
                "id": m.id,
                "about_gb": m.about_gb,
                "updated": m.updated,
                "downloads": m.downloads,
                "how_to_get_it": m.how_to_get_it,
            }))
            .collect::<Vec<_>>(),
    })))
}

/// Makes one thing that stands for many, in the service that owns them (ADR 0054).
///
/// The general answer to a request no amount of aiming can express. "All the lights" is
/// not a target and never can be; a collection *is* one, and once it exists nothing about
/// acting on it is special.
pub(super) async fn make_a_collection(
    State(state): State<AppState>,
    Json(req): Json<CollectionRequest>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let said = blocking(move || {
        let channels = native_channels(config.as_ref());
        let Some((_, channel)) = channels.iter().find(|(name, _)| *name == req.server) else {
            return Err(AppError::BadRequest {
                message: format!("Endora has no direct reach into {}", req.server),
            });
        };
        // Resolve the person's words to the service's own ids. Nothing is invented: a
        // name that matches nothing is reported rather than guessed at.
        let known = channel.known().map_err(|why| AppError::BadRequest { message: why })?;
        let mut members: Vec<String> = Vec::new();
        let mut unknown: Vec<String> = Vec::new();
        for wanted in &req.of {
            match known
                .iter()
                .find(|(_, name)| name.eq_ignore_ascii_case(wanted.trim()))
            {
                Some((id, _)) if !members.contains(id) => members.push(id.clone()),
                Some(_) => {}
                None => unknown.push(wanted.clone()),
            }
        }
        if !unknown.is_empty() {
            return Err(AppError::BadRequest {
                message: format!("{} isn't anything this service knows", unknown.join(", ")),
            });
        }
        match channel.collect(&req.name, &members) {
            Some(Ok(mut write)) => {
                write.id = endora_application::IdSource::new_id(ids.as_ref());
                write.at_ms = clock.now().unix_millis();
                write.server = req.server;
                let described = write.describe();
                endora_capabilities::ConfigWriteLog::record(config.as_ref(), &write)
                    .map_err(AppError::Repository)?;
                Ok(json!({ "made": described, "id": write.id.to_string(), "members": members.len() }))
            }
            Some(Err(why)) => Err(AppError::BadRequest { message: why }),
            None => Err(AppError::BadRequest {
                message: "Endora is not allowed to change this service's settings".to_owned(),
            }),
        }
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(said))
}

/// Exactly what the butler is told before it answers.
///
/// Five separate times this session a fact was believed to be reaching a turn and was not —
/// presence, the facts behind an answer, the activity account, the calendar — and each was
/// diagnosed by inference because there was no way to look. Guessing lost every time.
///
/// So the turn's own grounding is readable. It is also the honest answer to "why did it say
/// that?": if something is absent here, the model never had it, and no amount of prompting
/// would have helped.
pub(super) async fn what_the_butler_is_told(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let understanding = state.understanding.clone();
    let knowledge = state.knowledge.clone();
    let config = state.config.clone();
    let chat = state.chat.clone();
    let clock = state.clock.clone();
    let mcp = mcp_snapshot(&state);
    let capabilities = state.capabilities.clone();
    let told = blocking(move || {
        let runner = build_runner(
            config.as_ref(),
            capabilities,
            mcp,
            proven_now(understanding.as_ref()),
        );
        usecases::butler_context(
            knowledge.as_ref(),
            knowledge.as_ref(),
            understanding.as_ref(),
            config.as_ref(),
            chat.as_ref(),
            &runner,
            config.as_ref(),
            clock.as_ref(),
        )
    })
    .await?;
    Ok(Json(json!({
        "now": told.now,
        "right_now": told.present,
        "what_it_has_been_doing": told.did_lately,
        "memory": told.memory,
        "brain_pages": told.brain_index,
        "names_it_knows": told.target_aliases,
        "how_its_actions_landed": told.track_record,
        "where_they_are": told.where_they_are,
        "skills_offered": told.capabilities.len(),
    })))
}

/// How Endora's recent actions actually landed (ADR 0053).
///
/// The battery scores the model; this scores the system. Without it, "more agentic" is a
/// feeling — and reliability is what decides how far autonomy can safely extend, because it
/// compounds.
pub(super) async fn how_it_has_been_landing(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    /// Enough to be a trend, few enough that a bad week is still visible.
    const RECENT: usize = 50;
    let understanding = state.understanding.clone();
    let all = blocking(move || {
        endora_understanding::OutcomeRepository::list(understanding.as_ref())
            .map_err(AppError::Repository)
    })
    .await?;
    // Newest first, which is what the tally's window expects.
    let mut newest = all;
    newest.sort_by_key(|o| std::cmp::Reverse(o.at().unix_millis()));
    let tally = endora_understanding::Reliability::over(&newest, RECENT);
    Ok(Json(json!({
        "considered": tally.considered,
        "changed": tally.changed,
        "unchanged": tally.unchanged,
        "failed": tally.failed,
        "unchecked": tally.unchecked,
        "worst_offender": tally.worst_offender.as_ref().map(|(id, n)| json!({ "capability": id, "times": n })),
        "in_words": tally.in_words(),
    })))
}

/// What kind of thing to connect, e.g. `caldav`.
#[derive(serde::Deserialize)]
pub(super) struct BeginConnecting {
    server: String,
    kind: String,
}

/// Starts connecting something new to a service, returning **that service's own form**.
///
/// Endora renders whatever comes back rather than knowing what a calendar needs, so a kind
/// The integrations already configured behind each connected channel (ADR 0058).
///
/// Read-only and best-effort: a channel that cannot say returns nothing, and the screen simply
/// shows no label rather than claiming the service is unconnected.
pub(super) async fn what_is_connected(
    State(state): State<AppState>,
) -> Result<Json<Vec<String>>, ApiError> {
    let config = state.config.clone();
    let found = blocking(move || {
        let mut all: Vec<String> = native_channels(config.as_ref())
            .iter()
            .flat_map(|(_, channel)| channel.already_connected())
            .collect();
        all.sort();
        all.dedup();
        Ok::<_, endora_application::AppError>(all)
    })
    .await?;
    Ok(Json(found))
}

/// of thing nobody here has heard of works exactly like one that ships today (ADR 0054).
pub(super) async fn begin_connecting(
    State(state): State<AppState>,
    Json(req): Json<BeginConnecting>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let form = blocking(move || {
        let channels = native_channels(config.as_ref());
        let Some((_, channel)) = channels.iter().find(|(name, _)| *name == req.server) else {
            return Err(AppError::BadRequest {
                message: format!("Endora has no direct reach into {}", req.server),
            });
        };
        match channel.begin_setup(&req.kind) {
            Some(Ok(form)) => Ok(form),
            Some(Err(why)) => Err(AppError::BadRequest { message: why }),
            None => Err(AppError::BadRequest {
                message: "Endora is not allowed to change this service's settings".to_owned(),
            }),
        }
    })
    .await?;
    Ok(Json(form_json(&form)))
}

/// The answers to one step of a setup form.
#[derive(serde::Deserialize)]
pub(super) struct FinishConnecting {
    server: String,
    form: String,
    answers: std::collections::BTreeMap<String, String>,
}

/// Sends the person's answers to their own service.
///
/// **Nothing here is stored.** A credential travels from the keyboard to the service that
/// will hold it and is not written down on the way — no setting, no log line, no event
/// text. Endora is passing a message, not keeping an account.
pub(super) async fn finish_connecting(
    State(state): State<AppState>,
    Json(req): Json<FinishConnecting>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let events = state.events.clone();
    let clock = state.clock.clone();
    let kind_said = req.server.clone();
    let outcome = blocking(move || {
        let channels = native_channels(config.as_ref());
        let Some((_, channel)) = channels.iter().find(|(name, _)| *name == req.server) else {
            return Err(AppError::BadRequest {
                message: format!("Endora has no direct reach into {}", req.server),
            });
        };
        let answers: Vec<(String, String)> = req.answers.into_iter().collect();
        match channel.finish_setup(&req.form, &answers) {
            Some(Ok(next)) => Ok(next),
            Some(Err(why)) => Err(AppError::BadRequest { message: why }),
            None => Err(AppError::BadRequest {
                message: "Endora is not allowed to change this service's settings".to_owned(),
            }),
        }
    })
    .await?;
    let _ = state.changes.send(());
    match outcome {
        // Only the fact is recorded, never the answers.
        None => {
            record_event(
                events.as_ref(),
                clock.as_ref(),
                &format!("Connected something new to {kind_said}"),
            );
            Ok(Json(json!({ "done": true })))
        }
        Some(form) => Ok(Json(form_json(&form))),
    }
}

/// A setup form as the interface needs it. A secret field is marked so the console masks it
/// and never redisplays a value.
pub(super) fn form_json(form: &endora_capabilities::SetupForm) -> serde_json::Value {
    json!({
        "done": false,
        "form": form.id,
        "step": form.step,
        "fields": form.fields.iter().map(|f| json!({
            "name": f.name,
            "kind": f.kind,
            "required": f.required,
            "default": f.default,
            "secret": f.secret,
        })).collect::<Vec<_>>(),
    })
}

/// Every change Endora has made to a service's own configuration (ADR 0054).
///
/// The memory right to *see* what it changed about the world, next to the right to see
/// what it believes and what it did.
pub(super) async fn list_config_writes(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let config = state.config.clone();
    let writes = blocking(move || {
        endora_capabilities::ConfigWriteLog::writes(config.as_ref(), CONFIG_WRITES_SHOWN)
            .map_err(AppError::Repository)
    })
    .await?;
    Ok(Json(
        writes
            .iter()
            .map(|w| {
                json!({
                    "id": w.id.to_string(),
                    "at_ms": w.at_ms,
                    "server": w.server,
                    "target": w.target,
                    "added": w.added,
                    "was": w.was,
                    "undone": w.undone,
                    "what": w.describe(),
                })
            })
            .collect(),
    ))
}

/// Puts one change back exactly as it was (ADR 0054).
///
/// The row is **kept** and marked, never deleted: what Endora changed about someone's
/// house is not something it should be able to make disappear.
pub(super) async fn undo_config_write(
    State(state): State<AppState>,
    Path(id): Path<String>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let wanted: u128 = id.parse().map_err(|_| {
        ApiError(AppError::BadRequest {
            message: "that is not a change id".to_owned(),
        })
    })?;
    let config = state.config.clone();
    let said = blocking(move || {
        let Some(write) = endora_capabilities::ConfigWriteLog::write(config.as_ref(), wanted)
            .map_err(AppError::Repository)?
        else {
            return Err(AppError::NotFound { entity: "change" });
        };
        if write.undone {
            return Ok("that change was already put back".to_owned());
        }
        let channels = native_channels(config.as_ref());
        let Some((_, channel)) = channels.iter().find(|(name, _)| *name == write.server) else {
            return Err(AppError::BadRequest {
                message: format!("Endora has no direct reach into {} any more", write.server),
            });
        };
        match channel.undo(&write) {
            Some(Ok(said)) => {
                endora_capabilities::ConfigWriteLog::mark_undone(config.as_ref(), wanted)
                    .map_err(AppError::Repository)?;
                Ok(said)
            }
            Some(Err(why)) => Err(AppError::BadRequest { message: why }),
            None => Err(AppError::BadRequest {
                message: "Endora is not allowed to write names into this service".to_owned(),
            }),
        }
    })
    .await?;
    let _ = state.changes.send(());
    Ok(Json(json!({ "ok": true, "undone": said })))
}

/// The person asks not to be bothered about one thing again.
///
/// Deliberately narrow: it silences **this** question and turns nothing off. A finding they
/// cannot answer and cannot dismiss is a chore that never empties, which is the shape ADR 0052
/// exists to prevent — and the trouble card has had its equivalent since it was written.
pub(super) async fn leave_this_one_alone(
    State(state): State<AppState>,
    Path((server, thing)): Path<(String, String)>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    blocking(move || {
        config
            .leave_it_alone(&server, &thing)
            .map_err(endora_application::AppError::from)
    })
    .await?;
    Ok(Json(json!({ "ok": true })))
}

/// Every name the person has already taught, as `(server, what they said)`.
///
/// A target they have named is a question they have answered, and a question that has been
/// answered stops being asked (ADR 0052). Without this the card asking "what is it actually
/// called?" survived being told — the finding derives from outcomes, and the failures that
/// produced it stay in the record for good.
pub(super) fn named_already(config: &endora_capabilities::ConfigStore) -> Vec<(String, String)> {
    let mut answered: Vec<(String, String)> =
        endora_capabilities::TargetAliasRepository::aliases(config)
            .unwrap_or_default()
            .into_iter()
            .map(|a| (a.server, a.said))
            .collect();
    // "Stop asking" is an answer too. Somebody who does not know a thing's real name has
    // nothing to type, and without this the card had no way out at all.
    answered.extend(config.left_alone().unwrap_or_default());
    answered
}

/// Turns off capabilities Endora has established do not work (ADR 0051).
///
/// The finding was already derived and already correct; it just sat in a card waiting for
/// someone to click. A butler that owns its own tooling acts on what it knows, so policy
/// applies it — **policy**, not the model. The derivation is arithmetic over stored
/// outcomes with nothing generative in the path, which is what makes acting on it
/// consistent with ADR 0051 rather than a widening of what the model is trusted to do.
///
/// Three properties make this safe to do unattended:
///
/// - it only ever turns something **off**, which cannot break anything that was working;
/// - the bar is deliberately high — several targets, repeated outright refusals, and not
///   one success of any kind, including unverified ones (ADR 0054);
/// - it is **one click** to undo, and the activity trail says what happened and why.
pub(super) fn withdraw_what_never_works(state: &AppState) {
    use endora_capabilities::CapabilityConfigRepository;
    let Ok(found) = usecases::repairs(state.understanding.as_ref(), &named_already(&state.config))
    else {
        return;
    };
    let already: std::collections::HashSet<String> = state
        .config
        .stances()
        .unwrap_or_default()
        .into_iter()
        .filter(|(_, st)| *st == endora_capabilities::Stance::Off)
        .map(|(id, _)| id)
        .collect();
    for repair in found {
        if repair.remedy != endora_understanding::Remedy::StopOfferingIt
            || already.contains(&repair.capability)
        {
            continue;
        }
        if state
            .config
            .set_stance(&repair.capability, endora_capabilities::Stance::Off)
            .is_err()
        {
            continue;
        }
        // Said out loud, in the place the person already looks. A capability quietly
        // disappearing is the kind of silent narrowing that erodes trust, so the record
        // carries the count that justified it and the way back.
        record_event(
            state.events.as_ref(),
            state.clock.as_ref(),
            &format!(
                "Stopped offering the {} skill to myself: {} attempts, every one refused, \
                 and it has never once worked. Turn it back on under Skills if you want it.",
                repair.capability, repair.attempts
            ),
        );
        let _ = state.changes.send(());
    }
}

/// Writes the names the person has confirmed back into the service that owns them
/// (ADR 0054).
///
/// A confirmed alias currently helps only Endora. The same fact written into Home
/// Assistant's own registry helps **everything** that talks to that house — its app, its
/// voice assistants, anything else — and stops the failure at the source rather than
/// recovering from it every time.
///
/// Deliberately only what the person already confirmed. Endora does not invent names here.
pub(super) async fn push_aliases_upstream(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let config = state.config.clone();
    let ids = state.ids.clone();
    let clock = state.clock.clone();
    let taught = blocking(move || {
        let channels = native_channels(config.as_ref());
        let aliases = endora_capabilities::TargetAliasRepository::aliases(config.as_ref())
            .map_err(AppError::Repository)?;
        let mut said: Vec<serde_json::Value> = Vec::new();
        for alias in aliases {
            let Some((_, channel)) = channels.iter().find(|(name, _)| *name == alias.server) else {
                continue;
            };
            let result = channel.teach(&alias.means, &alias.said);
            said.push(match result {
                Some(Ok(mut write)) => {
                    // The change is only real once it is written down: the prior value is
                    // the undo, and holding it for the length of a function call is not a
                    // reversibility story (ADR 0054).
                    let already = write
                        .was
                        .iter()
                        .any(|a| a.eq_ignore_ascii_case(&write.added));
                    write.id = endora_application::IdSource::new_id(ids.as_ref());
                    write.at_ms = clock.now().unix_millis();
                    write.server = alias.server.clone();
                    let described = write.describe();
                    if !already {
                        endora_capabilities::ConfigWriteLog::record(config.as_ref(), &write)
                            .map_err(AppError::Repository)?;
                    }
                    json!({
                        "alias": alias.said,
                        "of": alias.means,
                        "done": if already {
                            format!("{} already answers to '{}'", write.target, write.added)
                        } else {
                            described
                        },
                        "id": if already { String::new() } else { write.id.to_string() },
                    })
                }
                Some(Err(why)) => {
                    json!({ "alias": alias.said, "of": alias.means, "failed": why })
                }
                None => json!({
                    "alias": alias.said,
                    "of": alias.means,
                    "skipped": "Endora is not allowed to write names into this service",
                }),
            });
        }
        Ok(said)
    })
    .await?;
    Ok(Json(json!({ "taught": taught })))
}

/// What Endora has noticed is wrong with its own tooling (ADR 0054).
///
/// Derived on read, never stored — there is nothing here to dismiss or process.
pub(super) async fn list_repairs(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let understanding = state.understanding.clone();
    let config = state.config.clone();
    let (found, withdrawn) = blocking(move || {
        use endora_capabilities::CapabilityConfigRepository;
        let found = usecases::repairs(understanding.as_ref(), &named_already(&config))?;
        let withdrawn: std::collections::HashSet<String> = config
            .stances()
            .unwrap_or_default()
            .into_iter()
            .filter(|(_, st)| *st == endora_capabilities::Stance::Off)
            .map(|(id, _)| id)
            .collect();
        Ok((found, withdrawn))
    })
    .await?;
    Ok(Json(
        found
            .iter()
            // A tool that is already turned off can never produce new evidence, so its
            // finding would sit there forever asking for something already done. The
            // derivation stays pure and unaware of config (ADR 0054); this is the one
            // place that knows both, and answering the question is what retires the card.
            .filter(|r| {
                r.remedy != endora_understanding::Remedy::StopOfferingIt
                    || !withdrawn.contains(&r.capability)
            })
            .map(|r| {
                json!({
                    "capability": r.capability,
                    "target": r.target,
                    "attempts": r.attempts,
                    // What would actually fix it (ADR 0054). The console offers a
                    // different control for each, because "what is it really called?"
                    // is the wrong question about a tool that has never worked at all.
                    "remedy": match r.remedy {
                        endora_understanding::Remedy::NameTheTarget => "name_the_target",
                        endora_understanding::Remedy::StopOfferingIt => "stop_offering_it",
                    },
                })
            })
            .collect(),
    ))
}

/// What Endora has done lately, and what it saw afterwards (ADR 0053) — the memory
/// right to *see* its actions, next to the beliefs it holds.
pub(super) async fn list_outcomes(
    State(state): State<AppState>,
) -> Result<Json<Vec<serde_json::Value>>, ApiError> {
    let understanding = state.understanding.clone();
    let items =
        blocking(move || usecases::recent_outcomes(understanding.as_ref(), OUTCOMES_SHOWN)).await?;
    Ok(Json(items.iter().map(outcome_json).collect()))
}

#[derive(serde::Deserialize)]
pub(super) struct ReactionBody {
    reaction: String,
}

/// The person says how an action landed. Offered where the action already appears —
/// they are never asked for it (ADR 0053).
pub(super) async fn react_to_outcome(
    State(state): State<AppState>,
    Path(id): Path<String>,
    Json(body): Json<ReactionBody>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let oid = id
        .parse::<u128>()
        .map(endora_application::OutcomeId::new)
        .map_err(|_| ApiError(AppError::NotFound { entity: "outcome" }))?;
    let reaction =
        endora_application::Reaction::from_name(body.reaction.trim()).ok_or_else(|| {
            ApiError(AppError::BadRequest {
                message: "reaction must be helped, did_not_help, or no_reaction".to_owned(),
            })
        })?;
    let understanding = state.understanding.clone();
    let out =
        blocking(move || usecases::react_to_outcome(understanding.as_ref(), oid, reaction)).await?;
    Ok(Json(outcome_json(&out)))
}
