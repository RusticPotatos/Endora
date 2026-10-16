use super::*;
use std::collections::BTreeSet;

fn container(name: &str, state: &str, health: &str, exit: Option<i32>) -> Container {
    Container {
        id: "a".repeat(64),
        name: name.to_owned(),
        state: state.to_owned(),
        health: health.to_owned(),
        exit,
    }
}

#[test]
fn a_container_already_stopped_when_the_loop_starts_is_left_alone() {
    let mut seen = BTreeSet::new();
    let signals = container_signals(
        &mut seen,
        &[container("ebooks", "exited", "unknown", Some(1))],
    );
    assert!(
        signals.is_empty(),
        "never seen running, so not Endora's business"
    );
}

#[test]
fn a_container_seen_running_that_exits_with_an_error_crashed() {
    let mut seen = BTreeSet::new();
    container_signals(
        &mut seen,
        &[container("sonarr", "running", "healthy", None)],
    );
    let signals = container_signals(
        &mut seen,
        &[container("sonarr", "exited", "unknown", Some(1))],
    );
    assert_eq!(signals.len(), 1);
    assert_eq!(signals[0].kind, Kind::ContainerCrashed);
    assert_eq!(signals[0].sentence(), "sonarr crashed (exit 1)");
    assert_eq!(signals[0].target.as_deref(), Some("a".repeat(64).as_str()));
    // Still down on the next look: still the same signal, so the item stays one item.
    let again = container_signals(
        &mut seen,
        &[container("sonarr", "exited", "unknown", Some(1))],
    );
    assert_eq!(again[0].key(), signals[0].key());
}

#[test]
fn a_deliberate_or_unknown_stop_is_noted_never_a_crash() {
    for exit in [Some(0), Some(137), Some(143), None] {
        let mut seen = BTreeSet::new();
        container_signals(&mut seen, &[container("plex", "running", "unknown", None)]);
        let signals = container_signals(&mut seen, &[container("plex", "exited", "unknown", exit)]);
        assert_eq!(signals[0].kind, Kind::ContainerStopped, "exit {exit:?}");
        assert_eq!(Kind::ContainerStopped.default_response(), Response::Note);
    }
}

#[test]
fn unhealthy_and_restarting_are_signals_and_healthy_is_not() {
    let mut seen = BTreeSet::new();
    let signals = container_signals(
        &mut seen,
        &[
            container("a", "running", "healthy", None),
            container("b", "running", "unhealthy", None),
            container("c", "restarting", "unknown", None),
            container("d", "running", "starting", None),
        ],
    );
    let kinds: Vec<_> = signals.iter().map(|s| (s.name.as_str(), s.kind)).collect();
    assert_eq!(
        kinds,
        [
            ("b", Kind::ContainerUnhealthy),
            ("c", Kind::ContainerLooping)
        ]
    );
}

#[test]
fn only_stuck_downloads_are_signals() {
    let queue = [
        QueuedDownload {
            title: "Show S01E01".into(),
            stuck: false,
        },
        QueuedDownload {
            title: "Film (2020)".into(),
            stuck: true,
        },
    ];
    let signals = download_signals(Source::Radarr, &queue);
    assert_eq!(signals.len(), 1);
    assert_eq!(
        signals[0].sentence(),
        "A download is stuck in Radarr: Film (2020)"
    );
}

#[test]
fn an_entry_keeps_its_identity_from_tomorrow_to_today() {
    let entries = [
        ("2026-10-17".to_owned(), "Dentist".to_owned()),
        ("2026-10-20".to_owned(), "Later".to_owned()),
    ];
    let tomorrow = event_signals(&entries, "2026-10-16", "2026-10-17");
    let today = event_signals(&entries, "2026-10-17", "2026-10-18");
    assert_eq!(tomorrow.len(), 1, "only today and tomorrow");
    assert_eq!(tomorrow[0].sentence(), "Tomorrow: Dentist");
    assert_eq!(today[0].sentence(), "Today: Dentist");
    assert_eq!(tomorrow[0].key(), today[0].key());
}

#[test]
fn a_container_is_restarted_once_an_hour_never_a_third_time_a_day_and_ten_in_all() {
    let now = 10 * DAY_MS;
    assert_eq!(may_restart(&[], &[], now), RestartVerdict::Restart);
    assert_eq!(
        may_restart(&[now - 30 * MINUTE_MS], &[], now),
        RestartVerdict::DidNotHold
    );
    assert_eq!(
        may_restart(&[now - 2 * HOUR_MS], &[], now),
        RestartVerdict::Restart
    );
    let twice = [now - 5 * HOUR_MS, now - 2 * HOUR_MS];
    assert_eq!(may_restart(&twice, &twice, now), RestartVerdict::KeepsDying);
    let yesterday = [now - 30 * HOUR_MS, now - 26 * HOUR_MS];
    assert_eq!(
        may_restart(&yesterday, &yesterday, now),
        RestartVerdict::Restart
    );
    let ten: Vec<i64> = (0..10).map(|i| now - 2 * HOUR_MS - i).collect();
    assert_eq!(may_restart(&[], &ten, now), RestartVerdict::DailyLimit);
}

#[test]
fn only_what_can_be_restarted_or_watched_may_be_set_so() {
    assert!(Kind::ContainerCrashed.allows(Response::Handle));
    assert!(
        !Kind::MailMoneyDue.allows(Response::Handle),
        "mail can only tell"
    );
    assert!(
        !Kind::ContainerStopped.allows(Response::Handle),
        "a deliberate stop is never undone"
    );
    assert!(!Kind::Event.allows(Response::LookInto));
    for kind in Kind::ALL {
        assert!(kind.allows(kind.default_response()), "{}", kind.name());
        assert!(kind.allows(Response::Note) && kind.allows(Response::Urgent));
        assert_eq!(Kind::from_name(kind.name()), Some(kind));
    }
}

#[test]
fn a_rule_covers_the_whole_family_about_the_same_thing() {
    let mut seen = BTreeSet::new();
    container_signals(
        &mut seen,
        &[container("Sonarr", "running", "unknown", None)],
    );
    let crashed = &container_signals(
        &mut seen,
        &[container("Sonarr", "exited", "unknown", Some(2))],
    )[0];
    assert!(covered(Family::Container, "sonarr", crashed));
    assert!(!covered(Family::Container, "radarr", crashed));
    assert!(!covered(Family::Download, "Sonarr", crashed));
}
