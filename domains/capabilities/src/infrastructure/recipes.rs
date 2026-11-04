use super::*;

// ---- Recipes: the person's own capabilities (ADR 0071) ---------------------

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
