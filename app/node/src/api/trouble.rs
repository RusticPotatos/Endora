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

/// How many of the changes made to the owner's services the console lists.
pub(super) const CONFIG_WRITES_SHOWN: usize = 50;

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
