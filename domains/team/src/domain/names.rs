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
    for second in std::iter::once("").chain(OPENINGS.iter().copied()) {
        for opening in OPENINGS {
            for vowel in VOWELS {
                for ending in ENDINGS {
                    let tail = if second.is_empty() {
                        String::new()
                    } else {
                        format!("{}{vowel}", second.to_lowercase())
                    };
                    let name = format!("{opening}{vowel}{tail}{ending}");
                    if name.len() > 2 && free(&name) && !FIXED.contains(&name.as_str()) {
                        return name;
                    }
                }
            }
        }
    }
    format!("Member {}", taken.len() + 1)
}
