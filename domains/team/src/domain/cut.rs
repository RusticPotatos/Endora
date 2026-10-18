//! Text held to a size, and never silently: whatever is cut says so.
//!
//! The limits on what HQ keeps bound storage and prompts, and are not raised. What they must
//! not do is lose words with no sign of it, so a reader of a cut line is told it was cut.

/// What ends a text that was cut.
pub const CUT: &str = "…[cut]";

/// `text` as it is if it has at most `most` characters; otherwise its beginning followed by
/// [`CUT`], with `most` characters in all (at least the marker, for a `most` smaller than it).
/// Cuts on character boundaries, never in the middle of one.
#[must_use]
pub fn cut(text: &str, most: usize) -> String {
    if text.chars().count() <= most {
        return text.to_owned();
    }
    let kept = most.saturating_sub(CUT.chars().count());
    let mut out: String = text.chars().take(kept).collect();
    out.push_str(CUT);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn text_that_fits_is_left_alone() {
        assert_eq!(cut("short", 10), "short");
        assert_eq!(cut("exactly ten", 11), "exactly ten");
        assert_eq!(cut("", 0), "");
    }

    #[test]
    fn text_that_does_not_fit_is_cut_with_the_marker_inside_the_limit() {
        let long = "a".repeat(50);
        let got = cut(&long, 20);
        assert_eq!(got.chars().count(), 20);
        assert!(got.ends_with(CUT), "{got}");
        assert!(got.starts_with("aaaa"));
    }

    #[test]
    fn a_cut_never_lands_inside_a_character() {
        let long = "é🙂".repeat(40);
        let got = cut(&long, 15);
        assert_eq!(got.chars().count(), 15);
        assert!(got.ends_with(CUT));
        assert!(got.starts_with("é🙂é🙂"));
    }

    #[test]
    fn a_limit_smaller_than_the_marker_still_says_it_was_cut() {
        assert_eq!(cut("abcdefgh", 3), CUT);
    }
}
