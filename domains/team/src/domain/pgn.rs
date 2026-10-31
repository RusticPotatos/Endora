//! A game of chess written out in Portable Game Notation, the standard export every chess
//! program reads: the seven tags (Event, Site, Date, Round, White, Black, Result), a
//! `Termination` tag, and the moves with their numbers, wrapped under 80 columns, ending in the
//! result. How the game ended in the house's own words (`black resigned`) is kept as a comment
//! before the result. Pure: the caller says who played, when, and how it ended.

use super::chess::Status;
use super::work::Millis;

/// The event a league game is written under.
pub const LEAGUE: &str = "Endora model league";
/// The event an owner's Holodeck match outside the league is written under.
pub const HOLODECK: &str = "Endora Holodeck";
/// The event a game members played in the office is written under.
pub const OFFICE: &str = "Endora office chess";
/// The site every game is written at.
const SITE: &str = "Endora HQ";
/// The longest line of move text the export format allows.
const WIDTH: usize = 79;

/// A game to write out.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Record<'a> {
    /// The event: [`LEAGUE`], [`HOLODECK`] or [`OFFICE`].
    pub event: &'a str,
    /// When it began, if known.
    pub date: Option<Millis>,
    /// Its round in the event, if it has one.
    pub round: Option<usize>,
    /// Who played white: a brain's label in the league, a member's name elsewhere.
    pub white: &'a str,
    /// Who played black.
    pub black: &'a str,
    /// The moves, in standard notation, white's first.
    pub moves: &'a [String],
    /// Where it stands.
    pub status: Status,
    /// How it ended, in the house's words; empty while it goes on.
    pub reason: &'a str,
    /// Stopped with no result (a Holodeck match nobody acted in).
    pub abandoned: bool,
}

/// The result as PGN writes it.
const fn result(status: Status) -> &'static str {
    match status {
        Status::WhiteWon => "1-0",
        Status::BlackWon => "0-1",
        Status::Draw => "1/2-1/2",
        Status::InProgress => "*",
    }
}

/// How the game ended, in the standard's words: a resignation, mate or a rule's draw is
/// `normal`; a side that stopped moving lost on `time forfeit`; a game the house adjourned as
/// a draw after a week without a move was decided by `adjudication`; a match stopped with no
/// result was `abandoned`; one still going on is `unterminated`.
fn termination(record: &Record<'_>) -> &'static str {
    if record.status == Status::InProgress {
        return if record.abandoned {
            "abandoned"
        } else {
            "unterminated"
        };
    }
    if record.reason.contains("stopped moving") || record.reason.contains("forfeit") {
        return "time forfeit";
    }
    if record.reason.contains("adjourned") {
        return "adjudication";
    }
    "normal"
}

/// A tag's value, its quotes and backslashes escaped and kept to one line.
fn tag_value(value: &str) -> String {
    value
        .chars()
        .filter(|c| !c.is_control())
        .flat_map(|c| match c {
            '"' | '\\' => vec!['\\', c],
            _ => vec![c],
        })
        .collect()
}

/// The UTC date, `YYYY.MM.DD`, of a moment (Howard Hinnant's civil-from-days).
fn date(at: Option<Millis>) -> String {
    let Some(at) = at else {
        return "????.??.??".to_owned();
    };
    let days = at.div_euclid(86_400_000) + 719_468;
    let era = days.div_euclid(146_097);
    let doe = days - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let day = doy - (153 * mp + 2) / 5 + 1;
    let month = if mp < 10 { mp + 3 } else { mp - 9 };
    let year = yoe + era * 400 + i64::from(month <= 2);
    format!("{year:04}.{month:02}.{day:02}")
}

/// The game written out in PGN, ending in a newline.
#[must_use]
pub fn pgn(record: &Record<'_>) -> String {
    let result = result(record.status);
    let round = record
        .round
        .map_or_else(|| "-".to_owned(), |r| r.to_string());
    let mut text = String::new();
    for (name, value) in [
        ("Event", record.event.to_owned()),
        ("Site", SITE.to_owned()),
        ("Date", date(record.date)),
        ("Round", round),
        ("White", record.white.to_owned()),
        ("Black", record.black.to_owned()),
        ("Result", result.to_owned()),
        ("Termination", termination(record).to_owned()),
    ] {
        text.push_str(&format!("[{name} \"{}\"]\n", tag_value(&value)));
    }
    text.push('\n');
    let mut tokens: Vec<String> = Vec::new();
    for (i, san) in record.moves.iter().enumerate() {
        if i % 2 == 0 {
            tokens.push(format!("{}.", i / 2 + 1));
        }
        tokens.push(san.clone());
    }
    let why: String = record
        .reason
        .chars()
        .filter(|c| !matches!(c, '{' | '}') && !c.is_control())
        .collect();
    if !why.trim().is_empty() {
        tokens.extend(format!("{{{}}}", why.trim()).split(' ').map(str::to_owned));
    }
    tokens.push(result.to_owned());
    let mut line = String::new();
    for token in tokens {
        if !line.is_empty() && line.len() + 1 + token.len() > WIDTH {
            text.push_str(&line);
            text.push('\n');
            line.clear();
        }
        if !line.is_empty() {
            line.push(' ');
        }
        line.push_str(&token);
    }
    text.push_str(&line);
    text.push('\n');
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    /// 2026-10-30, noon UTC.
    const NOON: Millis = 1_793_361_600_000;

    fn moves(all: &[&str]) -> Vec<String> {
        all.iter().map(|m| (*m).to_owned()).collect()
    }

    fn record<'a>(moves: &'a [String], status: Status, reason: &'a str) -> Record<'a> {
        Record {
            event: LEAGUE,
            date: Some(NOON),
            round: Some(3),
            white: "Local",
            black: "Claude Haiku",
            moves,
            status,
            reason,
            abandoned: false,
        }
    }

    #[test]
    fn a_mate_is_written_with_the_seven_tags_numbered_moves_and_the_result() {
        let played = moves(&["f3", "e5", "g4", "Qh4#"]);
        let written = pgn(&record(&played, Status::BlackWon, "checkmate"));
        assert_eq!(
            written,
            "[Event \"Endora model league\"]\n[Site \"Endora HQ\"]\n[Date \"2026.10.30\"]\n\
             [Round \"3\"]\n[White \"Local\"]\n[Black \"Claude Haiku\"]\n[Result \"0-1\"]\n\
             [Termination \"normal\"]\n\n1. f3 e5 2. g4 Qh4# {checkmate} 0-1\n"
        );
    }

    #[test]
    fn a_resignation_ends_normally_and_says_who_resigned() {
        let played = moves(&["e4", "e5"]);
        let written = pgn(&record(&played, Status::WhiteWon, "black resigned"));
        assert!(written.contains("[Result \"1-0\"]"), "{written}");
        assert!(written.contains("[Termination \"normal\"]"), "{written}");
        assert!(
            written.ends_with("1. e4 e5 {black resigned} 1-0\n"),
            "{written}"
        );
    }

    #[test]
    fn a_side_that_stopped_moving_loses_on_time_forfeit() {
        let played = moves(&["e4"]);
        let written = pgn(&record(
            &played,
            Status::WhiteWon,
            "black stopped moving, and forfeits",
        ));
        assert!(
            written.contains("[Termination \"time forfeit\"]"),
            "{written}"
        );
        assert!(
            written.ends_with("1. e4 {black stopped moving, and forfeits} 1-0\n"),
            "{written}"
        );
    }

    #[test]
    fn an_adjourned_game_is_an_adjudicated_draw() {
        let played = moves(&["d4", "d5", "c4"]);
        let written = pgn(&record(
            &played,
            Status::Draw,
            "adjourned after a week without a move",
        ));
        assert!(written.contains("[Result \"1/2-1/2\"]"), "{written}");
        assert!(
            written.contains("[Termination \"adjudication\"]"),
            "{written}"
        );
        assert!(written.ends_with("1/2-1/2\n"), "{written}");
    }

    #[test]
    fn a_game_going_on_or_abandoned_has_no_result() {
        let played = moves(&["e4", "c5"]);
        let live = pgn(&record(&played, Status::InProgress, ""));
        assert!(live.contains("[Result \"*\"]"), "{live}");
        assert!(live.contains("[Termination \"unterminated\"]"), "{live}");
        assert!(live.ends_with("\n1. e4 c5 *\n"), "{live}");
        let mut stopped = record(&played, Status::InProgress, "");
        stopped.abandoned = true;
        assert!(pgn(&stopped).contains("[Termination \"abandoned\"]"));
        let empty = pgn(&record(&[], Status::InProgress, ""));
        assert!(empty.ends_with("\n\n*\n"), "{empty}");
    }

    #[test]
    fn names_are_escaped_an_unknown_date_is_marked_and_long_games_wrap() {
        let played: Vec<String> = ["Nf3", "Nf6", "Ng1", "Ng8"]
            .iter()
            .cycle()
            .take(60)
            .map(|m| (*m).to_owned())
            .collect();
        let mut r = record(&played, Status::Draw, "the same position a third time");
        r.white = "Jane \"the\" Doe\\";
        r.black = "John Doe";
        r.date = None;
        r.round = None;
        r.event = OFFICE;
        let written = pgn(&r);
        assert!(
            written.contains("[White \"Jane \\\"the\\\" Doe\\\\\"]"),
            "{written}"
        );
        assert!(written.contains("[Date \"????.??.??\"]"), "{written}");
        assert!(written.contains("[Round \"-\"]"), "{written}");
        assert!(written.contains("[Event \"Endora office chess\"]"));
        let body = written.split("\n\n").nth(1).unwrap();
        assert!(body.lines().count() > 1, "{body}");
        assert!(body.lines().all(|l| l.len() < 80), "{body}");
        assert!(body.contains("30. Ng1 Ng8"), "{body}");
    }

    #[test]
    fn dates_are_the_utc_day() {
        assert_eq!(date(Some(0)), "1970.01.01");
        assert_eq!(date(Some(NOON)), "2026.10.30");
        assert_eq!(date(Some(951_782_400_000)), "2000.02.29");
    }
}
