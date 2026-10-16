//! What the agenda loop (ADR 0103) is fed: the home-lab observer's snapshot and the sorted
//! mail, turned into the agenda's signals. Composed here because it reads two contexts.

use endora_agenda::domain::{
    Alert, Container, Kind, QueuedDownload, Signal, Source, alert_signals, check_signals,
    container_signals, download_signals,
};
use endora_mail::{Digest, Reason};
use std::collections::BTreeSet;

/// How far back mail can still become an item.
pub const MAIL_RECENT_MS: i64 = 2 * 24 * 60 * 60 * 1000;

/// The signals of every service the home-lab observer read in full, from its snapshot. A
/// service it could not read (unconfigured, unavailable) is left out: no observation, so
/// nothing of its closes.
pub fn homelab_signals(
    snapshot: &serde_json::Value,
    seen_running: &mut BTreeSet<String>,
) -> Vec<(Source, Vec<Signal>)> {
    let services = snapshot["services"].as_array().cloned().unwrap_or_default();
    let mut out = Vec::new();
    for service in &services {
        if !matches!(service["state"].as_str(), Some("healthy" | "degraded")) {
            continue;
        }
        let items = service["items"].as_array().cloned().unwrap_or_default();
        let text = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_owned();
        match service["name"].as_str() {
            Some("docker") => {
                let list: Vec<Container> = items
                    .iter()
                    .filter(|c| !text(&c["name"]).is_empty())
                    .map(|c| Container {
                        id: text(&c["id"]),
                        name: text(&c["name"]),
                        state: text(&c["state"]),
                        health: text(&c["health"]),
                        exit: c["exit"].as_i64().and_then(|e| i32::try_from(e).ok()),
                    })
                    .collect();
                out.push((Source::Docker, container_signals(seen_running, &list)));
            }
            Some(name @ ("sonarr" | "radarr")) => {
                let source = if name == "sonarr" {
                    Source::Sonarr
                } else {
                    Source::Radarr
                };
                let queue: Vec<QueuedDownload> = items
                    .iter()
                    .map(|d| QueuedDownload {
                        title: text(&d["title"]),
                        stuck: d["stuck"].as_bool().unwrap_or(false),
                    })
                    .collect();
                out.push((source, download_signals(source, &queue)));
            }
            _ => {}
        }
    }
    out
}

/// The monitoring's signals, from the console's "watch" read: the firing alerts and the
/// failing uptime checks.
#[must_use]
pub fn monitoring_signals(watched: &serde_json::Value) -> Vec<Signal> {
    let text = |v: &serde_json::Value| v.as_str().unwrap_or_default().to_owned();
    let alerts: Vec<Alert> = watched["alerts"]
        .as_array()
        .into_iter()
        .flatten()
        .map(|a| Alert {
            name: text(&a["name"]),
            severity: text(&a["severity"]),
            target: text(&a["target"]),
            summary: text(&a["summary"]),
        })
        .collect();
    let checks: Vec<String> = watched["checks_failing"]
        .as_array()
        .into_iter()
        .flatten()
        .map(text)
        .collect();
    let mut signals = alert_signals(&alerts);
    signals.extend(check_signals(&checks));
    signals
}

/// The calendar's window for the agenda, in the owner's own time zone: from the start of
/// their today to the end of their tomorrow, as `(from_ms, to_ms, today, tomorrow)`.
#[must_use]
pub fn calendar_window(now_ms: i64, utc_offset_minutes: i64) -> (i64, i64, String, String) {
    const DAY: i64 = 24 * 60 * 60 * 1000;
    let offset = utc_offset_minutes * 60_000;
    let local = now_ms + offset;
    let start = local - local.rem_euclid(DAY) - offset;
    let today = endora_understanding::domain::brain::date_of(local);
    let tomorrow = endora_understanding::domain::brain::date_of(local + DAY);
    (start, start + 2 * DAY, today, tomorrow)
}

/// The sorted mail's signals: one per recent message that matters, at the kind its strongest
/// reason gives. The words are the reader's checked line; the sender is what a Stop covers.
#[must_use]
pub fn mail_signals(digests: &[Digest], now_ms: i64) -> Vec<Signal> {
    digests
        .iter()
        .filter(|d| now_ms - d.at_ms <= MAIL_RECENT_MS)
        .filter_map(|d| {
            let has = |r: Reason| d.reasons.contains(&r);
            let kind = if has(Reason::AccountSecurity) {
                Kind::MailAccountSecurity
            } else if has(Reason::MoneyDue) {
                Kind::MailMoneyDue
            } else if has(Reason::WaitingOnYou)
                || ((has(Reason::ReplyInYourThread) || has(Reason::FromSomeoneYouWriteTo))
                    && d.flags.needs_reply)
            {
                Kind::MailWaitingOnYou
            } else if has(Reason::ReplyInYourThread) || has(Reason::FromSomeoneYouWriteTo) {
                Kind::MailFromAPerson
            } else if has(Reason::Appointment) {
                Kind::MailAppointment
            } else if has(Reason::OrderProblem) {
                Kind::MailOrderProblem
            } else if has(Reason::YourApp) {
                Kind::MailAboutYourApp
            } else {
                return None;
            };
            let name = if d.from_name.trim().is_empty() {
                d.from_address.clone()
            } else {
                d.from_name.clone()
            };
            Some(Signal {
                kind,
                subject: d.key.clone(),
                about: d.from_address.trim().to_lowercase(),
                name,
                detail: super::mail_line(d),
                target: None,
                cautious: has(Reason::MayNotBeGenuine),
                at: Some(d.at_ms),
            })
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use endora_mail::{Bucket, Flags};
    use serde_json::json;

    fn digest(key: &str, reasons: Vec<Reason>, needs_reply: bool) -> Digest {
        Digest {
            key: key.to_owned(),
            at_ms: 1_000,
            from_address: "Jane.Doe@Example.com".to_owned(),
            from_name: "Jane Doe".to_owned(),
            bucket: Bucket::People,
            reasons,
            sentence: Some("Asks whether Friday still works.".to_owned()),
            flags: Flags {
                needs_reply,
                ..Flags::default()
            },
            read_ok: true,
            cautions: Vec::new(),
            appointment: None,
        }
    }

    #[test]
    fn the_strongest_reason_decides_and_a_sender_is_what_stop_covers() {
        let digests = [
            digest(
                "a",
                vec![Reason::FromSomeoneYouWriteTo, Reason::AccountSecurity],
                false,
            ),
            digest("b", vec![Reason::ReplyInYourThread], true),
            digest("c", vec![Reason::ReplyInYourThread], false),
            digest("f", vec![Reason::FromSomeoneYouWriteTo], true),
            digest("d", vec![], false),
            digest("e", vec![Reason::MoneyDue, Reason::MayNotBeGenuine], false),
        ];
        let signals = mail_signals(&digests, 2_000);
        let kinds: Vec<_> = signals
            .iter()
            .map(|s| (s.subject.as_str(), s.kind))
            .collect();
        assert_eq!(
            kinds,
            [
                ("a", Kind::MailAccountSecurity),
                ("b", Kind::MailWaitingOnYou),
                ("c", Kind::MailFromAPerson),
                ("f", Kind::MailWaitingOnYou),
                ("e", Kind::MailMoneyDue),
            ]
        );
        assert_eq!(signals[0].about, "jane.doe@example.com");
        assert!(signals[4].cautious && !signals[0].cautious);
        assert!(
            signals[1]
                .sentence()
                .contains("Asks whether Friday still works.")
        );
    }

    #[test]
    fn the_calendar_window_is_the_owners_today_and_tomorrow() {
        // 2026-10-17 02:00 UTC is still the 16th, 22:00, four hours behind.
        let now = 1_792_202_400_000;
        let (from, to, today, tomorrow) = calendar_window(now, -240);
        assert_eq!(
            (today.as_str(), tomorrow.as_str()),
            ("2026-10-16", "2026-10-17")
        );
        assert_eq!(
            from, 1_792_123_200_000,
            "the 16th at 00:00 there, 04:00 UTC"
        );
        assert_eq!(to - from, 2 * 24 * 60 * 60 * 1000);
    }

    #[test]
    fn the_watch_read_becomes_alert_and_check_signals() {
        let watched = json!({"alerts": [{"name": "FilesystemFull", "severity": "critical",
            "target": "/", "summary": "/ is 97% full"}], "checks_failing": ["Plex"]});
        let kinds: Vec<_> = monitoring_signals(&watched)
            .iter()
            .map(|s| s.kind)
            .collect();
        assert_eq!(kinds, [Kind::DiskFull, Kind::CheckFailing]);
    }

    #[test]
    fn old_mail_is_not_an_item() {
        let old = digest("a", vec![Reason::MoneyDue], false);
        assert!(mail_signals(&[old], 1_000 + MAIL_RECENT_MS + 1).is_empty());
    }

    #[test]
    fn only_services_read_in_full_are_observations() {
        let mut seen = BTreeSet::new();
        let snapshot = json!({"services": [
            {"name": "docker", "state": "degraded", "items": [
                {"id": "a", "name": "sonarr", "state": "running", "health": "unhealthy"}]},
            {"name": "sonarr", "state": "unavailable", "error": "connection", "items": []},
            {"name": "radarr", "state": "healthy", "items": [
                {"title": "Film", "stuck": true}, {"title": "Other", "stuck": false}]}
        ]});
        let got = homelab_signals(&snapshot, &mut seen);
        let sources: Vec<_> = got.iter().map(|(s, v)| (*s, v.len())).collect();
        assert_eq!(sources, [(Source::Docker, 1), (Source::Radarr, 1)]);
        assert_eq!(got[0].1[0].kind, Kind::ContainerUnhealthy);
        assert!(seen.contains("sonarr"));
    }
}
