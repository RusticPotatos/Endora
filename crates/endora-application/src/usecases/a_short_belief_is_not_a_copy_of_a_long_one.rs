//! Caught by the live smoke, which is where this repository's bugs keep surfacing.
//! Containment made a specific belief swallow a general one — and because the same rule
//! decides whether a night's thought is a discovery, a detailed belief quietly stopped
//! related simpler ones from ever forming. The opposite of learning, in the function
//! that decides what is learned.

use super::says_the_same_thing;

const SPECIFIC: &str = "Work unpredictability squeezes your evenings — a long issue \
                        call pushed your main work late — so the after-work run needs \
                        to stay small enough to survive a late day.";
const GENERAL: &str = "you prefer to run after work on Tuesdays and Thursdays";

#[test]
fn two_thoughts_about_one_subject_are_not_one_thought() {
    assert!(
        !says_the_same_thing(SPECIFIC, GENERAL),
        "a preference and what the week does to it are different beliefs"
    );
    // Symmetric, which containment never was.
    assert!(!says_the_same_thing(GENERAL, SPECIFIC));
}

#[test]
fn a_real_repeat_is_still_a_repeat() {
    // The job it must keep doing: rewording is not a discovery.
    assert!(says_the_same_thing(
        "you prefer to run after work on Tuesdays and Thursdays",
        "You prefer running after work on Tuesdays and Thursdays."
    ));
}
