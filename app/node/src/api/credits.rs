//! What is left with each paid model provider (the owner, 2026-10-30): DeepSeek's balance as
//! DeepSeek says it, and Anthropic's as the figure the owner last read from Anthropic's Console
//! less what the relay has paid it since. The owner is told on the desk, once, when either
//! falls below its threshold. The rules are `endora_capabilities::credits`.

use super::*;
use endora_capabilities::credits;

/// The settings id the figures are kept under.
const CREDITS: &str = "credits";

/// How long a DeepSeek balance read is shown before it is asked again: ten minutes.
const DEEPSEEK_FRESH_MS: i64 = 10 * 60 * 1000;

/// The owner's figures, as kept.
#[derive(Default)]
struct Kept {
    /// The Anthropic balance the owner last read, in micros, and when.
    claude_set: Option<(i64, i64)>,
    /// What Endora had paid Anthropic when the owner read it.
    claude_spent_at_set: i64,
    claude_low: Option<i64>,
    deepseek_low: Option<i64>,
    claude_told: bool,
    deepseek_told: bool,
    /// DeepSeek's last answer and when it was read.
    deepseek_read: Option<(serde_json::Value, i64)>,
}

impl Kept {
    fn load(state: &AppState) -> Self {
        let mut kept = Self::default();
        let Ok(all) = state.config.all_settings() else {
            return kept;
        };
        let mut set = None;
        let mut set_at = 0;
        let mut read = None;
        let mut read_at = 0;
        for (id, key, value) in all {
            if id != CREDITS {
                continue;
            }
            let n = value.parse::<i64>().ok();
            match key.as_str() {
                "claude_set" => set = n,
                "claude_set_at" => set_at = n.unwrap_or(0),
                "claude_spent_at_set" => kept.claude_spent_at_set = n.unwrap_or(0),
                "claude_low" => kept.claude_low = n,
                "deepseek_low" => kept.deepseek_low = n,
                "claude_told" => kept.claude_told = value == "1",
                "deepseek_told" => kept.deepseek_told = value == "1",
                "deepseek_read" => read = serde_json::from_str(&value).ok(),
                "deepseek_read_at" => read_at = n.unwrap_or(0),
                _ => {}
            }
        }
        kept.claude_set = set.map(|s| (s, set_at));
        kept.deepseek_read = read.map(|r| (r, read_at));
        kept
    }

    fn claude_low(&self) -> i64 {
        self.claude_low.unwrap_or(credits::CLAUDE_LOW)
    }

    fn deepseek_low(&self) -> i64 {
        self.deepseek_low.unwrap_or(credits::DEEPSEEK_LOW)
    }
}

fn keep(state: &AppState, key: &str, value: &str) {
    if let Err(e) = state.config.set_setting(CREDITS, key, value) {
        eprintln!("credits: could not keep {key}: {e}");
    }
}

/// The DeepSeek provider on the owner's list, with its key: the first with a key whose address
/// is DeepSeek's or that the owner named DeepSeek.
fn deepseek(state: &AppState) -> Option<endora_capabilities::model_lists::Provider> {
    use endora_capabilities::model_lists::ModelListRepository as _;
    state.config.providers().ok()?.into_iter().find(|p| {
        !p.api_key.is_empty()
            && (p.url.contains("deepseek.com") || p.name.eq_ignore_ascii_case("deepseek"))
    })
}

/// DeepSeek's balance, read afresh when the last read is older than `fresh_ms` (blocking).
fn deepseek_balance(state: &AppState, now: i64, fresh_ms: i64) -> Option<(credits::Balance, i64)> {
    let kept = Kept::load(state);
    if let Some((answer, at)) = kept.deepseek_read.as_ref() {
        if now - at < fresh_ms {
            return credits::deepseek_balance(answer).map(|b| (b, *at));
        }
    }
    let provider = deepseek(state)?;
    match endora_infrastructure::relay::get_balance(&provider.url, &provider.api_key) {
        Ok(answer) => {
            keep(state, "deepseek_read", &answer.to_string());
            keep(state, "deepseek_read_at", &now.to_string());
            credits::deepseek_balance(&answer).map(|b| (b, now))
        }
        Err(e) => {
            eprintln!("credits: DeepSeek's balance could not be read: {e}");
            kept.deepseek_read
                .and_then(|(answer, at)| credits::deepseek_balance(&answer).map(|b| (b, at)))
        }
    }
}

/// What the console shows (blocking: it may ask DeepSeek).
fn shown(state: &AppState, now: i64) -> serde_json::Value {
    let kept = Kept::load(state);
    let spent = state.store.provider_spent("anthropic").unwrap_or(0);
    let claude = kept.claude_set.map(|(set, at)| {
        let left = credits::claude_left(set, kept.claude_spent_at_set, spent);
        json!({ "left": left, "set": set, "set_at": at,
                "spent_since": (spent - kept.claude_spent_at_set).max(0),
                "low": left < kept.claude_low() })
    });
    let deepseek = deepseek_balance(state, now, DEEPSEEK_FRESH_MS).map(|(b, at)| {
        #[allow(clippy::cast_possible_truncation)]
        let micros = (b.total * 1_000_000.0).round() as i64;
        json!({ "left": micros, "currency": b.currency, "available": b.available,
                "read_at": at, "low": micros < kept.deepseek_low() })
    });
    json!({
        "claude": claude,
        "deepseek": deepseek,
        "claude_low": kept.claude_low(),
        "deepseek_low": kept.deepseek_low(),
        "has_deepseek": deepseek_has_key(state),
    })
}

fn deepseek_has_key(state: &AppState) -> bool {
    deepseek(state).is_some()
}

/// After the relay paid a provider for a turn's call (blocking): counts it, and tells the
/// owner on the desk, once, when the balance has fallen below its threshold.
pub(super) fn after_charge(state: &AppState, run: i64, model: &str, micros: i64) {
    let provider = if model.starts_with("claude-") {
        "anthropic"
    } else if model.starts_with("deepseek") {
        "deepseek"
    } else {
        return;
    };
    if micros > 0 {
        if let Err(e) = state.store.add_provider_spent(provider, micros) {
            eprintln!("credits: could not count {micros} micros paid to {provider}: {e}");
        }
    }
    let now = state.clock.now().unix_millis();
    let kept = Kept::load(state);
    let (left, low, told, key, who, top_up) = if provider == "anthropic" {
        let Some((set, _)) = kept.claude_set else {
            return;
        };
        let spent = state.store.provider_spent("anthropic").unwrap_or(0);
        (
            credits::claude_left(set, kept.claude_spent_at_set, spent),
            kept.claude_low(),
            kept.claude_told,
            "claude_told",
            "Claude's credit (Anthropic) is down to about",
            "top it up in Anthropic's Console, then enter the new balance in Settings → AI",
        )
    } else {
        let Some((balance, _)) = deepseek_balance(state, now, DEEPSEEK_FRESH_MS) else {
            return;
        };
        #[allow(clippy::cast_possible_truncation)]
        let micros = (balance.total * 1_000_000.0).round() as i64;
        (
            micros,
            kept.deepseek_low(),
            kept.deepseek_told,
            "deepseek_told",
            "DeepSeek's credit is down to",
            "top it up on DeepSeek's platform",
        )
    };
    let (tell, mark) = credits::warn(left, low, told);
    if mark != told {
        keep(state, key, if mark { "1" } else { "0" });
    }
    if tell {
        super::contracts::tell_owner(
            state,
            run,
            &format!("{who} {}: {top_up}.", credits::dollars(left)),
        );
    }
}

/// `GET /v1/credits`: what is left with each paid provider.
pub(super) async fn status(State(state): State<AppState>) -> Json<serde_json::Value> {
    let now = state.clock.now().unix_millis();
    let looked = state.clone();
    let shown = tokio::task::spawn_blocking(move || shown(&looked, now))
        .await
        .unwrap_or_else(|_| json!({}));
    Json(shown)
}

#[derive(Deserialize)]
pub(super) struct Change {
    /// The Anthropic balance the owner reads in the Console now, in dollars.
    #[serde(default)]
    claude_balance: Option<f64>,
    /// Below what, in dollars, the owner is told.
    #[serde(default)]
    claude_low: Option<f64>,
    #[serde(default)]
    deepseek_low: Option<f64>,
}

fn micros_of(dollars: f64) -> Option<i64> {
    if !dollars.is_finite() || !(0.0..=100_000.0).contains(&dollars) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some((dollars * 1_000_000.0).round() as i64)
}

/// `POST /v1/credits`: the owner enters the Anthropic balance the Console shows (after a
/// top-up, or to correct the estimate), or changes a threshold. A new balance re-arms the
/// warning.
pub(super) async fn change(
    State(state): State<AppState>,
    Json(req): Json<Change>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let refused = |m: &str| {
        ApiError(AppError::BadRequest {
            message: m.to_owned(),
        })
    };
    let now = state.clock.now().unix_millis();
    if let Some(d) = req.claude_balance {
        let set = micros_of(d).ok_or_else(|| refused("a balance is between $0 and $100,000"))?;
        let spent = state.store.provider_spent("anthropic").unwrap_or(0);
        keep(&state, "claude_set", &set.to_string());
        keep(&state, "claude_set_at", &now.to_string());
        keep(&state, "claude_spent_at_set", &spent.to_string());
        keep(&state, "claude_told", "0");
    }
    for (key, value) in [
        ("claude_low", req.claude_low),
        ("deepseek_low", req.deepseek_low),
    ] {
        if let Some(d) = value {
            let low =
                micros_of(d).ok_or_else(|| refused("a threshold is between $0 and $100,000"))?;
            keep(&state, key, &low.to_string());
        }
    }
    Ok(status(State(state)).await)
}

#[cfg(test)]
mod tests {
    use super::super::relay::tests::{fake, hq_as};
    use super::*;

    fn told(seen: &super::super::relay::tests::Seen) -> Vec<String> {
        seen.lock()
            .unwrap()
            .iter()
            .filter(|(p, _, _)| p == "/v1/team/notice")
            .map(|(_, _, b)| {
                serde_json::from_str::<serde_json::Value>(b).unwrap()["text"]
                    .as_str()
                    .unwrap()
                    .to_owned()
            })
            .collect()
    }

    #[tokio::test]
    async fn claudes_balance_is_the_owners_figure_less_what_the_relay_paid_and_warns_once() {
        let (hq_url, to_hq) = hq_as(0, "developer");
        let state = super::super::relay::tests::node(&hq_url, "http://127.0.0.1:9");
        // Before the owner enters a figure there is nothing to show or warn of.
        after_charge(&state, 7, "claude-haiku-5-5", 2_000_000);
        assert!(shown(&state, 0)["claude"].is_null());
        let set = change(
            State(state.clone()),
            Json(Change {
                claude_balance: Some(10.0),
                claude_low: None,
                deepseek_low: None,
            }),
        )
        .await
        .ok()
        .expect("a balance is taken");
        assert_eq!(
            set.0["claude"]["left"], 10_000_000,
            "what was paid before is not counted"
        );
        after_charge(&state, 7, "claude-sonnet-5-5", 4_000_000);
        assert_eq!(shown(&state, 0)["claude"]["left"], 6_000_000);
        assert!(told(&to_hq).is_empty(), "above $5: nothing said");
        after_charge(&state, 7, "claude-sonnet-5-5", 1_500_000);
        after_charge(&state, 7, "claude-sonnet-5-5", 500_000);
        let notices = told(&to_hq);
        assert_eq!(notices.len(), 1, "told once: {notices:?}");
        assert!(
            notices[0].contains("$4.50") && notices[0].contains("Anthropic"),
            "{notices:?}"
        );
        // A local model's call is no provider's.
        after_charge(&state, 7, "qwen3.5:9b", 9_000_000);
        assert_eq!(shown(&state, 0)["claude"]["left"], 4_000_000);
    }

    #[tokio::test]
    async fn deepseeks_balance_is_read_from_deepseek_and_warns_when_low() {
        use endora_capabilities::model_lists::ModelListRepository as _;
        let (hq_url, to_hq) = hq_as(0, "developer");
        let (deepseek_url, asked) = fake(Arc::new(|path, _| match path {
            "/user/balance" => (
                200,
                "application/json",
                json!({ "is_available": true, "balance_infos": [
                    { "currency": "USD", "total_balance": "0.84" } ] })
                .to_string(),
            ),
            _ => (404, "application/json", "{}".to_owned()),
        }));
        let state = super::super::relay::tests::node(&hq_url, "http://127.0.0.1:9");
        // A fake on the house's network stands in for DeepSeek, named as the owner names it.
        state
            .config
            .add_provider("DeepSeek", &format!("{deepseek_url}/v1"), "a-key")
            .unwrap();
        let first = shown(&state, 1_000);
        assert_eq!(first["deepseek"]["left"], 840_000, "{first}");
        assert_eq!(first["deepseek"]["low"], true);
        let _ = shown(&state, 2_000);
        assert_eq!(asked.lock().unwrap().len(), 1, "read once in ten minutes");
        after_charge(&state, 7, "deepseek-flash", 2_000);
        let notices = told(&to_hq);
        assert_eq!(notices.len(), 1, "{notices:?}");
        assert!(notices[0].contains("DeepSeek") && notices[0].contains("$0.84"));
        assert_eq!(state.store.provider_spent("deepseek").unwrap(), 2_000);
    }
}
