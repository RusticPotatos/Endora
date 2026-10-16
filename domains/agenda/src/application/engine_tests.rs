use super::*;
use crate::domain::{DAY_MS, Family, HOUR_MS, Kind, Level, MINUTE_MS, Response, Signal, Source};
use crate::infrastructure::{SqliteAgendaStore, migrate};
use endora_persistence::Db;
use std::cell::RefCell;

#[derive(Default)]
struct Fakes {
    restarted: RefCell<Vec<String>>,
    refuse: RefCell<Option<String>>,
    pushed: RefCell<Vec<String>>,
    lines: RefCell<Vec<String>>,
}

impl Restarter for Fakes {
    fn restart(&self, target: &str) -> Result<(), String> {
        if let Some(why) = self.refuse.borrow().clone() {
            return Err(why);
        }
        self.restarted.borrow_mut().push(target.to_owned());
        Ok(())
    }
}

impl Pusher for Fakes {
    fn push(&self, text: &str) -> Result<(), String> {
        self.pushed.borrow_mut().push(text.to_owned());
        Ok(())
    }
}

impl Activity for Fakes {
    fn record(&self, _at: i64, line: &str) {
        self.lines.borrow_mut().push(line.to_owned());
    }
}

fn store() -> SqliteAgendaStore {
    let db = Db::open_in_memory().expect("db");
    migrate(&db).expect("tables");
    SqliteAgendaStore::new(db)
}

fn crashed(name: &str) -> Signal {
    Signal {
        kind: Kind::ContainerCrashed,
        subject: name.to_owned(),
        about: name.to_owned(),
        name: name.to_owned(),
        detail: "exit 1".to_owned(),
        target: Some("f".repeat(64)),
        cautious: false,
        at: None,
    }
}

fn stuck(title: &str) -> Signal {
    Signal {
        kind: Kind::DownloadStuck,
        subject: title.to_owned(),
        about: title.to_owned(),
        name: title.to_owned(),
        detail: "Sonarr".to_owned(),
        target: None,
        cautious: false,
        at: None,
    }
}

fn mail(kind: Kind, key: &str, cautious: bool) -> Signal {
    Signal {
        kind,
        subject: key.to_owned(),
        about: format!("{key}@example.com"),
        name: "Example".to_owned(),
        detail: "A bill from Example. Amount: $12.".to_owned(),
        target: None,
        cautious,
        at: Some(T0),
    }
}

const T0: i64 = 100 * DAY_MS;

fn last(f: &Fakes) -> String {
    f.lines.borrow().last().cloned().unwrap_or_default()
}

#[test]
fn a_crashed_container_is_restarted_and_its_item_clears_when_it_runs_again() {
    let (s, f) = (store(), Fakes::default());
    let agenda = Agenda::new(&s, &f, &f, &f);
    let change = agenda
        .observe(Source::Docker, &[crashed("sonarr")], T0)
        .unwrap();
    assert_eq!(change.opened, 1);
    assert_eq!(f.restarted.borrow().len(), 1);
    assert_eq!(last(&f), "Handled: sonarr crashed (exit 1). Restarted it.");
    let open = s.open_items().unwrap();
    assert_eq!(
        (open[0].level, open[0].why.as_str()),
        (Level::Handled, "Restarted it.")
    );
    // Seen again before it is back: still one item, and no second restart.
    agenda
        .observe(Source::Docker, &[crashed("sonarr")], T0 + MINUTE_MS)
        .unwrap();
    assert_eq!(s.open_items().unwrap().len(), 1);
    assert_eq!(f.restarted.borrow().len(), 1);
    // Running again: the item closes, with the time.
    let change = agenda
        .observe(Source::Docker, &[], T0 + 2 * MINUTE_MS)
        .unwrap();
    assert_eq!(change.closed, 1);
    assert!(s.open_items().unwrap().is_empty());
    assert_eq!(last(&f), "Cleared: sonarr crashed (exit 1), after 2 min.");
}

#[test]
fn a_restart_that_does_not_take_is_told_but_only_after_a_read_proves_it() {
    let (s, f) = (store(), Fakes::default());
    let agenda = Agenda::new(&s, &f, &f, &f);
    agenda
        .observe(Source::Docker, &[crashed("sonarr")], T0)
        .unwrap();
    // The deadline passes, but the containers have not been read since: nothing is known.
    assert!(!agenda.tick(T0 + 6 * MINUTE_MS).unwrap());
    assert_eq!(s.open_items().unwrap()[0].level, Level::Handled);
    agenda
        .observe(Source::Docker, &[crashed("sonarr")], T0 + 6 * MINUTE_MS)
        .unwrap();
    assert!(agenda.tick(T0 + 6 * MINUTE_MS).unwrap());
    let item = &s.open_items().unwrap()[0];
    assert_eq!(item.level, Level::NeedsYou);
    assert_eq!(
        last(&f),
        "Needs you: sonarr crashed (exit 1). I restarted it, but it isn't back."
    );
    assert_eq!(f.restarted.borrow().len(), 1, "told, not restarted again");
}

#[test]
fn a_container_that_stops_again_within_the_hour_is_told_and_a_third_time_is_urgent() {
    let (s, f) = (store(), Fakes::default());
    let agenda = Agenda::new(&s, &f, &f, &f);
    agenda
        .observe(Source::Docker, &[crashed("plex")], T0)
        .unwrap();
    agenda.observe(Source::Docker, &[], T0 + MINUTE_MS).unwrap();
    agenda
        .observe(Source::Docker, &[crashed("plex")], T0 + 30 * MINUTE_MS)
        .unwrap();
    assert_eq!(f.restarted.borrow().len(), 1);
    let item = &s.open_items().unwrap()[0];
    assert_eq!(item.level, Level::NeedsYou);
    assert!(
        item.why.starts_with("It stopped again within an hour"),
        "{}",
        item.why
    );
    // Two restarts that held, hours apart; the third crash that day is urgent and pushed.
    agenda.observe(Source::Docker, &[], T0 + HOUR_MS).unwrap();
    agenda
        .observe(Source::Docker, &[crashed("plex")], T0 + 3 * HOUR_MS)
        .unwrap();
    agenda
        .observe(Source::Docker, &[], T0 + 3 * HOUR_MS + MINUTE_MS)
        .unwrap();
    agenda
        .observe(Source::Docker, &[crashed("plex")], T0 + 6 * HOUR_MS)
        .unwrap();
    assert_eq!(f.restarted.borrow().len(), 2);
    let item = &s.open_items().unwrap()[0];
    assert_eq!(item.level, Level::Urgent);
    assert!(item.pushed);
    assert_eq!(
        f.pushed.borrow().as_slice(),
        ["Urgent: plex crashed (exit 1)."]
    );
}

#[test]
fn a_refused_restart_needs_you_with_the_reason() {
    let (s, f) = (store(), Fakes::default());
    *f.refuse.borrow_mut() = Some("protected container".to_owned());
    let agenda = Agenda::new(&s, &f, &f, &f);
    agenda
        .observe(Source::Docker, &[crashed("portainer")], T0)
        .unwrap();
    let item = &s.open_items().unwrap()[0];
    assert_eq!(item.level, Level::NeedsYou);
    assert_eq!(item.why, "I couldn't restart it: protected container.");
    assert!(
        s.restarts_since(0).unwrap().is_empty(),
        "a refused restart is not counted"
    );
}

#[test]
fn a_failed_read_closes_nothing() {
    let (s, f) = (store(), Fakes::default());
    let agenda = Agenda::new(&s, &f, &f, &f);
    agenda
        .observe(Source::Sonarr, &[stuck("Show S01E01")], T0)
        .unwrap();
    // Another source's empty read is not this source's.
    agenda.observe(Source::Radarr, &[], T0 + MINUTE_MS).unwrap();
    agenda.tick(T0 + 2 * DAY_MS).unwrap();
    assert_eq!(s.open_items().unwrap().len(), 1);
}

#[test]
fn a_stuck_download_is_watched_for_a_day_then_needs_you_and_three_are_watched_at_most() {
    let (s, f) = (store(), Fakes::default());
    let agenda = Agenda::new(&s, &f, &f, &f);
    let all = [stuck("a"), stuck("b"), stuck("c"), stuck("d")];
    agenda.observe(Source::Sonarr, &all, T0).unwrap();
    let open = s.open_items().unwrap();
    let levels: Vec<_> = open.iter().map(|i| i.level).collect();
    assert_eq!(
        levels,
        [Level::Fyi, Level::Fyi, Level::Fyi, Level::NeedsYou]
    );
    assert_eq!(
        open[0].why,
        "Watching it; you'll hear if it hasn't cleared in 1 day."
    );
    assert!(open[3].why.starts_with("I'm already watching three things"));
    agenda
        .observe(Source::Sonarr, &all[..1], T0 + DAY_MS)
        .unwrap();
    agenda.tick(T0 + DAY_MS).unwrap();
    let open = s.open_items().unwrap();
    assert_eq!(open.len(), 1, "the ones that cleared closed");
    assert_eq!(open[0].level, Level::NeedsYou);
    assert_eq!(open[0].why, "Still the case after 1 day.");
}

#[test]
fn stop_teaches_a_rule_that_covers_the_family_until_it_is_undone() {
    let (s, f) = (store(), Fakes::default());
    *f.refuse.borrow_mut() = Some("no".to_owned());
    let agenda = Agenda::new(&s, &f, &f, &f);
    agenda
        .observe(Source::Docker, &[crashed("ebooks")], T0)
        .unwrap();
    let id = s.open_items().unwrap()[0].id;
    let rule = agenda.stop(id, T0 + MINUTE_MS).unwrap();
    assert_eq!(
        (rule.family, rule.about.as_str()),
        (Family::Container, "ebooks")
    );
    assert_eq!(
        last(&f),
        "You stopped it: Leave the container ebooks alone."
    );
    assert!(s.open_items().unwrap().is_empty());
    let mut unhealthy = crashed("ebooks");
    unhealthy.kind = Kind::ContainerUnhealthy;
    agenda
        .observe(
            Source::Docker,
            &[crashed("ebooks"), unhealthy],
            T0 + HOUR_MS,
        )
        .unwrap();
    assert!(
        s.open_items().unwrap().is_empty(),
        "left alone, whatever happens to it"
    );
    agenda.forget_rule(rule.id, T0 + 2 * HOUR_MS).unwrap();
    agenda
        .observe(Source::Docker, &[crashed("ebooks")], T0 + 3 * HOUR_MS)
        .unwrap();
    assert_eq!(s.open_items().unwrap().len(), 1);
}

#[test]
fn the_owner_re_levels_a_kind_within_what_it_allows() {
    let (s, f) = (store(), Fakes::default());
    let agenda = Agenda::new(&s, &f, &f, &f);
    assert!(
        agenda
            .set_response(Kind::MailMoneyDue, Some(Response::Handle), T0)
            .is_err()
    );
    agenda
        .set_response(Kind::ContainerCrashed, Some(Response::NeedsYou), T0)
        .unwrap();
    assert_eq!(last(&f), "You set \"A container crashed\" to Needs you.");
    agenda
        .observe(Source::Docker, &[crashed("sonarr")], T0)
        .unwrap();
    assert!(f.restarted.borrow().is_empty(), "asked, not restarted");
    assert_eq!(s.open_items().unwrap()[0].level, Level::NeedsYou);
    agenda
        .set_response(Kind::ContainerCrashed, None, T0)
        .unwrap();
    assert_eq!(
        agenda.response_for(Kind::ContainerCrashed).unwrap(),
        Response::Handle
    );
}

#[test]
fn mail_tells_never_reopens_once_answered_and_pushes_naming_nothing() {
    let (s, f) = (store(), Fakes::default());
    let agenda = Agenda::new(&s, &f, &f, &f);
    let security = mail(Kind::MailAccountSecurity, "m1", false);
    agenda
        .observe(Source::Mail, &[security.clone()], T0)
        .unwrap();
    let item = s.open_items().unwrap()[0].clone();
    assert_eq!(item.level, Level::Urgent);
    assert_eq!(
        f.pushed.borrow().as_slice(),
        ["Urgent mail needs a look. It's pinned in Endora."]
    );
    // Mail is not a state: absent from the next read, it stays.
    agenda.observe(Source::Mail, &[], T0 + MINUTE_MS).unwrap();
    assert_eq!(s.open_items().unwrap().len(), 1);
    agenda
        .answer(item.id, Answer::Done, T0 + 2 * MINUTE_MS)
        .unwrap();
    agenda
        .observe(Source::Mail, &[security], T0 + 3 * MINUTE_MS)
        .unwrap();
    assert!(
        s.open_items().unwrap().is_empty(),
        "answered stays answered"
    );
    assert_eq!(f.pushed.borrow().len(), 1);
}

#[test]
fn mail_that_may_not_be_genuine_is_never_more_than_fyi_and_an_fyi_lasts_a_day() {
    let (s, f) = (store(), Fakes::default());
    let agenda = Agenda::new(&s, &f, &f, &f);
    agenda
        .observe(
            Source::Mail,
            &[mail(Kind::MailAccountSecurity, "m2", true)],
            T0,
        )
        .unwrap();
    assert_eq!(s.open_items().unwrap()[0].level, Level::Fyi);
    assert!(f.pushed.borrow().is_empty());
    agenda
        .observe(Source::Mail, &[mail(Kind::MailMoneyDue, "m3", false)], T0)
        .unwrap();
    agenda.tick(T0 + DAY_MS).unwrap();
    let open = s.open_items().unwrap();
    assert_eq!(
        open.len(),
        1,
        "the FYI expired; what needs you stays until answered"
    );
    assert_eq!(open[0].level, Level::NeedsYou);
}

#[test]
fn five_pushes_a_day_at_most() {
    let (s, f) = (store(), Fakes::default());
    let agenda = Agenda::new(&s, &f, &f, &f);
    let urgent: Vec<Signal> = (0..7)
        .map(|i| mail(Kind::MailAccountSecurity, &format!("u{i}"), false))
        .collect();
    agenda.observe(Source::Mail, &urgent, T0).unwrap();
    assert_eq!(f.pushed.borrow().len(), 5);
    assert_eq!(
        last(&f),
        "Didn't push an urgent item: five pushes today already."
    );
    assert_eq!(
        s.open_items().unwrap().len(),
        7,
        "still pinned, just not pushed"
    );
}

#[test]
fn an_entry_moving_from_tomorrow_to_today_updates_its_one_item() {
    let (s, f) = (store(), Fakes::default());
    let agenda = Agenda::new(&s, &f, &f, &f);
    let entries = [("2026-10-17".to_owned(), "Dentist".to_owned())];
    let tomorrow = crate::domain::event_signals(&entries, "2026-10-16", "2026-10-17");
    let today = crate::domain::event_signals(&entries, "2026-10-17", "2026-10-18");
    agenda.observe(Source::Calendar, &tomorrow, T0).unwrap();
    let change = agenda
        .observe(Source::Calendar, &today, T0 + DAY_MS)
        .unwrap();
    assert_eq!((change.opened, change.updated), (0, 1));
    let open = s.open_items().unwrap();
    assert_eq!(open[0].sentence, "Today: Dentist");
}

#[test]
fn repeats_from_one_sender_fold_into_one_item_that_stays_answered() {
    let (s, f) = (store(), Fakes::default());
    let agenda = Agenda::new(&s, &f, &f, &f);
    let mut first = mail(Kind::MailAccountSecurity, "n1", false);
    first.about = "no-reply@example.com".to_owned();
    let mut second = first.clone();
    second.subject = "n2".to_owned();
    second.detail = "A newer sign-in.".to_owned();
    let change = agenda
        .observe(Source::Mail, &[first.clone(), second.clone()], T0)
        .unwrap();
    assert_eq!((change.opened, change.updated), (1, 1));
    let open = s.open_items().unwrap();
    assert_eq!(open.len(), 1, "one card, not a stream");
    assert_eq!(
        (open[0].sentence.as_str(), open[0].why.as_str()),
        ("A newer sign-in.", "More like this came in.")
    );
    assert_eq!(f.pushed.borrow().len(), 1, "one push for the lot");
    agenda
        .answer(open[0].id, Answer::Done, T0 + MINUTE_MS)
        .unwrap();
    agenda
        .observe(Source::Mail, &[first, second], T0 + 2 * MINUTE_MS)
        .unwrap();
    assert!(s.open_items().unwrap().is_empty(), "neither comes back");
}

#[test]
fn something_that_happened_hours_ago_is_pinned_but_never_pushed() {
    let (s, f) = (store(), Fakes::default());
    let agenda = Agenda::new(&s, &f, &f, &f);
    let mut old = mail(Kind::MailAccountSecurity, "old", false);
    old.at = Some(T0 - 7 * HOUR_MS);
    let fresh = mail(Kind::MailAccountSecurity, "fresh", false);
    agenda.observe(Source::Mail, &[old, fresh], T0).unwrap();
    let levels: Vec<_> = s
        .open_items()
        .unwrap()
        .iter()
        .map(|i| (i.level, i.pushed))
        .collect();
    assert_eq!(levels, [(Level::Urgent, false), (Level::Urgent, true)]);
    assert_eq!(f.pushed.borrow().len(), 1);
}

#[test]
fn a_backup_still_failing_a_day_after_it_was_told_is_urgent() {
    let (s, f) = (store(), Fakes::default());
    let agenda = Agenda::new(&s, &f, &f, &f);
    let stale = crate::domain::alert_signals(&[crate::domain::Alert {
        name: "BackupStale".into(),
        severity: "warning".into(),
        target: "system".into(),
        summary: "Backup plan system last succeeded 2 days ago".into(),
    }]);
    agenda.observe(Source::Monitoring, &stale, T0).unwrap();
    assert_eq!(s.open_items().unwrap()[0].level, Level::NeedsYou);
    agenda
        .observe(Source::Monitoring, &stale, T0 + DAY_MS)
        .unwrap();
    agenda.tick(T0 + DAY_MS).unwrap();
    let item = &s.open_items().unwrap()[0];
    assert_eq!(
        (item.level, item.why.as_str()),
        (Level::Urgent, "Still the case after 1 day.")
    );
    assert_eq!(f.pushed.borrow().len(), 1);
}
