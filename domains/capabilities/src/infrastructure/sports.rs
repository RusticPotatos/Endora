use super::*;

// ---- Sports schedules (live; ESPN's public schedule feed, no API key) ------

/// Reads a team's current-season schedule and returns the next games after a supplied
/// UTC date. The date is supplied by the application so tests and the running node agree
/// about "today"; the capability itself only owns the external lookup and parsing.
pub(super) struct SportsScheduleCapability;

pub(super) fn next_schedule_games(
    events: &[Value],
    team_id: &str,
    cutoff: i64,
    count: usize,
) -> Vec<Value> {
    let mut games: Vec<Value> = events
        .iter()
        .filter_map(|event| {
            let date = event["date"].as_str()?;
            if crate::application::schedule::utc_millis(date)? < cutoff {
                return None;
            }
            let competitors = event["competitions"].get(0)?["competitors"].as_array()?;
            let opponent = competitors
                .iter()
                .find(|c| c["id"].as_str() != Some(team_id))?;
            let opponent_name = opponent["team"]["displayName"].as_str()?;
            let home_away = event["competitions"].get(0)?["competitors"]
                .as_array()?
                .iter()
                .find(|c| c["id"].as_str() == Some(team_id))?["homeAway"]
                .as_str()
                .unwrap_or("unknown");
            let venue = event["competitions"].get(0)?["venue"]["fullName"]
                .as_str()
                .unwrap_or("");
            Some(json!({
                "id": event["id"].as_str()?,
                "date": date,
                "opponent": opponent_name,
                "home_away": home_away,
                "venue": venue,
                "name": event["name"].as_str().unwrap_or("")
            }))
        })
        .collect();
    games.sort_by_key(|game| {
        crate::application::schedule::utc_millis(game["date"].as_str().unwrap_or(""))
            .unwrap_or(i64::MAX)
    });
    games.truncate(count);
    games
}

impl Capability for SportsScheduleCapability {
    fn info(&self) -> CapabilityInfo {
        CapabilityInfo {
            id: "sports_schedule",
            name: "Sports schedule",
            description: "Find a team's current-season schedule and the next games from today.",
            category: "information",
            reaches_external: true,
            reversibility: Reversibility::Observe,
            configured: true,
            needs: "",
            settings: &[],
            wants_place: false,
            borrows_from: None,
            third_party: true,
            input_schema: Some(
                r#"{"type":"object","properties":{"team":{"type":"string","description":"team name or nickname, such as Carolina Panthers"},"after":{"type":"string","description":"UTC date YYYY-MM-DD; include games on or after this date"},"count":{"type":"integer","description":"number of games, 1-5 (default 3)"}},"required":["team","after"]}"#,
            ),
        }
    }

    fn invoke(
        &self,
        input: &Value,
        _settings: &CapabilitySettings,
    ) -> Result<Value, CapabilityError> {
        let query = str_field(input, "team")?;
        let after = str_field(input, "after")?;
        let after_date = crate::application::schedule::utc_millis(&format!("{after}T00:00:00Z"))
            .ok_or_else(|| {
                CapabilityError::BadInput("'after' must be a valid YYYY-MM-DD".to_owned())
            })?;
        let current = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?
            .as_millis() as i64;
        let cutoff = input["after_ms"]
            .as_i64()
            .unwrap_or(current)
            .max(after_date);
        let count = input
            .get("count")
            .and_then(Value::as_u64)
            .map_or(3, |n| n.clamp(1, 5)) as usize;

        // Resolve the nickname through the league's own team catalog. This handles
        // "Panthers" and full names without a hard-coded list, while still making the
        // selected team explicit in the result.
        let teams_url =
            "https://site.api.espn.com/apis/site/v2/sports/football/nfl/teams?limit=100";
        let teams: Value = serde_json::from_str(&http_get_text(teams_url, 512 * 1024)?)
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let wanted = normalize_words(query);
        let team = teams["sports"]
            .as_array()
            .into_iter()
            .flatten()
            .flat_map(|sport| sport["leagues"].as_array().into_iter().flatten())
            .flat_map(|league| league["teams"].as_array().into_iter().flatten())
            .filter_map(|entry| entry.get("team"))
            .find(|candidate| {
                let names = [
                    candidate["displayName"].as_str().unwrap_or(""),
                    candidate["shortDisplayName"].as_str().unwrap_or(""),
                    candidate["nickname"].as_str().unwrap_or(""),
                    candidate["abbreviation"].as_str().unwrap_or(""),
                ];
                names.iter().any(|name| {
                    let normalized = normalize_words(name);
                    normalized == wanted
                        || normalized.ends_with(&format!(" {wanted}"))
                        || wanted.ends_with(&format!(" {normalized}"))
                        || (wanted == "panthers" && normalized.contains("panthers"))
                })
            })
            .ok_or_else(|| {
                CapabilityError::BadInput(format!("couldn't find an NFL team called '{query}'"))
            })?;
        let team_id = team["id"].as_str().ok_or_else(|| {
            CapabilityError::Unavailable(
                "the sports source returned a team without an id".to_owned(),
            )
        })?;
        let team_name = team["displayName"].as_str().unwrap_or(query).to_owned();
        let schedule_url = format!(
            "https://site.api.espn.com/apis/site/v2/sports/football/nfl/teams/{team_id}/schedule"
        );
        let schedule: Value = serde_json::from_str(&http_get_text(&schedule_url, 512 * 1024)?)
            .map_err(|e| CapabilityError::Unavailable(e.to_string()))?;
        let events = schedule["events"].as_array().ok_or_else(|| {
            CapabilityError::Unavailable("the sports source returned no schedule".to_owned())
        })?;
        let games = next_schedule_games(events, team_id, cutoff, count);
        Ok(json!({
            "team": team_name,
            "team_id": team_id,
            "cutoff_ms": cutoff,
            "after": after,
            "count": games.len(),
            "games": games,
            "source": schedule_url
        }))
    }

    fn summarize(&self, output: &Value) -> String {
        let team = output["team"].as_str().unwrap_or("that team");
        let Some(games) = output["games"].as_array() else {
            return format!("I couldn't find a schedule for {team}.");
        };
        if games.is_empty() {
            return format!("I couldn't find any upcoming games for {team} from today onward.");
        }
        let mut text = format!("Next {} {} games from today:\n", games.len(), team);
        for (i, game) in games.iter().enumerate() {
            let date = game["date"].as_str().unwrap_or("").get(..10).unwrap_or("");
            let opponent = game["opponent"].as_str().unwrap_or("opponent");
            let venue = game["venue"]
                .as_str()
                .filter(|v| !v.is_empty())
                .unwrap_or("");
            let where_played = match game["home_away"].as_str() {
                Some("home") => "home",
                Some("away") => "away",
                _ => "",
            };
            let detail = [where_played, venue]
                .into_iter()
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(", ");
            text.push_str(&format!(
                "{}. {} vs {}{}\n",
                i + 1,
                date,
                opponent,
                if detail.is_empty() {
                    String::new()
                } else {
                    format!(" ({detail})")
                }
            ));
        }
        if let Some(source) = output["source"].as_str() {
            text.push_str(&format!("Source: {source}"));
        }
        text
    }
}
