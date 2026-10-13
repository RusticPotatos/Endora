use super::*;

// ---- Recipes: the person's own capabilities (ADR 0071) ---------------------

/// Runs the person's own [`Recipe`]s as capabilities — the whole self-authoring
/// sandbox. A recipe cannot act, by construction: `Reversibility::Observe` is
/// hardcoded below, not read from anywhere a recipe could set it, and its
/// interpretation never grows past the five fields it has (there is no branch,
/// no loop, no second request to add one to). Its output taints the turn like any
/// other outside service's words (`third_party: true`, ADR 0064).
///
/// **Off is the default a stored stance overrides, not the observe band's own.**
/// Every other read defaults to `Auto` — a person did not choose to expose it, the
/// band did. A recipe is different: the person wrote the request themselves, and
/// running it before they have said so once is the one thing this format refuses
/// to do silently. The HTTP layer stamps an explicit `Off` row at authoring time
/// (ADR 0062's stored-beats-default, the same mechanism `trust_all` already uses
/// for MCP tools) — this reads that row, and reads it fresh, so *nothing* here is
/// hardcoded as a special case in the stance ladder itself.
///
/// Interpretation reuses [`scan_outbound_secret`] and [`redact_pii_in_value`] —
/// the exact free functions [`RegistryRunner::run`] calls — rather than a second
/// copy of either (ADR 0071's "zero parallel machinery").
pub struct RecipeRunner {
    recipes: Vec<Recipe>,
    stances: std::collections::HashMap<String, Stance>,
}

impl RecipeRunner {
    /// Wraps the person's recipes with their stored stances — built fresh per
    /// turn, the same posture as [`RegistryRunner::with_config`], so authoring or
    /// enabling one takes effect on the very next turn.
    #[must_use]
    pub fn new(recipes: Vec<Recipe>, stances: Vec<(String, Stance)>) -> Self {
        Self {
            recipes,
            stances: stances.into_iter().collect(),
        }
    }

    /// A recipe's stance, defaulting to `Off` — see the struct doc for why that
    /// default lives here and not in [`default_stance`].
    fn stance_of(&self, capability_id: &str) -> Stance {
        self.stances
            .get(capability_id)
            .copied()
            .unwrap_or(Stance::Off)
    }

    fn find(&self, id: &str) -> Option<&Recipe> {
        let short = id.strip_prefix("recipe.")?;
        self.recipes.iter().find(|r| r.id == short)
    }
}

impl CapabilityRunner for RecipeRunner {
    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.recipes
            .iter()
            .map(|r| {
                let capability_id = r.capability_id();
                // Binary, deliberately: the console offers one affirmative action
                // ("enable"), not a confirm-each-use middle state, so anything
                // other than Off is the same as enabled.
                let enabled = self.stance_of(&capability_id) != Stance::Off;
                crate::application::CapabilitySpec {
                    id: capability_id,
                    wants_place: false,
                    third_party: true,
                    description: r.description.clone(),
                    configured: enabled,
                    reversibility: Reversibility::Observe,
                    autonomous: enabled,
                    input_schema: Some(recipe_input_schema(r)),
                }
            })
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        self.find(id).map(|r| {
            if self.stance_of(&r.capability_id()) == Stance::Off {
                Decision::Block
            } else {
                Decision::Act
            }
        })
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        let recipe = self
            .find(id)
            .ok_or_else(|| format!("no such recipe '{id}'"))?;
        if self.stance_of(&recipe.capability_id()) == Stance::Off {
            return Err(format!("the '{id}' recipe is turned off"));
        }
        // The same tripwire and redaction every reaches_external built-in goes
        // through — a recipe always reaches external by construction (its `get`
        // is required to be http(s)), so both apply unconditionally.
        if let Some(kind) = scan_outbound_secret(input_json) {
            return Err(format!(
                "refusing to send this to '{id}' — the request looks like it contains {kind}"
            ));
        }
        let mut input: Value = serde_json::from_str(input_json.trim())
            .or_else(|_| Ok::<Value, serde_json::Error>(json!({})))
            .unwrap_or_else(|_| json!({}));
        redact_pii_in_value(&mut input);
        run_recipe(recipe, &input)
    }
}

/// The JSON Schema a recipe's declared inputs describe, for native tool-calling —
/// the same role `input_schema` plays for every other skill. Every input is
/// required: a `get` slot with nothing to fill it is a request that cannot be
/// made, and there is no optional-parameter concept in five fields to fall back
/// to.
pub(super) fn recipe_input_schema(recipe: &Recipe) -> String {
    let properties: serde_json::Map<String, Value> = recipe
        .inputs
        .iter()
        .map(|i| {
            (
                i.name.clone(),
                json!({ "type": i.kind.as_str(), "description": format!("for {}", recipe.description) }),
            )
        })
        .collect();
    let required: Vec<&str> = recipe.inputs.iter().map(|i| i.name.as_str()).collect();
    json!({ "type": "object", "properties": properties, "required": required }).to_string()
}

/// Runs one recipe: fills `get` from `input`, fetches it, and fills `say` from
/// the response. The whole interpreter — nothing here can do anything a plain GET
/// followed by reading two known fields back could not already do.
///
/// # Errors
/// A `String` message when an input is missing or the wrong shape, the request
/// itself is refused (by [`RecipeRunner::run`]'s checks above), the fetch fails,
/// or the response is not JSON / lacks a path `say` names.
pub(super) fn run_recipe(recipe: &Recipe, input: &Value) -> Result<String, String> {
    let filled_get = fill_get_template(recipe, input)?;
    // GUARDED, not the plain helper. A recipe's URL is arbitrary — the person
    // typed it, and one day the butler will draft it — which is exactly the case
    // `guard_egress` exists for (ADR 0051). The built-in skills reach constant
    // hosts and need no guard; this one shipped using their helper and was, until
    // this line, a way to point the butler at 169.254.169.254 or anything else on
    // the house's own network. Redirects are re-guarded per hop by the same call.
    let body = guarded_get_text(&filled_get, 64 * 1024).map_err(|e| e.to_string())?;
    let response: Value =
        serde_json::from_str(&body).map_err(|_| "the response wasn't JSON".to_owned())?;
    fill_say_template(&recipe.say, &response)
}

/// Fills `recipe.get`'s `{slot}` placeholders from `input`, URL-encoding every
/// value. Validated at authoring time ([`Recipe::new`]) to name only declared
/// inputs, so the one failure mode left is the *call* omitting one or supplying
/// the wrong shape — reported by name, since that is squarely the caller's (the
/// model's) mistake to see and correct.
pub(super) fn fill_get_template(recipe: &Recipe, input: &Value) -> Result<String, String> {
    let mut url = recipe.get.clone();
    for i in &recipe.inputs {
        let Some(value) = input.get(&i.name) else {
            return Err(format!("missing input '{}'", i.name));
        };
        let filled = match i.kind {
            RecipeInputKind::Text => value
                .as_str()
                .map(str::to_owned)
                .ok_or_else(|| format!("'{}' must be text", i.name))?,
            RecipeInputKind::Number => {
                if !value.is_number() {
                    return Err(format!("'{}' must be a number", i.name));
                }
                value.to_string()
            }
        };
        url = url.replace(&format!("{{{}}}", i.name), &urlencode(&filled));
    }
    Ok(url)
}

/// What a draft recipe did when tried, before anyone commits to it (ADR 0071).
///
/// The authoring loop this exists for: fetch it once, see the real answer, pick
/// the field you want out of it, and read the sentence back — rather than
/// hand-writing a JSON path against an API you are guessing about.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RecipeTrial {
    /// The address actually fetched, with the sample values filled in.
    pub url: String,
    /// Every path `say` could address, with what this response had there.
    pub fields: Vec<(String, String)>,
    /// The sentence as it reads, when every placeholder resolved.
    pub preview: Option<String>,
    /// Why it did not, when one did not.
    pub preview_error: Option<String>,
    /// The response held a list. Recipes read objects only, so those fields are
    /// not offered — said plainly, because "my field isn't there" otherwise looks
    /// like a bug rather than the format's edge.
    pub skipped_a_list: bool,
}

/// The most fields offered from one response. A long tail of paths is a wall,
/// not a chooser, and anything past this is reachable by typing the path.
pub(super) const MOST_FIELDS_OFFERED: usize = 60;

/// Runs a draft recipe once and reports what came back (ADR 0071).
///
/// Deliberately the **same** path a saved recipe takes — the same validation, the
/// same guarded fetch, the same templating — so what you see when you try it is
/// what you get when you save it. A trial that took a shortcut would be a demo,
/// not evidence.
///
/// # Errors
/// The draft's own validation errors, or whatever the fetch reported.
pub fn try_recipe(
    inputs: Vec<RecipeInput>,
    get: &str,
    say: &str,
    values: &Value,
) -> Result<RecipeTrial, String> {
    // A placeholder id and description: a trial validates the URL and its slots,
    // which is what can be wrong at this stage. The real id is validated on save.
    let draft = Recipe::new("draft", "a draft", inputs, get, say).map_err(|e| e.to_string())?;
    let url = fill_get_template(&draft, values)?;
    let body = guarded_get_text(&url, 64 * 1024).map_err(|e| e.to_string())?;
    let response: Value =
        serde_json::from_str(&body).map_err(|_| "the response wasn't JSON".to_owned())?;
    let mut fields = Vec::new();
    let mut skipped_a_list = false;
    readable_paths(&response, "", &mut fields, &mut skipped_a_list);
    fields.truncate(MOST_FIELDS_OFFERED);
    let (preview, preview_error) = match fill_say_template(&draft.say, &response) {
        Ok(sentence) => (Some(sentence), None),
        Err(why) => (None, Some(why)),
    };
    Ok(RecipeTrial {
        url,
        fields,
        preview,
        preview_error,
        skipped_a_list,
    })
}

/// Every path in a response that a `say` template could actually address, with
/// the value found there.
///
/// Objects only, exactly matching [`fill_say_template`]'s reach: a path is a
/// sequence of object keys, so a field inside a list has no expressible path and
/// offering one would hand the person a template that cannot resolve. Nulls are
/// skipped for the same reason — the template treats them as missing.
pub(super) fn readable_paths(
    value: &Value,
    prefix: &str,
    out: &mut Vec<(String, String)>,
    skipped_a_list: &mut bool,
) {
    if out.len() >= MOST_FIELDS_OFFERED {
        return;
    }
    match value {
        Value::Object(map) => {
            for (key, child) in map {
                let path = if prefix.is_empty() {
                    key.clone()
                } else {
                    format!("{prefix}.{key}")
                };
                readable_paths(child, &path, out, skipped_a_list);
            }
        }
        Value::Array(items) => {
            if !items.is_empty() {
                *skipped_a_list = true;
            }
        }
        Value::Null => {}
        scalar => {
            if !prefix.is_empty() {
                let shown = match scalar {
                    Value::String(s) => s.clone(),
                    other => other.to_string(),
                };
                out.push((prefix.to_owned(), shown));
            }
        }
    }
}

/// Fills `say`'s `{a.b.c}` placeholders by walking that dotted path into the
/// response, stringifying whatever it finds. No expressions, no array indices —
/// a path is a sequence of object keys and nothing else, which is the whole
/// reason a recipe's response reading can be trusted not to hide a second
/// request: there is no syntax left for one to hide in.
pub(super) fn fill_say_template(say: &str, response: &Value) -> Result<String, String> {
    let mut out = say.to_owned();
    for path in placeholders_in(say) {
        let mut here = response;
        for step in path.split('.') {
            here = here
                .get(step)
                .ok_or_else(|| format!("the response didn't have '{path}'"))?;
        }
        let shown = match here {
            Value::String(s) => s.clone(),
            Value::Null => return Err(format!("the response didn't have '{path}'")),
            other => other.to_string(),
        };
        out = out.replace(&format!("{{{path}}}"), &shown);
    }
    Ok(out)
}
