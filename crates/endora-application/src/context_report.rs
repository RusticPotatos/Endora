//! What the model is sent each turn, and how much of its window that fills (ADR 0088).
//!
//! The prompt is assembled in `endora-infrastructure` from a [`ButlerContext`], the recent
//! messages and the preferences. This measures the same inputs, section by section, so the
//! console can show the person what is being sent and how full the window is.
//!
//! **Sizes are estimates**: four characters to a token, which is close for English and rough
//! for JSON. They are labelled so wherever they are shown. The model server's own count is
//! not available to the application layer, and an honest estimate beats a precise-looking
//! number that is not.

use crate::Preference;
use crate::ports::ButlerContext;
use endora_conversation::{ChatMessage, MessageRole};

/// How many lines of one section are listed, so a hundred-tool prompt stays a short answer.
const MOST_ITEMS: usize = 60;
/// How much of one line is kept.
const MOST_LINE: usize = 160;

/// Roughly how many tokens a piece of text takes.
#[must_use]
pub fn estimate_tokens(text: &str) -> usize {
    text.chars().count().div_ceil(4)
}

/// One line of a section.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextItem {
    /// What is sent, clipped.
    pub text: String,
    /// Its estimated size.
    pub tokens: usize,
}

/// One kind of thing in the prompt.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextSection {
    /// A stable key for the console.
    pub key: &'static str,
    /// What the person reads.
    pub label: &'static str,
    /// Estimated size of the whole section.
    pub tokens: usize,
    /// How many things are in it (more than are listed, when long).
    pub count: usize,
    /// The biggest things first, clipped.
    pub items: Vec<ContextItem>,
    /// Whether this section is part of what a bare "Ask deep" plan is shown (ADR 0086).
    pub to_deep: bool,
    /// A line of explanation, where the section needs one.
    pub note: &'static str,
}

/// The whole picture.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ContextReport {
    /// In the order they appear in the prompt.
    pub sections: Vec<ContextSection>,
    /// The sum, estimated.
    pub total_tokens: usize,
    /// The model's window, as configured.
    pub window_tokens: usize,
}

fn clip(text: &str) -> String {
    let one_line = text.split_whitespace().collect::<Vec<_>>().join(" ");
    if one_line.chars().count() <= MOST_LINE {
        return one_line;
    }
    let mut out: String = one_line.chars().take(MOST_LINE).collect();
    out.push('…');
    out
}

/// A section from the texts that fill it. The biggest are listed first, because the question
/// the person is asking is "what is taking the room".
fn section(
    key: &'static str,
    label: &'static str,
    note: &'static str,
    to_deep: bool,
    texts: &[String],
) -> ContextSection {
    let mut items: Vec<ContextItem> = texts
        .iter()
        .map(|t| ContextItem {
            text: clip(t),
            tokens: estimate_tokens(t),
        })
        .collect();
    let tokens = texts.iter().map(|t| estimate_tokens(t)).sum();
    let count = items.len();
    items.sort_by(|a, b| b.tokens.cmp(&a.tokens));
    items.truncate(MOST_ITEMS);
    ContextSection {
        key,
        label,
        tokens,
        count,
        items,
        to_deep,
        note,
    }
}

/// Measures what the next message would send.
///
/// `instructions` is the fixed text Endora always sends first. `summary` is the running
/// summary of older chat, and `history` the recent messages that ride word for word.
#[must_use]
pub fn context_report(
    context: &ButlerContext,
    history: &[ChatMessage],
    preferences: &[Preference],
    summary: Option<&str>,
    instructions: &str,
    window_tokens: usize,
) -> ContextReport {
    let tool_texts: Vec<String> = context
        .tools
        .iter()
        .map(|t| {
            format!(
                "{} — {} {}",
                t.id,
                t.description,
                t.input_schema.as_deref().unwrap_or("")
            )
        })
        .collect();
    let nonempty = |s: &str| (!s.trim().is_empty()).then(|| s.to_owned());
    let clock_and_place: Vec<String> = [
        nonempty(&context.now).map(|n| format!("It is {n}")),
        nonempty(&context.where_they_are).map(|p| format!("They are based in {p}")),
    ]
    .into_iter()
    .flatten()
    .collect();
    let prefs: Vec<String> = preferences
        .iter()
        .map(|p| format!("({}) {}", p.kind().name(), p.text()))
        .collect();
    let recent: Vec<String> = history
        .iter()
        .map(|m| {
            let who = match m.role() {
                MessageRole::User => "You",
                MessageRole::Butler => "Endora",
            };
            format!("{who}: {}", m.text())
        })
        .collect();
    let summary_text: Vec<String> = summary
        .filter(|s| !s.trim().is_empty())
        .map(|s| vec![s.to_owned()])
        .unwrap_or_default();
    let held_back: Vec<String> = context.deferred.iter().map(|t| t.id.clone()).collect();

    let mut sections = vec![
        section(
            "instructions",
            "Endora's instructions",
            "Fixed text sent first every time.",
            false,
            &[instructions.to_owned()],
        ),
        section("now", "Date and place", "", false, &clock_and_place),
        section(
            "summary",
            "Summary of earlier chat",
            "Older messages folded into a few sentences.",
            false,
            &summary_text,
        ),
        section("preferences", "Your preferences", "", false, &prefs),
        section(
            "understanding",
            "What Endora understands about you",
            "Its beliefs about you; separate from the chat.",
            false,
            &context.understanding,
        ),
        section(
            "present",
            "The house right now",
            "",
            false,
            &context.present,
        ),
        section(
            "did_lately",
            "What Endora did lately",
            "Only sent when you ask what it did.",
            false,
            &context.did_lately,
        ),
        section(
            "aliases",
            "Names you taught it",
            "",
            false,
            &context.target_aliases,
        ),
        section(
            "track_record",
            "How its past actions landed",
            "",
            false,
            &context.track_record,
        ),
        section(
            "skill_list",
            "Skill list in the instructions",
            "The same skills again, as a plain list.",
            true,
            &context.capabilities,
        ),
        section(
            "skills",
            "Skill definitions",
            "What each skill does and its inputs, sent as tools. This is what an Ask deep plan sees.",
            true,
            &tool_texts,
        ),
        section(
            "recent",
            "Recent messages",
            "The last stretch of this chat, word for word.",
            false,
            &recent,
        ),
    ];
    sections.retain(|s| s.count > 0);
    let mut held = section(
        "held_back",
        "Held back until needed",
        "Found on request; not sent now.",
        false,
        &held_back,
    );
    // Not in the prompt, so it does not count toward the total; listed so it is visible.
    held.tokens = 0;
    if held.count > 0 {
        sections.push(held);
    }
    let total_tokens = sections.iter().map(|s| s.tokens).sum();
    ContextReport {
        sections,
        total_tokens,
        window_tokens,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::ports::CapabilityTool;

    fn tool(id: &str, description: &str, schema: Option<&str>) -> CapabilityTool {
        CapabilityTool {
            id: id.to_owned(),
            description: description.to_owned(),
            input_schema: schema.map(str::to_owned),
        }
    }

    #[test]
    fn four_characters_to_a_token_rounded_up() {
        assert_eq!(estimate_tokens(""), 0);
        assert_eq!(estimate_tokens("abcd"), 1);
        assert_eq!(estimate_tokens("abcde"), 2);
    }

    #[test]
    fn the_biggest_things_are_listed_first_and_the_total_is_the_sum() {
        let context = ButlerContext {
            tools: vec![
                tool("small", "tiny", None),
                tool("big", "does a lot", Some(&"x".repeat(400))),
            ],
            ..ButlerContext::default()
        };

        let report = context_report(&context, &[], &[], None, "be nice", 16_384);

        let skills = report.sections.iter().find(|s| s.key == "skills").unwrap();
        assert_eq!(skills.count, 2);
        assert!(
            skills.items[0].text.starts_with("big"),
            "{:?}",
            skills.items
        );
        assert_eq!(
            report.total_tokens,
            report.sections.iter().map(|s| s.tokens).sum::<usize>()
        );
        assert_eq!(report.window_tokens, 16_384);
    }

    #[test]
    fn empty_sections_are_left_out_and_held_back_tools_do_not_count() {
        let context = ButlerContext {
            deferred: vec![tool("turn_on_light", "acts", None)],
            ..ButlerContext::default()
        };

        let report = context_report(&context, &[], &[], None, "hi", 1000);

        assert!(report.sections.iter().all(|s| s.key != "understanding"));
        let held = report
            .sections
            .iter()
            .find(|s| s.key == "held_back")
            .unwrap();
        assert_eq!(held.count, 1);
        assert_eq!(held.tokens, 0);
        assert_eq!(report.total_tokens, estimate_tokens("hi"));
    }

    #[test]
    fn only_the_skills_are_marked_as_going_to_the_deep_model() {
        let context = ButlerContext {
            tools: vec![tool("a", "b", None)],
            capabilities: vec!["a — b".to_owned()],
            understanding: vec!["likes tea".to_owned()],
            ..ButlerContext::default()
        };

        let report = context_report(&context, &[], &[], Some("earlier"), "hi", 1000);

        let deep: Vec<&str> = report
            .sections
            .iter()
            .filter(|s| s.to_deep)
            .map(|s| s.key)
            .collect();
        assert_eq!(deep, vec!["skill_list", "skills"]);
    }

    #[test]
    fn a_long_line_is_clipped_but_still_counted_whole() {
        let long = "word ".repeat(200);
        let context = ButlerContext {
            understanding: vec![long.clone()],
            ..ButlerContext::default()
        };

        let report = context_report(&context, &[], &[], None, "", 1000);

        let s = report
            .sections
            .iter()
            .find(|s| s.key == "understanding")
            .unwrap();
        assert!(s.items[0].text.chars().count() <= MOST_LINE + 1);
        assert_eq!(s.tokens, estimate_tokens(&long));
    }
}
