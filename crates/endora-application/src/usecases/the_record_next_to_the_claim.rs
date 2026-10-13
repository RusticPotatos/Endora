use super::*;

fn did() -> Vec<String> {
    vec![
        "11:01 — I wrote to you unprompted: \"Good morning, sir.…\"".to_owned(),
        "21:33 — tried home-assistant.HassTurnOn and it failed".to_owned(),
    ]
}

#[test]
fn being_asked_what_it_did_gets_the_record_attached() {
    // The question behind the live denial "no specific activities were recorded while
    // you were out", given verbatim twice on two different days with eleven things in the
    // record. The trigger is now the question, so it does not matter how the model words
    // its answer — including when it words it as a plain "no".
    for asked in [
        "did you do anything while I was out?",
        "what have you been up to today?",
        "have you done anything lately",
        "what are you doing",
    ] {
        let shown = account_behind(asked, &did());
        assert!(shown.contains("[did]"), "{asked:?}: {shown}");
        assert!(shown.contains("I wrote to you unprompted"), "{shown}");
        assert!(shown.contains("HassTurnOn and it failed"), "{shown}");
    }
}

#[test]
fn asking_about_proactive_actions_also_gets_it() {
    // The other live shape: the model hedged "It seems like you're asking about whether
    // any proactive actions were taken today…" to a question of this kind.
    assert!(
        account_behind("have you taken any proactive actions today?", &did()).contains("[did]")
    );
    assert!(account_behind("anything unprompted overnight?", &did()).contains("[did]"));
}

#[test]
fn an_ordinary_question_gets_nothing_attached() {
    // Measured live: the model ended a normal answer with "…other events or activities,
    // feel free to ask!" and the whole record was appended to a reply about a city,
    // because the trigger was a word in the MODEL'S sentence. Nothing here asks what
    // Endora did, so nothing is attached, whatever it says back.
    for ordinary in [
        "watch events are in new york this month",
        "what watch events are in new york this month?",
        "can you look online?",
        "is the kitchen table light on?",
        "turn the garage light off",
        "what time is it?",
        "can you find me some activities for the kids this weekend",
    ] {
        assert_eq!(account_behind(ordinary, &did()), "", "{ordinary}");
    }
}

#[test]
fn with_nothing_to_report_nothing_is_attached() {
    // No record, no disclosure — appending an empty account would be its own kind of
    // noise, and an answer that there was nothing to report would then be correct.
    assert_eq!(
        account_behind("did you do anything while I was out?", &[]),
        ""
    );
}
