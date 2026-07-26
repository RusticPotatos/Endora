//! Searching a server's own reading for the target a call failed to name (ADR 0041).
//!
//! When an action fails to match a name, the most useful thing in the world is the list
//! of names that *do* exist — and Endora already holds it, because a failed action reads
//! the state back (ADR 0034). Until now it handed the model the whole reading and hoped.
//!
//! Measured, that hope is misplaced. Across fourteen consecutive attempts at a light
//! called `Kitchen Table`, with `names: Kitchen Table` present in the reading every
//! single time, the model never once sent that string. It searched the *argument* space
//! instead — flipping `domain` between `light` and `switch`, inventing floors, moving the
//! noun into `device_class` — because finding one line in five kilobytes of unbroken text
//! and copying it exactly is the thing this model is worst at.
//!
//! So the search becomes code. Everything here is **pure text**: it knows nothing about
//! Home Assistant, YAML, or any server's schema. A reading is lines, a line's value is
//! what follows its colon, and a candidate is a value sharing words with what was asked
//! for. The same functions serve a calendar whose event is really "Endora Syncup" and a
//! filesystem whose path was misspelled.

/// How many candidates are worth showing. A shortlist the model can copy from, not a
/// second haystack.
const SHORTLIST: usize = 5;

/// A name found in a server's reading that resembles what a failed call was aiming at.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Candidate {
    /// The value exactly as it appears in the reading — what a retry must send.
    pub value: String,
    /// How many of the asked-for words it contains.
    pub matched: usize,
    /// Whether it contains **every** word the call was aiming at. Only an unambiguous
    /// exact-and-complete match is strong enough to retry on its own.
    pub complete: bool,
}

/// The words a call was aiming at: the values of its **scalar string** fields, split into
/// words and lowercased.
///
/// Arrays are skipped for the reason the read-back skips them (ADR 0034): a scalar points
/// at something, an array restricts which kinds count. `domain: ["light"]` is not part of
/// what was aimed at, and treating it as such would make every light a candidate.
#[must_use]
pub fn target_words(input_json: &str) -> Vec<String> {
    let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(input_json)
    else {
        return Vec::new();
    };
    let mut words: Vec<String> = obj
        .values()
        .filter_map(|v| v.as_str())
        .flat_map(str::split_whitespace)
        .map(str::to_lowercase)
        .filter(|w| w.chars().any(char::is_alphanumeric))
        .collect();
    words.sort();
    words.dedup();
    words
}

/// The scalar string fields a call carried, as `(field, value)` — the places a real name
/// could be put on a retry.
#[must_use]
pub fn target_fields(input_json: &str) -> Vec<(String, String)> {
    let Ok(serde_json::Value::Object(obj)) = serde_json::from_str::<serde_json::Value>(input_json)
    else {
        return Vec::new();
    };
    obj.into_iter()
        .filter_map(|(k, v)| {
            let s = v.as_str()?.trim();
            (!s.is_empty()).then(|| (k, s.to_owned()))
        })
        .collect()
}

/// Candidate names from a server's reading, best first.
///
/// Format-agnostic by construction. A reading is split into lines; a line's **value** is
/// whatever follows its first colon (or the whole line, if it has none), stripped of list
/// markers and quotes. That holds for YAML, for JSON, and for a bare list of names, and
/// it is the most this can assume without learning one server's shape.
///
/// Ranked by how many of the asked-for words each contains, then by brevity — a shorter
/// value containing the same words is the more precise answer.
#[must_use]
pub fn candidates(reading: &str, words: &[String]) -> Vec<Candidate> {
    if words.is_empty() {
        return Vec::new();
    }
    let fragments: Vec<String> = lines_of(reading).iter().map(|l| value_of(l)).collect();
    // Words the server never uses are the person's vocabulary, not the server's, and they
    // carry no power to tell one candidate from another. Live: "turn on the guest bedroom
    // left lamp" found `Guest Bedroom Left` and then refused to use it, because nothing in
    // the house is called a "lamp" and the match was therefore not "complete".
    //
    // Dropping them stays safe because completeness is not what makes a retry safe —
    // UNIQUENESS is. "turn on the garage lamp" still resolves to nothing actionable,
    // because dropping "lamp" leaves "garage", which matches several things.
    let words: Vec<String> = words
        .iter()
        .filter(|w| fragments.iter().any(|f| contains_word(f, w)))
        .cloned()
        .collect();
    if words.is_empty() {
        return Vec::new();
    }
    let mut found: Vec<Candidate> = Vec::new();
    for value in fragments {
        if value.is_empty() {
            continue;
        }
        let matched = words.iter().filter(|w| contains_word(&value, w)).count();
        if matched == 0 {
            continue;
        }
        if found.iter().any(|c| c.value.eq_ignore_ascii_case(&value)) {
            continue;
        }
        found.push(Candidate {
            matched,
            complete: matched == words.len(),
            value,
        });
    }
    found.sort_by(|a, b| {
        b.matched
            .cmp(&a.matched)
            .then_with(|| a.value.len().cmp(&b.value.len()))
            .then_with(|| a.value.cmp(&b.value))
    });
    found
}

/// The one candidate worth retrying on Endora's own initiative, if there is one.
///
/// Deliberately strict, because this is the half that **acts**: exactly one candidate may
/// contain every word the call was aiming at. Two plausible names is not a search result,
/// it is a guess, and a guess that actuates something is the failure mode
/// [ADR 0024](../../docs/adr/0024-reversibility-bands.md) exists to prevent.
#[must_use]
pub fn only_real_match(found: &[Candidate]) -> Option<&Candidate> {
    let mut complete = found.iter().filter(|c| c.complete);
    let first = complete.next()?;
    complete.next().is_none().then_some(first)
}

/// Rewrites a failed call to aim at `name`, by putting it in `field` and **dropping** the
/// other scalar fields whose words the name already contains.
///
/// Those fields are redundant rather than discarded: `area: "kitchen"` adds nothing to
/// `name: "Kitchen Table"`, and leaving it in is how a retry re-fails on a value that was
/// only ever a fragment of the real name.
///
/// Array fields are **kept untouched**. They narrow what the call can hit, and widening a
/// call during recovery is exactly how one action became every light in the house.
#[must_use]
pub fn retarget(input_json: &str, field: &str, name: &str) -> String {
    let Ok(serde_json::Value::Object(mut obj)) =
        serde_json::from_str::<serde_json::Value>(input_json)
    else {
        return input_json.to_owned();
    };
    let lowered = name.to_lowercase();
    obj.retain(|key, value| {
        if key == field {
            return true;
        }
        let Some(s) = value.as_str() else {
            return true; // arrays and numbers narrow the call; never widen on recovery
        };
        !is_fragment_of(s, &lowered)
    });
    obj.insert(field.to_owned(), serde_json::Value::String(name.to_owned()));
    serde_json::Value::Object(obj).to_string()
}

/// The shortlist to show when Endora cannot safely retry: what it looked for, and the
/// names in the reading that resemble it.
///
/// Replaces handing over the whole reading and hoping. Copying from three lines is a
/// different task from finding one line in five kilobytes.
#[must_use]
pub fn shortlist(found: &[Candidate]) -> String {
    if found.is_empty() {
        return String::new();
    }
    let names: Vec<String> = found
        .iter()
        .take(SHORTLIST)
        .map(|c| format!("  - {}", c.value))
        .collect();
    format!(
        "\n\n[candidates] that name did not match anything. These exist and look like \
         what you asked for — use one of them EXACTLY as written:\n{}",
        names.join("\n")
    )
}

/// Splits a reading into the fragments a value could live in.
///
/// Newlines first — including the literal two-character `\n`, which is not a nicety: a
/// server that returns its text inside JSON arrives as one unbroken line, and the live
/// Home Assistant reading is 5KB with zero real newlines and 255 escaped ones. Any server
/// that JSON-wraps its text has that shape, so this is a fact about transport rather than
/// about one integration.
///
/// Then on JSON structure, so a one-line document yields one fragment per field rather
/// than one fragment containing everything.
fn lines_of(reading: &str) -> Vec<String> {
    reading
        .replace("\\n", "\n")
        .split(['\n', ',', '{', '}', '[', ']'])
        .map(str::to_owned)
        .collect()
}

/// A line's value: whatever follows its first colon, minus list markers and quotes.
fn value_of(line: &str) -> String {
    let after = line.split_once(':').map_or(line, |(_, v)| v);
    after
        .trim()
        .trim_start_matches(['-', '*', '"', '\''])
        .trim_end_matches(['"', '\'', ',', '{', '}'])
        .trim()
        .to_owned()
}

/// Whether `value` is a **fragment** of `name` — every word of it appears in the name.
///
/// `"kitchen"` is a fragment of `"Kitchen Table"`, so a call carrying both is saying the
/// same thing twice; `"1st"` is not, so a floor filter survives a retry untouched.
#[must_use]
pub fn is_fragment_of(value: &str, name: &str) -> bool {
    let value = value.trim();
    !value.is_empty() && value.split_whitespace().all(|w| contains_word(name, w))
}

/// Whether `haystack` contains `word` as a **whole** word, case-insensitively.
///
/// Whole words matter: "table" must not match "Comfortable", and "on" must not match
/// every other name in the house.
fn contains_word(haystack: &str, word: &str) -> bool {
    haystack
        .split(|c: char| !c.is_alphanumeric())
        .any(|w| w.eq_ignore_ascii_case(word))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The live reading, in the shape it actually arrives in: JSON-wrapped, so every
    /// newline is an escaped two-character sequence.
    const LIVE: &str = "{\"success\": true, \"result\": \"Live Context:\\n\
        - names: Bedroom Main 1\\n  domain: light\\n  areas: Bedroom\\n\
        - names: Kitchen Bright\\n  domain: scene\\n  areas: Kitchen\\n\
        - names: Kitchen Main Light\\n  domain: light\\n  areas: Kitchen\\n\
        - names: Kitchen Table\\n  domain: light\\n  state: 'on'\\n  areas: Kitchen\\n\
        - names: Outside Color\\n  domain: light\\n  areas: Outside\\n\"}";

    #[test]
    fn it_finds_the_name_the_model_could_not() {
        // Fourteen attempts failed with this exact reading in context. The search that
        // was too hard for the model is one line of arithmetic here.
        let words = target_words(r#"{"name":"table","area":"kitchen","domain":["light"]}"#);
        assert_eq!(words, vec!["kitchen".to_owned(), "table".to_owned()]);
        let found = candidates(LIVE, &words);
        let best = only_real_match(&found).expect("no unambiguous match");
        assert_eq!(best.value, "Kitchen Table");
    }

    #[test]
    fn a_kind_filter_is_not_something_the_call_was_aiming_at() {
        // `domain: ["light"]` restricts which kinds count; it does not point at a thing.
        // Counting it would make every light in the house a candidate.
        let words = target_words(r#"{"name":"table","domain":["light"],"device_class":["a"]}"#);
        assert_eq!(words, vec!["table".to_owned()]);
    }

    #[test]
    fn two_plausible_names_are_a_guess_and_not_a_search_result() {
        // "kitchen" alone matches four things. Retrying on one of them would be acting on
        // a coin flip, so nothing is retried — the shortlist is shown instead.
        let words = target_words(r#"{"area":"kitchen"}"#);
        let found = candidates(LIVE, &words);
        assert!(found.len() > 1, "{found:?}");
        assert!(
            only_real_match(&found).is_none(),
            "acted on an ambiguous match: {found:?}"
        );
        assert!(shortlist(&found).contains("Kitchen Table"));
    }

    #[test]
    fn nothing_resembling_it_means_nothing_to_offer() {
        let words = target_words(r#"{"name":"greenhouse"}"#);
        assert!(candidates(LIVE, &words).is_empty());
        assert!(shortlist(&[]).is_empty());
    }

    #[test]
    fn the_retry_carries_the_exact_name_and_drops_what_it_already_contains() {
        // `area: "kitchen"` adds nothing to `name: "Kitchen Table"` — and leaving it in
        // is how the retry re-fails on a fragment of the real name.
        let out = retarget(
            r#"{"name":"table","area":"kitchen","domain":["light"]}"#,
            "name",
            "Kitchen Table",
        );
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["name"], "Kitchen Table");
        assert!(v.get("area").is_none(), "kept a redundant fragment: {out}");
        assert_eq!(v["domain"], serde_json::json!(["light"]));
    }

    #[test]
    fn recovery_never_widens_what_a_call_can_hit() {
        // The rule written in blood: one call became every light in the house. A retry
        // keeps every kind filter, and keeps scalars the name does NOT contain.
        let out = retarget(
            r#"{"name":"table","floor":"1st","domain":["light"]}"#,
            "name",
            "Kitchen Table",
        );
        let v: serde_json::Value = serde_json::from_str(&out).unwrap();
        assert_eq!(v["domain"], serde_json::json!(["light"]));
        assert_eq!(v["floor"], "1st", "dropped a filter it does not contain");
    }

    #[test]
    fn a_word_must_match_a_whole_word() {
        assert!(contains_word("Kitchen Table", "table"));
        assert!(!contains_word("Comfortable Chair", "table"));
    }

    #[test]
    fn it_reads_a_calendar_exactly_as_it_reads_a_house() {
        // The point of ADR 0041: no Home Assistant knowledge anywhere in here. A JSON
        // reading from any server is searched the same way.
        let reading = r#"{"events":[{"title": "Endora Syncup"},{"title": "Dentist"}]}"#;
        let words = target_words(r#"{"title":"endora sync-up"}"#);
        let found = candidates(reading, &words);
        assert_eq!(
            found.first().map(|c| c.value.as_str()),
            Some("Endora Syncup")
        );
    }

    #[test]
    fn a_bare_list_of_names_works_too() {
        let reading = "Kitchen Table\nKitchen Main Light\nGarage Main";
        let found = candidates(reading, &["table".to_owned()]);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].value, "Kitchen Table");
    }

    #[test]
    fn a_word_the_house_never_uses_does_not_block_a_clear_match() {
        // Live: "turn on the guest bedroom left lamp". The search found `Guest Bedroom
        // Left` and then refused to use it, because nothing in the house is called a
        // "lamp" so the match was not "complete". The person's vocabulary is not the
        // server's, and a word that appears nowhere cannot tell candidates apart.
        let reading = "- names: Guest Bedroom Left\n- names: Guest Bedroom\n- names: Garage Main";
        let words = target_words(r#"{"area":"guest bedroom left","name":"lamp"}"#);
        let found = candidates(reading, &words);
        let best = only_real_match(&found).expect("blocked by a word the house never uses");
        assert_eq!(best.value, "Guest Bedroom Left");
    }

    #[test]
    fn dropping_unknown_words_still_cannot_make_a_vague_request_actionable() {
        // What keeps that safe is UNIQUENESS, not completeness. "the garage lamp" loses
        // "lamp" and is left with "garage", which matches several things — so nothing is
        // retried, exactly as before.
        let reading = "- names: Garage\n- names: Garage Main\n- names: Garage Bright";
        let words = target_words(r#"{"name":"garage lamp"}"#);
        let found = candidates(reading, &words);
        assert!(found.len() > 1, "{found:?}");
        assert!(
            only_real_match(&found).is_none(),
            "acted on a vague request: {found:?}"
        );
    }

    #[test]
    fn a_request_sharing_no_words_at_all_finds_nothing() {
        let reading = "- names: Garage Main";
        let found = candidates(reading, &target_words(r#"{"name":"greenhouse mister"}"#));
        assert!(found.is_empty(), "{found:?}");
    }
}
