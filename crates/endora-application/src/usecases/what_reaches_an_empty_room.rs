use super::*;

fn offering_nothing() -> ButlerContext {
    ButlerContext::default()
}

#[test]
fn an_apology_for_an_unreachable_model_is_not_an_answer() {
    // Live, 00:00:14: the night loop ran, could not reach the model, and posted "Give
    // me a moment and try again" into an empty room. Nobody had asked anything.
    let degraded = ButlerReply {
        text: "Sorry — I couldn't reach my language model just now.".to_owned(),
        degraded: true,
        ..ButlerReply::default()
    };
    assert!(not_an_answer(&degraded, &offering_nothing()));

    // The same sentence, when it is genuinely the butler's answer, is kept — the flag
    // is what distinguishes them, not the words.
    let genuine = ButlerReply {
        text: "Sorry — I couldn't reach my language model just now.".to_owned(),
        ..ButlerReply::default()
    };
    assert!(!not_an_answer(&genuine, &offering_nothing()));
}

#[test]
fn protocol_prose_posted_unprompted_is_not_an_answer() {
    // Live, 04:18, in the person's chat: the check-in's whole contribution for the day.
    // The suppression list held "functions provided" and missed this by one word.
    let leaked = ButlerReply {
        text: "None of the functions listed involve a 'person' domain, so there's no \
               need to call any."
            .to_owned(),
        ..ButlerReply::default()
    };
    assert!(not_an_answer(&leaked, &offering_nothing()));
}

#[test]
fn the_marker_is_the_shortest_thing_that_can_only_be_plumbing() {
    // Two live misses, each by less than a word. The list held "functions provided" and
    // missed "None of the functions listed involve a 'person' domain"; then it held
    // "functions" and missed "please provide me with a function name". An exact-phrase
    // list will always fail this way.
    for leaked in [
        "None of the functions listed involve a 'person' domain, so there's no need.",
        "Sure, please provide me with a function name and the corresponding arguments.",
        "Based on your request, here are the appropriate function calls.",
    ] {
        let reply = ButlerReply {
            text: leaked.to_owned(),
            ..ButlerReply::default()
        };
        assert!(not_an_answer(&reply, &offering_nothing()), "{leaked}");
    }
}

#[test]
fn an_ordinary_reply_is_still_an_answer() {
    let real = ButlerReply {
        text: "Good morning, sir. The kitchen and garage lights are on.".to_owned(),
        ..ButlerReply::default()
    };
    assert!(!not_an_answer(&real, &offering_nothing()));
}
