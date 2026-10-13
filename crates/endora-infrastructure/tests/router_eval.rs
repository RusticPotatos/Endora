//! Ignored live evaluation: can the local model sort a message into LOOKUP / PERSONAL / CHAT
//! well enough to decide when a turn needs a look at the outside world (ADR 0089)?
//!
//! It runs a fixed set of phrasings through `classify_need` and compares the model with the
//! word list it replaces (`asks_about_the_world`), so the claim "the router is at least as good"
//! is measured and not argued. The set includes the live misses the word list had, and controls
//! where a world word appears in a personal message.
//!
//! Needs a live endpoint: set ENDORA_MODEL_URL and ENDORA_EVAL_MODELS (comma separated).
//! No personal data: places and teams are the placeholder ones from AGENTS.md.

use endora_application::usecases::asks_about_the_world;
use endora_application::{Butler, NeedKind};
use endora_infrastructure::LlmButler;

use NeedKind::{Chat, Lookup, Personal};

/// (message, what it is). Includes the phrasings the word list got wrong.
const CASES: &[(&str, NeedKind)] = &[
    // The world: schedules, scores, prices, news, hours, facts.
    ("when is the next Example Team game?", Lookup),
    ("what specific games did the Example Team win/loose", Lookup),
    ("who won the game last night", Lookup),
    ("what time does the pharmacy on Main Street close", Lookup),
    ("how much is a ticket to the Sunday show", Lookup),
    (
        "what is the weather going to be like this weekend in New York",
        Lookup,
    ),
    ("did the Example Team trade anyone this week", Lookup),
    ("what's the latest on the transit strike", Lookup),
    ("is the museum open on Monday", Lookup),
    ("how many points did the Example Team score", Lookup),
    ("what movies are playing near me tonight", Lookup),
    ("what is the current price of gold", Lookup),
    ("which team leads the league right now", Lookup),
    ("what happened at the council meeting", Lookup),
    ("how tall is the tallest building in the city", Lookup),
    ("what are the best-reviewed restaurants downtown", Lookup),
    ("is there a recall on that stroller model", Lookup),
    ("what time is sunset today in Example City", Lookup),
    ("does the library have extended hours this month", Lookup),
    ("who is playing at the arena next month", Lookup),
    // The person's own things.
    ("when is my next appointment", Personal),
    ("is the kitchen light on", Personal),
    ("what did you do while I was out", Personal),
    ("who is home right now", Personal),
    ("turn off the bedroom lamp", Personal),
    ("when did you last check my calendar", Personal),
    ("what do you know about my routine", Personal),
    ("did anyone ring the door", Personal),
    ("what's on my list today", Personal),
    ("remind me what I asked you yesterday", Personal),
    // Small talk, writing, arithmetic.
    ("thanks, that helps", Chat),
    ("how are you today", Chat),
    ("write a two line poem about rain", Chat),
    ("what is 12 times 14", Chat),
    ("tell me a joke", Chat),
    ("can you explain what a mortgage is", Chat),
    ("good morning", Chat),
    ("when you are ready, we can carry on", Chat),
    ("what do you think about cats", Chat),
    ("summarise that in one sentence", Chat),
];

fn label(k: Option<NeedKind>) -> &'static str {
    match k {
        Some(Lookup) => "LOOKUP",
        Some(Personal) => "PERSONAL",
        Some(Chat) => "CHAT",
        None => "none",
    }
}

#[test]
#[ignore = "needs a live model: set ENDORA_MODEL_URL and ENDORA_EVAL_MODELS"]
fn the_local_model_sorts_messages_better_than_the_word_list() {
    let url = std::env::var("ENDORA_MODEL_URL").expect("set ENDORA_MODEL_URL");
    let models = std::env::var("ENDORA_EVAL_MODELS").expect("set ENDORA_EVAL_MODELS");

    // The word list only decides one thing: lookup or not.
    let list_right = CASES
        .iter()
        .filter(|(q, want)| asks_about_the_world(q) == (*want == Lookup))
        .count();
    println!(
        "word list (lookup vs not): {list_right}/{} right",
        CASES.len()
    );
    for (q, want) in CASES {
        if asks_about_the_world(q) != (*want == Lookup) {
            println!("   list missed: {q:?} (is {})", label(Some(*want)));
        }
    }

    for model in models.split(',').map(str::trim).filter(|m| !m.is_empty()) {
        let butler = LlmButler::new(url.clone(), model.to_owned());
        let (mut exact, mut lookup_ok, mut none) = (0, 0, 0);
        let mut missed: Vec<String> = Vec::new();
        for (q, want) in CASES {
            let got = butler.classify_need(q);
            if got.is_none() {
                none += 1;
            }
            if got == Some(*want) {
                exact += 1;
            }
            if (got == Some(Lookup)) == (*want == Lookup) {
                lookup_ok += 1;
            } else {
                missed.push(format!(
                    "{q:?} (is {}, said {})",
                    label(Some(*want)),
                    label(got)
                ));
            }
        }
        println!(
            "{model}: exact {exact}/{n}, lookup-vs-not {lookup_ok}/{n}, no answer {none}",
            n = CASES.len()
        );
        for m in &missed {
            println!("   router missed: {m}");
        }
    }
}
