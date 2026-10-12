//! Completion from structured tool observations, never from generated prose.
use crate::{GoalObservation, Timestamp};
use serde_json::Value;

/// Verifies and renders the next requested events from a recent successful read.
pub fn verified_schedule(
    observations: &[GoalObservation],
    team: &str,
    count: usize,
    now: Timestamp,
) -> Option<String> {
    for observation in observations.iter().rev() {
        if !matches!(observation.tool.as_str(), "sports_schedule" | "web_fetch")
            || observation.error.is_some()
        {
            continue;
        }
        let Some(age) = now.unix_millis().checked_sub(observation.at.unix_millis()) else {
            continue;
        };
        if !(0..=300_000).contains(&age) {
            continue;
        }
        let Some(raw) = observation.data.as_deref() else {
            continue;
        };
        let answer = if observation.tool == "web_fetch" {
            verify_calendar(raw, &observation.input, team, count, now.unix_millis())
        } else {
            verify(raw, team, count, now.unix_millis())
        };
        if let Some(answer) = answer {
            return Some(answer);
        }
    }
    None
}

fn team_matches(name: &str, wanted: &str) -> bool {
    let normalize = |s: &str| {
        s.split_whitespace()
            .collect::<Vec<_>>()
            .join(" ")
            .to_lowercase()
    };
    let name = normalize(name);
    let wanted = normalize(wanted);
    !wanted.is_empty() && (name == wanted || name.ends_with(&format!(" {wanted}")))
}

fn verify_calendar(raw: &str, input: &str, team: &str, count: usize, now: i64) -> Option<String> {
    let data: Value = serde_json::from_str(raw).ok()?;
    let input: Value = serde_json::from_str(input).ok()?;
    let source = data["url"].as_str()?;
    if input["url"].as_str()? != source || data.get("structured_error").is_some() {
        return None;
    }
    let events = data["sports_events"].as_array()?;
    let mut games = Vec::new();
    let mut selected = None;
    let mut ids = std::collections::HashSet::new();
    for event in events {
        let home = event["homeTeam"]["name"].as_str()?;
        let away = event["awayTeam"]["name"].as_str()?;
        let (name, opponent) = if team_matches(home, team) {
            (home, away)
        } else if team_matches(away, team) {
            (away, home)
        } else {
            continue;
        };
        if team_matches(opponent, team) {
            return None;
        }
        if selected.is_some_and(|prior| prior != name) {
            return None;
        }
        selected = Some(name);
        let date = event["startDate"].as_str()?;
        let start = endora_capabilities::application::schedule::utc_millis(date)?;
        let identity = event["@id"]
            .as_str()
            .map(str::to_owned)
            .unwrap_or_else(|| format!("{source}#{date}-{opponent}"));
        if identity.is_empty() || !ids.insert(identity.clone()) {
            return None;
        }
        if start < now {
            continue;
        }
        games.push((
            start,
            serde_json::json!({"id":identity,"date":date,"opponent":opponent}),
        ));
    }
    games.sort_by_key(|(start, _)| *start);
    if games.len() < count {
        return None;
    }
    let name = selected?;
    let games: Vec<Value> = games
        .into_iter()
        .take(count)
        .map(|(_, value)| value)
        .collect();
    verify(&serde_json::json!({"team":name,"team_id":name,"cutoff_ms":now,"count":count,"source":source,"games":games}).to_string(), team, count, now)
}

fn verify(raw: &str, team: &str, count: usize, now: i64) -> Option<String> {
    let data: Value = serde_json::from_str(raw).ok()?;
    // A future cutoff could omit earlier upcoming games.
    if data["cutoff_ms"].as_i64()? > now {
        return None;
    }
    let name = data["team"].as_str()?;
    if !team_matches(name, team) {
        return None;
    }
    if data["team_id"].as_str()?.is_empty() {
        return None;
    }
    let source = data["source"].as_str()?;
    let rest = source.strip_prefix("https://")?;
    let host = rest.split('/').next()?;
    if host.is_empty() || source.contains(char::is_whitespace) || host.contains('@') {
        return None;
    }
    let games = data["games"].as_array()?;
    if !(1..=20).contains(&count) || games.len() != count || data["count"].as_u64()? != count as u64
    {
        return None;
    }
    let mut ids = std::collections::HashSet::new();
    let mut particulars = std::collections::HashSet::new();
    let mut prior = now;
    let mut answer = format!("Next {count} {name} games:\n");
    for (n, game) in games.iter().enumerate() {
        let id = game["id"].as_str()?;
        let opponent = game["opponent"].as_str()?;
        let date = game["date"].as_str()?;
        let start = endora_capabilities::application::schedule::utc_millis(date)?;
        if id.is_empty()
            || opponent.trim().is_empty()
            || !ids.insert(id)
            || !particulars.insert((start, opponent))
            || start < prior
            || (n > 0 && start == prior)
        {
            return None;
        }
        prior = start;
        answer.push_str(&format!("{}. {date} vs {opponent}\n", n + 1));
    }
    answer.push_str(&format!("Source: {source}"));
    Some(answer)
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    #[test]
    fn a_fetched_calendar_sorts_future_games_and_requires_real_receipts() {
        let now = 1_914_969_600_000;
        let source = "https://example.com/schedule";
        let event = |date: &str, opponent: &str| {
            json!({"@type":"SportsEvent", "startDate":date,
            "homeTeam":{"name":"Example Team"},"awayTeam":{"name":opponent}})
        };
        let data = json!({"url":source,"sports_events":[
            event("2030-09-15T17:00:00Z","Wolves"), event("2030-09-01T17:00:00Z","Owls"), event("2030-09-08T17:00:00Z","Bears")
        ]});
        let input = json!({"url":source}).to_string();
        let result = verify_calendar(&data.to_string(), &input, "example team", 2, now).unwrap();
        assert!(result.find("Bears").unwrap() < result.find("Wolves").unwrap());
        assert!(!result.contains("Owls"));
        assert!(verify_calendar(&data.to_string(), "{}", "example team", 2, now).is_none());
        assert!(verify_calendar(&data.to_string(), &input, "example team", 3, now).is_none());
        for bad in [
            json!({"url":source,"text":"1. Bears 2. Wolves"}),
            json!({"url":source,"sports_events":[event("invalid","Bears")]}),
            json!({"url":source,"sports_events":[event("2030-09-08T17:00:00Z","Bears"),event("2030-09-08T17:00:00Z","Bears")]}),
            json!({"url":source,"sports_events":data["sports_events"],"structured_error":"Malformed"}),
        ] {
            assert!(verify_calendar(&bad.to_string(), &input, "example team", 2, now).is_none());
        }
    }
    fn fixture() -> Value {
        json!({"team":"Example Team","team_id":"fixture-team","cutoff_ms":1_914_969_600_000_i64,"count":3,"source":"https://example.com/schedule","games":[
        {"id":"one","opponent":"Bears","date":"2030-09-08T17:00:00Z"},
        {"id":"two","opponent":"Wolves","date":"2030-09-15T17:00:00Z"},
        {"id":"three","opponent":"Eagles","date":"2030-09-22T17:00:00Z"}]})
    }
    fn accepts(data: Value) -> bool {
        verify(&data.to_string(), "example team", 3, 1_914_969_600_000).is_some()
    }
    #[test]
    fn supported_schedule_completes() {
        assert!(accepts(fixture()));
    }
    #[test]
    fn unsupported_shapes_are_rejected() {
        for (field, value) in [
            ("team", json!("Owls")),
            ("team_id", json!("")),
            ("count", json!(2)),
            ("source", json!("invented")),
        ] {
            let mut data = fixture();
            data[field] = value;
            assert!(!accepts(data), "{field}");
        }
        let mut data = fixture();
        data["games"][0]["date"] = json!("2030-09-01T17:00:00Z");
        assert!(!accepts(data));
        let mut data = fixture();
        data["games"][1]["id"] = json!("one");
        assert!(!accepts(data));
        let mut data = fixture();
        data["games"].as_array_mut().unwrap().swap(0, 1);
        assert!(!accepts(data));
        let mut data = fixture();
        data["games"][0]["date"] = json!("2030-02-30T17:00:00Z");
        assert!(!accepts(data));
        let mut data = fixture();
        data["games"].as_array_mut().unwrap().pop();
        assert!(!accepts(data));
        let mut data = fixture();
        data["cutoff_ms"] = json!(1_914_969_600_001_i64);
        assert!(!accepts(data));
    }
    #[test]
    fn prose_cannot_supply_structured_evidence() {
        assert!(
            verify(
                "1. one\n2. two\n3. three\nSource: invented",
                "example team",
                3,
                0
            )
            .is_none()
        );
    }

    #[test]
    fn old_failed_or_other_tool_receipts_do_not_establish_completion() {
        let now = Timestamp::from_unix_millis(1_914_969_600_000);
        let receipt = GoalObservation {
            tool: "sports_schedule".to_owned(),
            input: "{}".to_owned(),
            output: "summary".to_owned(),
            data: Some(fixture().to_string()),
            error: None,
            at: now,
        };
        assert!(
            verified_schedule(std::slice::from_ref(&receipt), "example team", 3, now).is_some()
        );
        for changed in [
            GoalObservation {
                tool: "web_fetch".to_owned(),
                ..receipt.clone()
            },
            GoalObservation {
                error: Some("403".to_owned()),
                ..receipt.clone()
            },
            GoalObservation {
                at: Timestamp::from_unix_millis(now.unix_millis() - 300_001),
                ..receipt.clone()
            },
            GoalObservation {
                at: Timestamp::from_unix_millis(now.unix_millis() + 1),
                ..receipt.clone()
            },
            GoalObservation {
                data: None,
                ..receipt.clone()
            },
        ] {
            assert!(verified_schedule(&[changed], "example team", 3, now).is_none());
        }
        assert!(
            verified_schedule(
                &[
                    receipt.clone(),
                    GoalObservation {
                        data: None,
                        ..receipt
                    }
                ],
                "example team",
                3,
                now
            )
            .is_some()
        );
    }
}
