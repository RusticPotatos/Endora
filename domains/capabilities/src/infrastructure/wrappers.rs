use super::*;

/// Passes one method of [`CapabilityRunner`](crate::application::CapabilityRunner) straight
/// through to a wrapped runner. See [`forwards_to_inner!`].
macro_rules! forward_one_to_inner {
    ($field:ident, about_the_person) => {
        fn about_the_person(&self) -> Vec<String> {
            self.$field.about_the_person()
        }
    };
    ($field:ident, current_states) => {
        fn current_states(&self) -> Vec<(String, String)> {
            self.$field.current_states()
        }
    };
    ($field:ident, decision) => {
        fn decision(&self, id: &str) -> Option<Decision> {
            self.$field.decision(id)
        }
    };
    ($field:ident, verifier) => {
        fn verifier(&self, id: &str) -> Option<String> {
            self.$field.verifier(id)
        }
    };
    ($field:ident, read_back_input) => {
        fn read_back_input(&self, action_id: &str, action_input: &str) -> String {
            self.$field.read_back_input(action_id, action_input)
        }
    };
}

/// Declares which of the port's **defaultable** methods a decorator simply passes along.
///
/// Runners are layered, and every method a wrapper does not answer itself has to be handed
/// down. Five of the seven have a default, and a default that returns nothing is
/// **indistinguishable from a service having nothing to say** — so forgetting one fails
/// silently and looks exactly like working. Two were forgotten this way: presence never
/// reached a turn and neither did the facts behind an answer, with every unit test passing
/// because they exercised the runner that answers rather than the stack production builds.
///
/// Two more were latent at the time of writing: `AliasRunner` and `OpenerRunner` never
/// passed presence or states along, and got away with it only because the runner *above*
/// them happened to answer first. Moving them in the stack would have broken it.
///
/// Writing it out per decorator is what made that possible. Here, adding a method to the
/// port means adding one arm to [`forward_one_to_inner!`], and every decorator that lists it
/// forwards it correctly by construction. What a decorator genuinely overrides it still
/// writes by hand, right next to this — so the difference between "changes this" and "passes
/// this along" is visible in one place.
macro_rules! forwards_to_inner {
    ($field:ident : $($method:ident),+ $(,)?) => {
        $( forward_one_to_inner!($field, $method); )+
    };
}

/// Fill in the person's place when a skill needs one and the request did not name it —
/// and put theirs back when the model named one **nobody asked about**.
///
/// Four briefs opened "here's your daily brief for New York" — a city nobody here lives in.
/// Three fixes tried to make the model *more likely* to remember before ADR 0065 took the
/// question away from it for the blank case. The eval then measured the case the fill
/// could not reach — `select:no-invented-place` — and the model, asked for weather with no
/// place named anywhere, invented a city on its first run. That is the trigger evidence
/// 0065 said would justify the sharper rule, so (amended 2026-08-04) here it is:
///
/// **A place counts as named only if the person's own words contain it.** "The weather in
/// Boston" keeps Boston, including as a follow-up whose earlier message named it — the
/// person's words travel in `the_person_said`. A city appearing from nowhere is the model
/// recalling, and recall is not a source (ADR 0068): it is replaced with where they live.
/// Coordinates are left alone — nobody types a latitude, so "did they say it?" cannot
/// arbitrate them, and a coordinate-shaped invention has never been observed.
#[must_use]
pub fn place_filled_in(
    input_json: &str,
    wants_place: bool,
    where_they_are: &str,
    the_person_said: &str,
) -> String {
    if !wants_place || where_they_are.is_empty() {
        return input_json.to_string();
    }
    let Ok(serde_json::Value::Object(mut args)) = serde_json::from_str(input_json) else {
        // Not an object: there is no field to fill. Left exactly as it came, so a malformed
        // call fails the way it would have anyway rather than failing differently here.
        return input_json.to_string();
    };
    // Coordinates are a place spelled in a way nobody speaks; the model may hold them
    // for its own reasons and the person's words cannot confirm or deny them.
    let coordinates = ["lat", "latitude"].iter().any(|k| {
        args.get(*k)
            .is_some_and(|v| !matches!(v, serde_json::Value::Null))
    });
    if coordinates {
        return input_json.to_string();
    }
    let said = the_person_said.to_lowercase();
    let mut replaced = false;
    for key in ["location", "place", "city", "q"] {
        let Some(serde_json::Value::String(value)) = args.get(key) else {
            continue;
        };
        let value = value.trim().to_owned();
        if value.is_empty() {
            continue;
        }
        if said.contains(&value.to_lowercase()) {
            // The person named it — theirs to mean, never overwritten.
            return input_json.to_string();
        }
        // Named by nobody: the model is recalling. Replaced, not trusted.
        args.insert(
            key.to_owned(),
            serde_json::Value::String(where_they_are.to_owned()),
        );
        replaced = true;
    }
    if !replaced {
        args.insert(
            "location".to_owned(),
            serde_json::Value::String(where_they_are.to_owned()),
        );
    }
    serde_json::Value::Object(args).to_string()
}

/// Reduces a call's arguments to what they **mean**, so that two spellings of one call
/// are recognised as one call (ADR 0053).
///
/// The turn loop refuses to run the same tool with the same input twice, and was being
/// beaten by punctuation and key order. Observed live in one morning briefing: two
/// identical requests for the whole house, and before them two identical failing attempts
/// to get weather out of the smart home — four rounds spent, of which two were free, on a
/// turn whose answer then had no room left to be any good.
///
/// Key order is not incidental here: the same model emits
/// `{"area":"","domain":["light"],"name":""}` and
/// `{"domain":["light"],"name":"","area":""}` for the same intent, run to run.
///
/// Arguments that will not parse fall back to the raw text — exactly what the guard
/// compared before, so nothing gets worse.
#[must_use]
pub fn same_call_as(input_json: &str) -> String {
    // `serde_json`'s map is ordered by key unless `preserve_order` is enabled, which it is
    // not here, so re-serialising a parsed value is already canonical.
    serde_json::from_str::<Value>(input_json)
        .as_ref()
        .map_or_else(|_| input_json.trim().to_owned(), Value::to_string)
}

/// Nudges tool arguments toward the tool's input schema for the common small-model
/// slip of passing a scalar where an array is wanted: for each top-level property the
/// schema types as `array`, a non-array value is wrapped in a one-element array. Only
/// that one coercion — everything else is passed through untouched, and any parse
/// failure returns the input unchanged (the server still validates).
/// `widening_is_harmless` is true for a **state reader**: the widest read is the
/// reader's whole job, so a call whose every target came out null is cleaned into the
/// unscoped read rather than left for the server to reject. An actuator never gets
/// that: cleaning its nulls once turned on every light in the house (see the guard
/// at the end).
pub(super) fn coerce_args_to_schema(
    input_json: &str,
    schema: Option<&serde_json::Value>,
    widening_is_harmless: bool,
) -> String {
    let Some(schema) = schema else {
        return input_json.to_owned();
    };
    let Some(props) = schema.get("properties").and_then(|p| p.as_object()) else {
        return input_json.to_owned();
    };
    let Ok(mut args) = serde_json::from_str::<serde_json::Value>(input_json) else {
        return input_json.to_owned();
    };
    let Some(obj) = args.as_object_mut() else {
        return input_json.to_owned();
    };
    for (key, val) in obj.iter_mut() {
        let wants_array = props
            .get(key)
            .and_then(|s| s.get("type"))
            .and_then(serde_json::Value::as_str)
            == Some("array");
        if wants_array && !val.is_array() && !val.is_null() {
            *val = serde_json::Value::Array(vec![val.take()]);
        }
        // Drop values the schema says are not allowed, rather than letting the server
        // reject the whole call. Observed live: `device_class: ["light"]` — not one of
        // that field's permitted values — failed the entire turn with a validation
        // error, so a light nobody could name stayed on.
        if let Some(allowed) = permitted_values(props.get(key)) {
            match val {
                serde_json::Value::Array(items) => {
                    items.retain(|i| i.as_str().is_some_and(|s| allowed.iter().any(|a| a == s)));
                }
                serde_json::Value::String(sv) if !allowed.iter().any(|a| a == sv) => {
                    *val = serde_json::Value::Null;
                }
                _ => {}
            }
        }
    }
    // Did the model TRY to aim at something? Recorded before the empties are dropped,
    // because that is the only moment the difference is visible.
    let aimed_at_something = obj.iter().any(|(_, v)| is_targeting_attempt(v));
    // A kind filter that merely repeats another one is a duplication, not a narrowing.
    // Live: HassTurnOff{area:"kitchen", domain:["switch"], device_class:["switch"]} —
    // the same word twice. `Kitchen Main Light` is domain `switch` with no matching
    // device_class, so the pair excluded everything and Home Assistant reported the
    // AREA as unmatched even though it exists.
    //
    // Dropping the duplicate cannot widen what the call can touch: the filter it
    // duplicates is still there, so the blast radius is unchanged. That is the whole
    // reason this is safe to do silently, where dropping a filter in general is not.
    drop_duplicated_kind_filters(obj);
    // An empty value is not a filter, it is noise — and some servers reject it outright
    // (`floor: ""` came back as "invalid slot info", failing the call). Dropping empties
    // also clears anything emptied by the enum check above.
    obj.retain(|_, v| match v {
        serde_json::Value::Null => false,
        serde_json::Value::String(s) => !s.trim().is_empty(),
        serde_json::Value::Array(items) => !items.is_empty(),
        _ => true,
    });
    // Dropping empties is what lets a slightly-malformed call through — but it must not
    // turn a call that aimed at ONE thing into a call that hits everything.
    //
    // Observed live: HassTurnOn{area:null, name:null, domain:["light"]} became
    // {domain:["light"]} and Home Assistant turned on every light in the house. The
    // model plainly meant to name something and sent nulls instead; the honest reading
    // of that is "it failed to say what", not "it meant all of them".
    //
    // So if every targeting value it gave was empty, the call is left alone rather than
    // cleaned into a house-wide one, and the server rejects it as it did before.
    //
    // Unless widening is harmless — a read. Live, the same nulls on the state reader
    // were faithfully passed through, rejected ("None is not of type 'string'"), and
    // an answerable question about the lights died in failure recovery.
    if !widening_is_harmless && aimed_at_something && !obj.iter().any(|(_, v)| v.is_string()) {
        return input_json.to_owned();
    }
    args.to_string()
}

/// Whether this value looks like an attempt to aim at something — a name, an area, a
/// floor — as opposed to a kind filter or a setting.
///
/// Scalars point at things and arrays restrict which kinds count, the same split the
/// read-back scoping uses. An empty or null scalar is an attempt that came out blank,
/// which is the case worth noticing.
pub(super) fn is_targeting_attempt(v: &Value) -> bool {
    v.is_null() || v.is_string()
}

/// Removes an array-valued filter whose values are already covered by another one.
///
/// Only exact duplicates go: `domain: ["switch"]` alongside `device_class: ["switch"]`
/// keeps one and drops the other, while `domain: ["light"]` alongside
/// `device_class: ["outlet"]` is a real narrowing and both stay. Scalars are never
/// touched — they name the target rather than restricting its kind.
///
/// The keeper is chosen by name order so the result is deterministic, and the filter
/// that survives constrains exactly as much as the pair did.
pub(super) fn drop_duplicated_kind_filters(obj: &mut serde_json::Map<String, serde_json::Value>) {
    let arrays: Vec<(String, Vec<String>)> = obj
        .iter()
        .filter_map(|(k, v)| {
            let items: Vec<String> = v
                .as_array()?
                .iter()
                .filter_map(|i| i.as_str().map(str::to_lowercase))
                .collect();
            (!items.is_empty()).then(|| (k.clone(), items))
        })
        .collect();
    let mut drop: Vec<String> = Vec::new();
    for (i, (key, values)) in arrays.iter().enumerate() {
        let duplicated_earlier = arrays[..i]
            .iter()
            .any(|(other, other_values)| other_values == values && !drop.contains(other));
        if duplicated_earlier {
            drop.push(key.clone());
        }
    }
    for key in drop {
        obj.remove(&key);
    }
}

/// The values a schema permits for a field, from `enum` — directly, or on an array's
/// `items`. `None` when the field is unconstrained, which is the common case.
///
/// Schema-driven on purpose: this is what lets Endora keep a model's slips from failing
/// a whole call without knowing anything about the server it is talking to (ADR 0054).
pub(super) fn permitted_values(field: Option<&serde_json::Value>) -> Option<Vec<String>> {
    let values = collect_enum(field?);
    (!values.is_empty()).then_some(values)
}

/// Every value an `enum` anywhere in this schema fragment permits.
///
/// A schema says "one of these" in more shapes than one: directly, on an array's `items`,
/// and — the shape that got through — nested inside `anyOf` / `oneOf` / `allOf`, which is
/// how a generated schema usually expresses "a string from this list, or null".
///
/// Looking only at the top two levels missed it, so a value the server would reject was
/// sent anyway. Live: `device_class: ["light"]` — not a device class, and Home Assistant
/// answered `'light' is not one of ['awning', 'blind', 'curtain', …]`, failing the whole
/// call. Endora had the list in the tool's own schema the entire time.
pub(super) fn collect_enum(field: &serde_json::Value) -> Vec<String> {
    let mut values: Vec<String> = Vec::new();
    if let Some(list) = field.get("enum").and_then(serde_json::Value::as_array) {
        values.extend(
            list.iter()
                .filter_map(|v| v.as_str().map(ToOwned::to_owned)),
        );
    }
    for key in ["items", "anyOf", "oneOf", "allOf", "prefixItems"] {
        match field.get(key) {
            Some(serde_json::Value::Array(branches)) => {
                for branch in branches {
                    values.extend(collect_enum(branch));
                }
            }
            Some(nested) => values.extend(collect_enum(nested)),
            None => {}
        }
    }
    values.sort();
    values.dedup();
    values
}

/// Merges several [`CapabilityRunner`] sources — the built-in registry and any MCP
/// servers — behind the single runner interface (ADR 0054). The application still
/// speaks to one `CapabilityRunner` and never learns a tool's origin. Ids are unique
/// across sources by construction (built-ins carry no dot; MCP tools are
/// `server.tool`), so the first source that lists an id owns it.
pub struct CompositeRunner {
    sources: Vec<Arc<dyn CapabilityRunner + Send + Sync>>,
}

impl CompositeRunner {
    /// Merges the given sources, consulted in order. Sources are [`Arc`] so a
    /// long-lived one (e.g. connected MCP servers) can be shared across turns while a
    /// fresh per-turn source (the config-bound registry runner) sits beside it.
    #[must_use]
    pub fn new(sources: Vec<Arc<dyn CapabilityRunner + Send + Sync>>) -> Self {
        Self { sources }
    }

    /// The source that lists `id` in its catalog, if any.
    fn owner(&self, id: &str) -> Option<&(dyn CapabilityRunner + Send + Sync)> {
        self.sources
            .iter()
            .find(|s| s.available().iter().any(|spec| spec.id == id))
            .map(AsRef::as_ref)
    }
}

impl CapabilityRunner for CompositeRunner {
    fn run_observed(
        &self,
        id: &str,
        input: &str,
    ) -> Result<crate::application::CapabilityRead, String> {
        self.owner(id)
            .ok_or_else(|| format!("no such skill '{id}'"))?
            .run_observed(id, input)
    }
    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.sources.iter().flat_map(|s| s.available()).collect()
    }

    fn about_the_person(&self) -> Vec<String> {
        self.sources
            .iter()
            .flat_map(|s| s.about_the_person())
            .collect()
    }

    fn current_states(&self) -> Vec<(String, String)> {
        self.sources
            .iter()
            .flat_map(|s| s.current_states())
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        self.owner(id)?.decision(id)
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        match self.owner(id) {
            Some(source) => source.run(id, input_json),
            None => Err(format!("no such skill '{id}'")),
        }
    }

    fn verifier(&self, id: &str) -> Option<String> {
        self.owner(id)?.verifier(id)
    }

    fn read_back_input(&self, action_id: &str, action_input: &str) -> String {
        self.owner(action_id).map_or_else(
            || "{}".to_owned(),
            |o| o.read_back_input(action_id, action_input),
        )
    }
}

/// Retries a **failed** call once with the person's confirmed target aliases applied
/// (ADR 0054).
///
/// Endora asks what a target is really called; the answer used to reach the model as
/// context and nothing more. Measured, that does not work: with `"table" means "Kitchen
/// Table"` in its prompt, the model still sent `name: "table"` and Home Assistant still
/// answered `no_match_reason=NAME`. Grounding is advice, and this model takes advice
/// about a third of the time.
///
/// So the alias becomes deterministic — but only as **recovery**, never as pre-emption:
///
/// - it fires only after a call has already **failed**, so it can never hijack a working
///   one or redirect an action that was about to hit the right thing;
/// - it uses only what the person **confirmed**, which is the authoritative source in
///   ADR 0054's ranking;
/// - it retries **once**;
/// - and it **says so** in the result, so the substitution is visible to the model, to
///   the outcome record, and to the person in the disclosure.
///
/// That last point is what keeps it honest. The model's mistake still happened, is still
/// recorded, and is still measurable — the eval sees the same failure it always did. What
/// changes is that the person's light comes on.
pub struct AliasRunner {
    inner: Arc<dyn CapabilityRunner + Send + Sync>,
    /// `(server, said, means)`, as the person confirmed them.
    aliases: Vec<(String, String, String)>,
}

impl AliasRunner {
    /// Wraps `inner` with the confirmed aliases.
    #[must_use]
    pub fn new(
        inner: Arc<dyn CapabilityRunner + Send + Sync>,
        aliases: Vec<(String, String, String)>,
    ) -> Self {
        Self { inner, aliases }
    }

    /// The input with any confirmed alias applied, or `None` when nothing matched.
    ///
    /// Matches a whole string **value** only — never a fragment, and never a field name.
    /// Replacing inside a value would let `"table"` rewrite `"comfortable"`, and a person
    /// who said what one thing is called has not licensed edits to everything containing
    /// that word.
    pub(super) fn apply(&self, id: &str, input_json: &str) -> Option<String> {
        let server = id.split_once('.').map(|(s, _)| s)?;
        let mut args: Value = serde_json::from_str(input_json).ok()?;
        let obj = args.as_object_mut()?;
        let mut resolved: Vec<String> = Vec::new();
        for (field, value) in obj.iter_mut() {
            let Some(text) = value.as_str() else { continue };
            let hit = self
                .aliases
                .iter()
                .find(|(srv, said, _)| srv == server && said.eq_ignore_ascii_case(text.trim()));
            if let Some((_, _, means)) = hit {
                *value = Value::String(means.clone());
                resolved.push(field.clone());
            }
        }
        if resolved.is_empty() {
            return None;
        }
        // The person's answer outranks the model's other guesses (ADR 0054's ranking).
        //
        // Observed live, with the alias in place and still failing:
        //   {name: "table light", area: "Living Room", floor: "1"} -> INVALID_FLOOR
        // The kitchen light was put on an invented floor in the wrong room, so the
        // substitution never got as far as being tried. A confirmed name identifies one
        // thing on its own; every other scalar the model supplied is a guess about the
        // same target, and keeping them can only contradict the answer.
        //
        // Kind filters are kept — they restrict which sorts of thing count rather than
        // claiming which one it is, and dropping them would widen the call.
        obj.retain(|field, value| resolved.contains(field) || !value.is_string());
        Some(args.to_string())
    }
}

impl CapabilityRunner for AliasRunner {
    // Only rewrites the arguments of a call; everything else passes through. The
    // first two were missing here and got away with it because the runner above
    // answered them first.
    forwards_to_inner!(inner: about_the_person, current_states, decision, verifier, read_back_input);

    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.inner.available()
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        self.run_observed(id, input_json).map(|read| read.text)
    }

    fn run_observed(
        &self,
        id: &str,
        input_json: &str,
    ) -> Result<crate::application::CapabilityRead, String> {
        let first = self.inner.run_observed(id, input_json);
        let Err(original) = first else {
            return first;
        };
        let Some(retry_input) = self.apply(id, input_json) else {
            return Err(original);
        };
        match self.inner.run_observed(id, &retry_input) {
            // Say what was substituted. The model answers from this, the outcome record
            // keeps it, and the person sees it — nothing about the recovery is hidden.
            Ok(mut out) => {
                out.text = format!(
                    "(Endora retried using the name you gave it: {retry_input}. The first attempt failed.)\n{}",
                    out.text
                );
                Ok(out)
            }
            // The alias did not help either; the original failure is the honest answer.
            Err(_) => Err(original),
        }
    }
}

/// Overlays the person's stances — and what the record has proven — onto the shared MCP
/// runner (ADR 0062), so a tool's word applies without rebuilding the connection.
///
/// An MCP tool's default is `off`: a server's self-report is not evidence, so an unvetted
/// tool is blocked outright. `ask` confirms each use — and graduates to acting on its own
/// when the record has proven the tool AND the person's envelope allows consequential
/// actions: the same two deliberate gates as before, with the record standing where a
/// stored "opened" flag used to.
pub struct OpenerRunner {
    inner: Arc<dyn CapabilityRunner + Send + Sync>,
    /// The servers Endora has a relationship with rather than answers from (ADR 0058).
    /// Their readings are the house, not a stranger's words (ADR 0064).
    the_house: std::collections::HashSet<String>,
    stances: std::collections::HashMap<String, Stance>,
    /// Tools with enough read-back confirmed changes (ADR 0062). Derived, never stored.
    proven: std::collections::HashSet<String>,
    /// The envelope's consequential dial — graduation's second gate.
    auto_consequential: bool,
}

impl OpenerRunner {
    /// Overlays `stances` and `proven` onto `inner`.
    #[must_use]
    pub fn new(
        inner: Arc<dyn CapabilityRunner + Send + Sync>,
        the_house: std::collections::HashSet<String>,
        stances: std::collections::HashMap<String, Stance>,
        proven: std::collections::HashSet<String>,
        auto_consequential: bool,
    ) -> Self {
        Self {
            inner,
            the_house,
            stances,
            proven,
            auto_consequential,
        }
    }

    /// A tool's stance: the person's word, or its band's default.
    fn stance_of(&self, id: &str, band: Reversibility) -> Stance {
        self.stances
            .get(id)
            .copied()
            .unwrap_or_else(|| default_stance(band))
    }

    /// Ask's graduation (ADR 0062): proven by read-back, and the envelope allows it.
    fn graduates(&self, id: &str) -> bool {
        self.proven.contains(id) && self.auto_consequential
    }
}

impl CapabilityRunner for OpenerRunner {
    // Only widens what may run; it has no opinion on what the services can see.
    forwards_to_inner!(inner: about_the_person, current_states, verifier, read_back_input);

    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.inner
            .available()
            .into_iter()
            .map(|mut spec| {
                // A server Endora has a relationship with is the house speaking about
                // itself, not a stranger (ADR 0064).
                if spec
                    .id
                    .split_once('.')
                    .is_some_and(|(server, _)| self.the_house.contains(server))
                {
                    spec.third_party = false;
                }
                match self.stance_of(&spec.id, spec.reversibility) {
                    Stance::Off => spec.autonomous = false,
                    // Graduation is an actuator's gate: `proven` means read-back
                    // confirmed CHANGES (ADR 0062), which a read can never produce.
                    // Applying it to the observe band left a nominated reader
                    // permanently non-autonomous the moment anything stored an `ask`
                    // for it (trust_all does, for every undecided tool) — and a
                    // butler that may not look answers from imagination. Seen live.
                    Stance::Ask if spec.reversibility == Reversibility::Observe => {
                        spec.autonomous = true;
                    }
                    Stance::Ask => spec.autonomous = self.graduates(&spec.id),
                    Stance::Auto => spec.autonomous = true,
                }
                spec
            })
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        // The service below is always asked — its own verdict must never be silently
        // swallowed by an overlay. The stance then narrows or widens it: `off` blocks
        // whatever was said below; `ask` is the person's allow-with-confirm, which is
        // precisely what may override a deny-by-default Block (and graduates when the
        // record and the envelope both say so); `auto` keeps whatever the service said,
        // so a stricter verdict below survives.
        let below = self.inner.decision(id);
        let band = self
            .inner
            .available()
            .into_iter()
            .find(|s| s.id == id)
            .map(|s| s.reversibility);
        let Some(band) = band else {
            return below;
        };
        Some(match self.stance_of(id, band) {
            Stance::Off => Decision::Block,
            // On the observe band `ask` means what `auto` means: confirming a read
            // blocks the very question it exists to answer, and graduation (proven
            // changes) is unsatisfiable for something that changes nothing.
            Stance::Ask if band == Reversibility::Observe => below.unwrap_or(Decision::Act),
            Stance::Ask => {
                if self.graduates(id) {
                    Decision::Act
                } else {
                    Decision::Confirm
                }
            }
            Stance::Auto => below.unwrap_or(Decision::Act),
        })
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        self.run_observed(id, input_json).map(|read| read.text)
    }

    fn run_observed(
        &self,
        id: &str,
        input_json: &str,
    ) -> Result<crate::application::CapabilityRead, String> {
        // Deny-by-default at the run layer too: `off` never runs, even on a direct call.
        if self.decision(id) == Some(Decision::Block) {
            return Err(format!(
                "'{id}' isn't allowed yet — allow it under Skills first (it will still \
                 confirm every use until it has proven itself)"
            ));
        }
        self.inner.run_observed(id, input_json)
    }
}

/// A wrapper for the butler's **unattended** turns — the heartbeat's check-in, daily
/// brief, and nightly loop — that clamps autonomy to the *reversible* bands.
///
/// The person's levers ([`OpenerRunner`], the autonomy envelope) answer "may Endora do
/// this **when I am here**": opening a tool and widening the envelope together clear an
/// irreversible capability to run inside a chat turn, where the person is present, sees
/// the activity trail, and can say stop. None of that is true at 03:00 while they sleep.
///
/// So an unattended turn gets a narrower catalog: anything above
/// [`Reversibility::Reversible`] loses `autonomous` and its [`Act`](Decision::Act)
/// verdict drops to [`Confirm`](Decision::Confirm) — there is nobody to confirm, so in
/// practice it simply does not run, and the butler is handed a factual tool result
/// saying so. `Observe` and `Reversible` capabilities are untouched, which is what lets
/// the nightly loop still research, draft, and form beliefs (ADR 0051).
///
/// This makes [`run_due_nightly_loop`]'s documented guarantee — "nothing here it could
/// do that it couldn't undo" — true in code rather than only in prose. Before this, that
/// claim held only while the envelope happened to be closed.
///
/// Deny-by-default: a capability whose band cannot be read is assumed to be an actuator,
/// the same rule ADR 0053 applies to verification.
pub struct ReversibleOnlyRunner {
    inner: Arc<dyn CapabilityRunner + Send + Sync>,
}

impl ReversibleOnlyRunner {
    /// Wraps `inner` so only reversible capabilities may act without a person.
    #[must_use]
    pub fn new(inner: Arc<dyn CapabilityRunner + Send + Sync>) -> Self {
        Self { inner }
    }

    /// Whether this capability may run with nobody there — `Observe` and `Reversible`
    /// only. An id with no visible band is treated as an actuator, so it may not.
    fn may_run_unattended(&self, id: &str) -> bool {
        self.inner
            .available()
            .into_iter()
            .find(|s| s.id == id)
            .is_some_and(|s| s.reversibility <= Reversibility::Reversible)
    }
}

impl CapabilityRunner for ReversibleOnlyRunner {
    // Unattended turns narrow what may ACT. Seeing is not acting.
    forwards_to_inner!(inner: about_the_person, current_states, verifier, read_back_input);

    // Clamping what may ACT unattended says nothing about what may be known. Presence
    // matters most on exactly these turns — whether anyone is home is half of whether to
    // speak at all — so dropping it here would have been the worst place to drop it.

    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.inner
            .available()
            .into_iter()
            .map(|mut spec| {
                if spec.reversibility > Reversibility::Reversible {
                    spec.autonomous = false;
                }
                spec
            })
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        match self.inner.decision(id)? {
            // Cleared to act when someone is present, but this is not that: fall back
            // to needing a person, who is by definition not here to give it.
            Decision::Act if !self.may_run_unattended(id) => Some(Decision::Confirm),
            other => Some(other),
        }
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        self.run_observed(id, input_json).map(|read| read.text)
    }

    fn run_observed(
        &self,
        id: &str,
        input_json: &str,
    ) -> Result<crate::application::CapabilityRead, String> {
        // Refused at the run layer too, so a direct call can't route around the
        // narrowed catalog above.
        if !self.may_run_unattended(id) {
            return Err(format!(
                "'{id}' can't run unattended — it changes something Endora couldn't take \
                 back, and nobody is here to confirm it"
            ));
        }
        self.inner.run_observed(id, input_json)
    }
}

/// Searches a server's own reading for the target a call failed to name, and retries when
/// exactly one thing matches (ADR 0054).
///
/// A failed action already causes Endora to read the state back (ADR 0053), so the list of
/// names that really exist is in hand at the moment it is most useful. Until now that
/// whole reading was handed to the model, which then had to find one line in it and copy
/// it exactly. Measured across fourteen consecutive attempts at a light called
/// `Kitchen Table` — with `names: Kitchen Table` in the reading every time — it never once
/// sent that string. It permuted the *arguments* instead: `domain` flipped between `light`
/// and `switch`, an invented floor, the noun moved into `device_class`.
///
/// So the search runs here, in code:
///
/// - **always**, a shortlist of resembling names replaces "here is the whole house", so
///   the model copies from three lines instead of searching five kilobytes;
/// - **only when exactly one** candidate contains every word the call was aiming at, the
///   call is retried against it. Two plausible names is a guess, and a guess that actuates
///   something is what ADR 0051 exists to prevent.
///
/// Recovery-only, like [`AliasRunner`] and for the same reasons: it cannot hijack a
/// working call, it is bounded, and it says what it did so the model, the outcome record
/// and the person all see the substitution.
///
/// It never widens a call. Kind filters are kept; only scalars the real name already
/// contains are dropped, because `area: "kitchen"` adds nothing to `name: "Kitchen Table"`.
pub struct TargetSearchRunner {
    inner: Arc<dyn CapabilityRunner + Send + Sync>,
    /// Direct reach into a server, by server name (ADR 0054). Where one exists, it is
    /// both the better reading — everything, not only what the tool surface exposes —
    /// and the better way to act, because an id cannot be mis-matched.
    channels: Vec<(String, Arc<dyn NativeChannel>)>,
}

/// How many places a real name may be tried. The call does not say which of its fields is
/// the name — that would be per-server knowledge — so a couple of placements are tried and
/// the first that works wins. A placement that is wrong fails to match and changes
/// nothing, which is why searching this way is safe.
pub(super) const MAX_PLACEMENTS: usize = 3;

impl TargetSearchRunner {
    /// Wraps `inner` so failed calls search its own reading before giving up.
    #[must_use]
    pub fn new(inner: Arc<dyn CapabilityRunner + Send + Sync>) -> Self {
        Self {
            inner,
            channels: Vec::new(),
        }
    }

    /// Wraps `inner`, with direct reach into the named servers (ADR 0054).
    #[must_use]
    pub fn with_channels(
        inner: Arc<dyn CapabilityRunner + Send + Sync>,
        channels: Vec<(String, Arc<dyn NativeChannel>)>,
    ) -> Self {
        Self { inner, channels }
    }

    /// The direct reach for whichever server owns `id`, if any.
    fn channel(&self, id: &str) -> Option<&Arc<dyn NativeChannel>> {
        let (server, _) = id.split_once('.')?;
        self.channels
            .iter()
            .find(|(name, _)| name == server)
            .map(|(_, channel)| channel)
    }

    /// This server's state as it is right now, via the tool the person nominated as its
    /// reader (ADR 0054). `None` when nobody nominated one — the honest silence, and the
    /// reason this whole mechanism needs no per-server code.
    fn reading(&self, id: &str) -> Option<String> {
        // Direct reach first: it reports everything the service knows, where the tool
        // surface reports only what that surface was configured to expose.
        if let Some(channel) = self.channel(id) {
            if let Ok(reading) = channel.reading() {
                return Some(reading);
            }
        }
        let verifier = self.inner.verifier(id)?;
        if verifier == id {
            return None; // a read that failed has nothing to look itself up in
        }
        self.inner.run(&verifier, "{}").ok()
    }

    /// The one candidate among the joint-best that the tool could actually operate.
    ///
    /// Live: "turn off the kitchen main light" tied three ways — `Kitchen Main Light`, its
    /// `LED` configuration entry, and its `Cloud connection` indicator. All three carry
    /// the device's name, and two of them are not controls at all. Reading the tie as
    /// genuine ambiguity meant refusing a request that had exactly one sensible answer.
    ///
    /// Only ever **narrows a tie**: it cannot overrule a clear winner, and if more than
    /// one operable thing remains the ambiguity was real and nothing is acted on.
    fn only_one_that_can_be_acted_on<'a>(
        &self,
        tool: &str,
        found: &'a [crate::target_search::Candidate],
    ) -> Option<&'a crate::target_search::Candidate> {
        let channel = self.channel(tool)?;
        let best = found.first()?;
        let tied: Vec<&crate::target_search::Candidate> =
            found.iter().filter(|c| c.matched == best.matched).collect();
        if tied.len() < 2 {
            return None; // not a tie; `only_real_match` already had its say
        }
        let known = channel.known().ok()?;
        let mut operable = tied.into_iter().filter(|candidate| {
            known
                .iter()
                .find(|(_, name)| name.eq_ignore_ascii_case(&candidate.value))
                .is_some_and(|(id, _)| channel.actionable(tool, id))
        });
        let only = operable.next()?;
        operable.next().is_none().then_some(only)
    }

    /// Acts on the matched name through the server's own interface, by id.
    ///
    /// `None` when this channel cannot express the tool, or does not know the name — the
    /// caller falls back to retrying the tool itself, so direct reach is an improvement
    /// and never a new way to fail.
    fn act_directly(
        &self,
        channel: &Arc<dyn NativeChannel>,
        tool: &str,
        name: &str,
    ) -> Option<Result<String, String>> {
        let known = channel.known().ok()?;
        let (entity, _) = known
            .iter()
            .find(|(_, known_name)| known_name.eq_ignore_ascii_case(name))?;
        match channel.act(tool, entity)? {
            // Say so. A person reading the trail should see that Endora went around the
            // tool surface, and exactly what it acted on (ADR 0053).
            Ok(out) => Some(Ok(format!(
                "(The first attempt failed. Endora looked up what actually exists and \
                 acted on '{name}' directly, as {entity}.)\n{out}"
            ))),
            // Falling back is the honest move: the tool retry may still work.
            Err(_) => None,
        }
    }

    /// The fields a real name could be placed in.
    ///
    /// **Every** scalar field, not only the ones already holding a fragment of the name.
    /// A call does not say which of its fields means "the name" — working that out would
    /// be the per-server knowledge this avoids — and the live failure was exactly a call
    /// that had the right words in the wrong field:
    ///
    /// ```text
    /// {area: "guest bedroom left", name: "lamp"}   -> INVALID_AREA
    /// ```
    ///
    /// `Guest Bedroom Left` is an entity, not an area. Restricting placements to
    /// fragment-holders left nowhere to put it: `area` already held the whole name, and
    /// `name` held "lamp", a word the house does not use. So it found the answer and had
    /// no way to try it.
    ///
    /// Fragment-holders first, most specific first, then everything else alphabetically —
    /// likeliest placement first, deterministic throughout. A field already holding the
    /// exact name is skipped: that call is the one that just failed.
    fn placements(input_json: &str, name: &str) -> Vec<String> {
        let lowered = name.to_lowercase();
        let mut fields: Vec<(bool, usize, String)> =
            crate::target_search::target_fields(input_json)
                .into_iter()
                .filter(|(_, value)| !value.eq_ignore_ascii_case(name))
                .map(|(field, value)| {
                    let fragment = crate::target_search::is_fragment_of(&value, &lowered);
                    (fragment, value.split_whitespace().count(), field)
                })
                .collect();
        fields.sort_by(|a, b| {
            b.0.cmp(&a.0)
                .then_with(|| b.1.cmp(&a.1))
                .then_with(|| a.2.cmp(&b.2))
        });
        fields.into_iter().map(|(_, _, field)| field).collect()
    }
}

impl CapabilityRunner for TargetSearchRunner {
    fn run_observed(
        &self,
        id: &str,
        input: &str,
    ) -> Result<crate::application::CapabilityRead, String> {
        if self.channel(id).is_none() {
            return self.inner.run_observed(id, input);
        }
        self.run(id, input)
            .map(|text| crate::application::CapabilityRead {
                text,
                data: None,
                input: input.to_owned(),
            })
    }
    // Presence, states and the read-back scope are ANSWERED here, from this runner's own
    // channels, so they are written out below rather than forwarded.
    forwards_to_inner!(inner: decision, verifier);

    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.inner.available()
    }

    fn about_the_person(&self) -> Vec<String> {
        self.channels
            .iter()
            .filter_map(|(_, channel)| channel.about_the_person())
            .collect()
    }

    fn current_states(&self) -> Vec<(String, String)> {
        self.channels
            .iter()
            .flat_map(|(_, channel)| channel.states().unwrap_or_default())
            .collect()
    }

    fn read_back_input(&self, action_id: &str, action_input: &str) -> String {
        // Where there is direct reach, verification reads the WHOLE service, both before
        // and after (ADR 0053).
        //
        // Scoping the read to the action's arguments assumes those arguments name the
        // right thing — and this layer exists precisely because they often do not. Live:
        // a call aimed at "living room" was corrected to `light.kitchen_table` and acted
        // on it, while the read-back kept looking at the living room. Both readings were
        // identical, so a light the person watched come on was recorded as no change, and
        // the butler said it had failed.
        //
        // Reading everything cannot be aimed at the wrong thing. It can register an
        // unrelated change as this action's — which is the safe direction to be wrong,
        // since a false "nothing happened" is what causes false denials and nearly caused
        // the most useful tool in the house to be withdrawn.
        if self.channel(action_id).is_some() {
            return "{}".to_owned();
        }
        self.inner.read_back_input(action_id, action_input)
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        // An exact identifier pins the target; anything else in the call can only widen
        // it (ADR 0054). Done BEFORE the call, because a call that widens and succeeds is
        // never seen by the recovery path below.
        let tightened = self
            .channel(id)
            .and_then(|c| c.tighten(input_json))
            .unwrap_or_else(|| input_json.to_owned());
        let input_json = tightened.as_str();
        if let Some(why) = self.channel(id).and_then(|c| c.refuse(id, input_json)) {
            return Err(why);
        }
        let first = self.inner.run(id, input_json);
        let Err(original) = first else {
            return first;
        };
        let Some(reading) = self.reading(id) else {
            return Err(original);
        };
        // A kind filter the service has never used as a category is not a category — it
        // is part of what the person named (ADR 0054). Only a service that can say so
        // changes anything here.
        let words = match self.channel(id).and_then(|c| c.categories().ok()) {
            Some(known) if !known.is_empty() => {
                crate::target_search::target_words_with_kinds(input_json, &known)
            }
            _ => crate::target_search::target_words(input_json),
        };
        let found = crate::target_search::candidates(&reading, &words);
        // Only an unambiguous match may be acted on. Everything else is shown — unless
        // the tie is only between a thing and its own diagnostics, which is not a real
        // ambiguity (ADR 0054).
        let settled = crate::target_search::only_real_match(&found)
            .or_else(|| self.only_one_that_can_be_acted_on(id, &found));
        let Some(best) = settled else {
            return Err(format!(
                "{original}{}",
                crate::target_search::shortlist(&found)
            ));
        };
        // Direct reach, where it exists: resolve the name to the service's own id and
        // act on exactly that (ADR 0054). An id cannot be mis-matched, so this is the end
        // of the guessing rather than a better guess.
        if let Some(channel) = self.channel(id) {
            if let Some(result) = self.act_directly(channel, id, &best.value) {
                return result;
            }
        }
        for field in Self::placements(input_json, &best.value)
            .into_iter()
            .take(MAX_PLACEMENTS)
        {
            let retry = crate::target_search::retarget(input_json, &field, &best.value);
            if let Ok(out) = self.inner.run(id, &retry) {
                // Say what was substituted, exactly as the alias recovery does. Nothing
                // about this is hidden from the model, the record, or the person.
                return Ok(format!(
                    "(The first attempt failed. Endora looked up what actually exists and \
                     retried against '{}'.)\n{out}",
                    best.value
                ));
            }
        }
        Err(format!(
            "{original}{}",
            crate::target_search::shortlist(&found)
        ))
    }
}

/// Hides the capabilities the person has **withdrawn** — turned off — from every
/// source, and refuses to run them (ADR 0054).
///
/// Turning a skill off has always worked for built-ins, because [`RegistryRunner`]
/// applies the stored flag itself. MCP tools had no equivalent: the flag could be set
/// and nothing happened, because the tools come from a shared connection built long
/// before the person's config is read. This applies it once, above every source, so
/// "off" means the same thing whatever the capability is and wherever it came from.
///
/// **Why hiding rather than blocking.** A blocked tool is still in the catalogue, and
/// the measured failure is the model *choosing the wrong tool* — it picks a brightness
/// setter to switch a light, is refused, and picks it again. Refusing more loudly does
/// not help; the tool has to stop being on the menu. Blocking is the answer for a tool
/// that works and is dangerous; withdrawal is the answer for one that does not work.
///
/// Reversible by construction: it reads a stored flag, so restoring a capability is one
/// click and nothing was lost.
pub struct WithdrawnRunner {
    inner: Arc<dyn CapabilityRunner + Send + Sync>,
    withdrawn: std::collections::HashSet<String>,
}

impl WithdrawnRunner {
    /// Wraps `inner`, hiding the given ids. Ids that no source offers are simply never
    /// matched, so a stale flag from a removed server is harmless.
    #[must_use]
    pub fn new(
        inner: Arc<dyn CapabilityRunner + Send + Sync>,
        withdrawn: std::collections::HashSet<String>,
    ) -> Self {
        Self { inner, withdrawn }
    }
}

impl CapabilityRunner for WithdrawnRunner {
    // Withdrawal is about which TOOLS are offered. What the services know about the person
    // and about the world is not a tool and is not withheld.
    forwards_to_inner!(inner: about_the_person, current_states, read_back_input);

    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        self.inner
            .available()
            .into_iter()
            .filter(|spec| !self.withdrawn.contains(&spec.id))
            .collect()
    }

    fn decision(&self, id: &str) -> Option<Decision> {
        if self.withdrawn.contains(id) {
            return None;
        }
        self.inner.decision(id)
    }

    fn verifier(&self, id: &str) -> Option<String> {
        // A withdrawn tool must not be nominated as anything's verifier either: it would
        // be asked to read state it can no longer be run to read.
        let verifier = self.inner.verifier(id)?;
        if self.withdrawn.contains(&verifier) {
            return None;
        }
        Some(verifier)
    }

    fn run(&self, id: &str, input_json: &str) -> Result<String, String> {
        self.run_observed(id, input_json).map(|read| read.text)
    }

    fn run_observed(
        &self,
        id: &str,
        input_json: &str,
    ) -> Result<crate::application::CapabilityRead, String> {
        // Refused here too, so a model naming it from earlier in the conversation — or a
        // direct call — cannot route around the narrowed catalogue.

        if self.withdrawn.contains(id) {
            return Err(format!("'{id}' is turned off"));
        }
        self.inner.run_observed(id, input_json)
    }
}
