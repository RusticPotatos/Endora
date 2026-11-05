//! Members' names (ADR 0142): eight fixed names first, then made up, and each one member's only.

/// The eight names, in the order they are given.
pub const FIXED: [&str; 8] = ["Mara", "Kit", "Ada", "Rae", "Bo", "Cy", "Dee", "Zed"];

const OPENINGS: [&str; 14] = [
    "B", "D", "F", "G", "J", "K", "L", "M", "N", "P", "R", "S", "T", "V",
];
const VOWELS: [&str; 5] = ["a", "e", "i", "o", "u"];
const ENDINGS: [&str; 6] = ["", "n", "l", "x", "s", "m"];

/// The first name nobody has: the next of the eight, then a made-up one. Compared whatever the
/// case, since members are named in threads by it.
#[must_use]
pub fn next_free(taken: &[&str]) -> String {
    let free = |name: &str| !taken.iter().any(|t| t.eq_ignore_ascii_case(name));
    if let Some(name) = FIXED.iter().find(|n| free(n)) {
        return (*name).to_owned();
    }
    made_up()
        .find(|name| name.len() > 2 && free(name) && !FIXED.contains(&name.as_str()))
        .unwrap_or_else(|| format!("Member {}", taken.len() + 1))
}

/// A made-up name: an opening and a vowel, the second syllable if there is one, and an ending.
fn name_of(opening: &str, vowel: &str, second: &str, ending: &str) -> String {
    let tail = if second.is_empty() {
        String::new()
    } else {
        format!("{}{vowel}", second.to_lowercase())
    };
    format!("{opening}{vowel}{tail}{ending}")
}

/// Every made-up name, in the order they are tried: one syllable, then two.
fn made_up() -> impl Iterator<Item = String> {
    let seconds = std::iter::once("").chain(OPENINGS.iter().copied());
    seconds.flat_map(|second| {
        OPENINGS.iter().flat_map(move |opening| {
            VOWELS.iter().flat_map(move |vowel| {
                ENDINGS
                    .iter()
                    .map(move |ending| name_of(opening, vowel, second, ending))
            })
        })
    })
}
