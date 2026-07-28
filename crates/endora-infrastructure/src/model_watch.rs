//! Watching for models worth knowing about (ADR 0055).
//!
//! The model layer's "discovery" only ever listed what was already pulled onto the box —
//! local inventory wearing discovery's name. It could never learn about a model nobody
//! had downloaded yet.
//!
//! This looks outward, and stops short of acting. It reports candidates and the exact
//! command to fetch one; it does not fetch anything. That line is
//! [0055](../../docs/adr/0055-the-model-layer.md)'s: **Endora does not host or manage the
//! model runtime.** Downloading gigabytes onto someone's machine is managing it, and a
//! background task that fills a disk is a far worse failure than a slow reply.
//!
//! It is also deliberately **pull, not push**: this runs when the person opens the models
//! screen. A weekly message saying "there are new models" is noise in an inbox meant for
//! things that matter.

use serde_json::Value;

/// A model that might be worth trying, as reported by the hub.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelSighting {
    /// The hub id, e.g. `bartowski/Qwen3-8B-GGUF`.
    pub id: String,
    /// Roughly how much VRAM it would want at 4-bit, in whole GB. An estimate, and
    /// labelled as one wherever it is shown.
    pub about_gb: u32,
    /// When the hub last saw it change, as an ISO date.
    pub updated: String,
    /// How many times it has been downloaded — the only popularity signal the hub gives
    /// cheaply, and a reasonable proxy for "someone has actually run this".
    pub downloads: u64,
    /// The command that would fetch it, for the person to run themselves.
    pub how_to_get_it: String,
}

/// How many to ask the hub for. Enough to filter down to a useful handful.
const ASK_FOR: usize = 60;

/// Models the hub knows about that would fit in `vram_gb`, newest first.
///
/// # Errors
/// A human-readable message if the hub cannot be reached or answers unexpectedly.
pub fn worth_knowing_about(vram_gb: u32) -> Result<Vec<ModelSighting>, String> {
    let url = format!(
        "https://huggingface.co/api/models?filter=gguf&sort=lastModified&direction=-1&limit={ASK_FOR}"
    );
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(std::time::Duration::from_secs(20)))
        .build()
        .into();
    let mut response = agent.get(&url).call().map_err(|e| e.to_string())?;
    if response.status().as_u16() >= 300 {
        return Err(format!("the hub returned status {}", response.status()));
    }
    let json: Value = response.body_mut().read_json().map_err(|e| e.to_string())?;
    Ok(sightings_from(&json, vram_gb))
}

/// Reads the hub's answer into candidates that fit. Split out so the filtering is
/// testable without a network.
#[must_use]
pub fn sightings_from(json: &Value, vram_gb: u32) -> Vec<ModelSighting> {
    let mut out: Vec<ModelSighting> = json
        .as_array()
        .unwrap_or(&Vec::new())
        .iter()
        .filter_map(|m| {
            let id = m["id"].as_str()?.to_owned();
            let about_gb = size_at_four_bits(&id)?;
            // Leave room for the context window and the runtime's own overhead: a model
            // that exactly fills the card spills into system memory and crawls, which is
            // the failure this is meant to help avoid rather than cause.
            (about_gb + 2 <= vram_gb).then(|| ModelSighting {
                how_to_get_it: format!("ollama pull hf.co/{id}"),
                id,
                about_gb,
                updated: m["lastModified"]
                    .as_str()
                    .unwrap_or_default()
                    .chars()
                    .take(10)
                    .collect(),
                downloads: m["downloads"].as_u64().unwrap_or_default(),
            })
        })
        .collect();
    // Most-used first among those that fit: recency got them into the list, and of the
    // recent ones the person wants the ones somebody has actually run.
    out.sort_by(|a, b| b.downloads.cmp(&a.downloads));
    out.dedup_by(|a, b| a.id == b.id);
    out
}

/// Roughly what a model wants at 4-bit, read from its **name**.
///
/// Crude on purpose. The hub does not state a quantised footprint, and fetching each
/// model's file listing to add it up would be dozens of requests to answer a question the
/// person is about to make for themselves. A parameter count in the name is the signal
/// every one of these carries, and ~0.6 GB per billion is close enough at 4-bit to sort
/// "fits" from "does not".
///
/// `None` when the name says nothing — better to leave a model out than to invite someone
/// to download something that cannot run.
#[must_use]
pub fn size_at_four_bits(id: &str) -> Option<u32> {
    let lowered = id.to_lowercase();
    let bytes: Vec<char> = lowered.chars().collect();
    let mut best: Option<f32> = None;
    for (i, c) in bytes.iter().enumerate() {
        if *c != 'b' {
            continue;
        }
        // Walk back over digits and at most one decimal point: `8b`, `1.5b`, `70b`.
        let mut start = i;
        let mut seen_dot = false;
        while start > 0 {
            let prev = bytes[start - 1];
            if prev.is_ascii_digit() {
                start -= 1;
            } else if prev == '.' && !seen_dot {
                seen_dot = true;
                start -= 1;
            } else {
                break;
            }
        }
        if start == i {
            continue;
        }
        // A letter before the number means it is part of a word, not a size.
        if start > 0 && bytes[start - 1].is_ascii_alphanumeric() {
            continue;
        }
        let Ok(params) = lowered[start..i].parse::<f32>() else {
            continue;
        };
        if params > 0.0 && params < 500.0 {
            best = Some(best.map_or(params, |b: f32| b.max(params)));
        }
    }
    // Round up: being wrong towards "too big to fit" costs a suggestion, being wrong the
    // other way costs a download and a crawl.
    best.map(|params| (params * 0.6).ceil() as u32)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_size_is_read_from_the_name() {
        assert_eq!(size_at_four_bits("bartowski/Qwen3-8B-GGUF"), Some(5));
        assert_eq!(size_at_four_bits("someone/llama-3.2-3b-instruct"), Some(2));
        assert_eq!(size_at_four_bits("x/Qwen2.5-14B-Instruct-GGUF"), Some(9));
        assert_eq!(size_at_four_bits("meta/Llama-3.3-70B"), Some(42));
    }

    #[test]
    fn a_name_that_says_nothing_is_left_out() {
        // Better to omit a model than to invite a download that cannot run.
        assert_eq!(size_at_four_bits("someone/my-cool-model"), None);
        // A `b` that is part of a word is not a size.
        assert_eq!(size_at_four_bits("someone/turbo-model"), None);
        assert_eq!(size_at_four_bits("org/deepblue"), None);
    }

    #[test]
    fn only_what_fits_with_room_to_spare_is_offered() {
        // Calibrated against a measured card rather than a guess: on a 12 GB A2000,
        // qwen2.5:14b loads at 9.5 GB and runs 100% on the GPU, so a 14B belongs in the
        // list. A 70B does not, and a model whose name states no size is left out.
        let hub = serde_json::json!([
            { "id": "a/Qwen3-8B-GGUF", "lastModified": "2026-07-20T10:00:00.000Z", "downloads": 900 },
            { "id": "b/Qwen2.5-14B-GGUF", "lastModified": "2026-07-21T10:00:00.000Z", "downloads": 5000 },
            { "id": "c/Llama-3.3-70B-GGUF", "lastModified": "2026-07-22T10:00:00.000Z", "downloads": 90000 },
            { "id": "d/mystery-model", "lastModified": "2026-07-23T10:00:00.000Z", "downloads": 10 }
        ]);
        let fits = sightings_from(&hub, 12);
        let ids: Vec<&str> = fits.iter().map(|s| s.id.as_str()).collect();
        assert_eq!(
            ids,
            vec!["b/Qwen2.5-14B-GGUF", "a/Qwen3-8B-GGUF"],
            "{fits:?}"
        );
        assert!(
            !ids.iter()
                .any(|id| id.contains("70B") || id.contains("mystery")),
            "offered something that cannot run: {ids:?}"
        );
        let eight = fits.iter().find(|s| s.id.starts_with("a/")).unwrap();
        assert_eq!(eight.updated, "2026-07-20");
        assert!(
            eight
                .how_to_get_it
                .contains("ollama pull hf.co/a/Qwen3-8B-GGUF")
        );
    }

    #[test]
    fn the_most_used_of_the_recent_ones_comes_first() {
        // Recency gets a model into the list; of those, the person wants the ones somebody
        // has actually run.
        let hub = serde_json::json!([
            { "id": "a/Thing-7B-GGUF", "lastModified": "2026-07-20T10:00:00.000Z", "downloads": 10 },
            { "id": "b/Other-7B-GGUF", "lastModified": "2026-07-19T10:00:00.000Z", "downloads": 8000 }
        ]);
        let fits = sightings_from(&hub, 12);
        assert_eq!(fits[0].id, "b/Other-7B-GGUF");
    }
}
