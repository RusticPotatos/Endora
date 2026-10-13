use super::*;

#[test]
fn a_wordier_way_of_saying_the_same_thing_is_the_same_belief() {
    // All three of these were on one screen at once, each asking to be confirmed
    // separately. The third is the one plain overlap missed: it spends most of its
    // words on how strongly the person feels and only two on the subject.
    let plain = "you prefer temperatures in Fahrenheit";
    assert!(similar(
        plain,
        "You prefer temperature measurements in Fahrenheit."
    ));
    assert!(similar(
        plain,
        "You find it more convenient and accurate to measure temperature in Fahrenheit"
    ));
}

#[test]
fn a_contradiction_needs_the_two_to_be_about_the_same_thing() {
    // Live, on the deployed instance: this one belief was reported as contradicting
    // every other belief Endora held, because negation asymmetry alone counted and
    // nothing checked the two were even on the same topic.
    let celsius = "You find it more convenient and accurate to measure temperature in Celsius \
         rather than Fahrenheit.";
    for unrelated in [
        "You want to know where I found the event information",
        "you usually want news, weather, and traffic",
        "you prefer to run after work on Tuesdays and Thursdays",
        "You like a positive, friendly greeting at the beginning of each day.",
    ] {
        assert!(
            !statements_disagree(celsius, unrelated),
            "claimed a contradiction with an unrelated belief: {unrelated:?}"
        );
    }
    // The one it genuinely does disagree with is still caught.
    assert!(statements_disagree(
        celsius,
        "you prefer temperatures in Fahrenheit"
    ));
}

#[test]
fn stripping_the_stance_does_not_merge_opposites() {
    // The cost of ignoring stance words: two statements about the same subject look
    // identical whether they agree or not. That is safe only because disagreement is
    // checked first, so this is the test that holds the whole trade together.
    assert!(!similar("you like running", "you hate running"));
    assert!(!similar(
        "you prefer temperatures in Fahrenheit",
        "you do not want temperatures in Fahrenheit"
    ));
    // And different subjects stay different however similarly they are framed.
    assert!(!similar("you prefer coffee", "you prefer tea"));
}
