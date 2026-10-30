//! What is left with each paid model provider (the owner, 2026-10-30): DeepSeek's balance as
//! DeepSeek says it, and Anthropic's as the figure the owner last read from Anthropic's Console
//! less what the relay has paid it since. The owner is told on the desk, once, when either
//! falls below its threshold. The rules are `endora_capabilities::credits`.
//!
//! A plan such as Claude Max grants API credit each billing cycle (the owner, 2026-10-30). The
//! owner may enter that grant, the day of the month the plan renews, and the credit they bought
//! on top. The estimate is two pots: what Endora pays comes off the grant first, then off the
//! bought credit; at the owner's midnight starting the renewal day (by `ENDORA_TIME_ZONE`, else
//! UTC) the estimate becomes the whole grant plus what is left of the bought credit, and the low
//! warning is armed again. A renewal is applied the next time the figures are looked at or a
//! charge is counted, before the charge, so the charge falls in the new cycle.

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
    /// Of `claude_set`, how much was credit the owner bought (spent after the grant).
    claude_purchased_at_set: i64,
    /// The plan's grant each cycle, in micros, when the owner has one.
    claude_grant: Option<i64>,
    /// The day of the month the plan renews (1–31).
    claude_renews_on: Option<u32>,
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
                "claude_purchased_at_set" => kept.claude_purchased_at_set = n.unwrap_or(0),
                "claude_grant" => kept.claude_grant = n.filter(|g| *g > 0),
                "claude_renews_on" => {
                    kept.claude_renews_on =
                        value.parse::<u32>().ok().filter(|d| (1..=31).contains(d));
                }
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

    /// Claude's pots now, given what Endora has paid Anthropic in all; `None` before the owner
    /// has entered anything.
    fn claude_pots(&self, spent: i64) -> Option<credits::Pots> {
        let (set, _) = self.claude_set?;
        Some(credits::claude_pots(
            set,
            self.claude_purchased_at_set,
            self.claude_spent_at_set,
            spent,
        ))
    }

    /// The grant and its renewal day, when the owner has both.
    fn plan(&self) -> Option<(i64, u32)> {
        Some((self.claude_grant?, self.claude_renews_on?))
    }
}

/// Starts Claude's estimate afresh at `now`: `set` in all, `purchased` of it bought, counted
/// from what Endora has paid by now. `rearm` clears the low warning's "told" mark.
fn anchor(state: &AppState, set: i64, purchased: i64, now: i64, rearm: bool, spent: i64) {
    keep(state, "claude_set", &set.to_string());
    keep(state, "claude_set_at", &now.to_string());
    keep(state, "claude_spent_at_set", &spent.to_string());
    keep(
        state,
        "claude_purchased_at_set",
        &purchased.min(set).to_string(),
    );
    if rearm {
        keep(state, "claude_told", "0");
    }
}

/// When the plan has renewed since Claude's estimate was last set: the estimate becomes the
/// whole grant plus what is left of the bought credit, and the warning is armed again.
fn renew_if_due(state: &AppState, now: i64) {
    let kept = Kept::load(state);
    let (Some((grant, renews_on)), Some((_, set_at))) = (kept.plan(), kept.claude_set) else {
        return;
    };
    let offset = state.time_zone.offset_minutes_at(now);
    if !credits::renewal_due(set_at, now, offset, renews_on) {
        return;
    }
    let spent = state.store.provider_spent("anthropic").unwrap_or(0);
    let Some(before) = kept.claude_pots(spent) else {
        return;
    };
    let (set, purchased) = credits::renew(grant, before);
    anchor(state, set, purchased, now, true, spent);
}

/// The next renewal day by the owner's clock, `YYYY-MM-DD`.
fn next_renewal(state: &AppState, now: i64, renews_on: u32) -> String {
    let offset = state.time_zone.offset_minutes_at(now);
    let today = (now + offset * 60_000).div_euclid(credits::DAY_MS);
    let (y, m, d) = credits::civil_from_days(credits::next_renewal(today, renews_on));
    format!("{y:04}-{m:02}-{d:02}")
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
    renew_if_due(state, now);
    let kept = Kept::load(state);
    let spent = state.store.provider_spent("anthropic").unwrap_or(0);
    let claude = kept
        .claude_set
        .zip(kept.claude_pots(spent))
        .map(|((set, at), pots)| {
            let left = pots.left();
            json!({ "left": left, "set": set, "set_at": at,
                    "grant_left": pots.grant, "purchased_left": pots.purchased,
                    "spent_since": (spent - kept.claude_spent_at_set).max(0),
                    "low": left < kept.claude_low() })
        });
    let plan = kept.plan();
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
        "claude_grant": plan.map(|(grant, _)| grant),
        "claude_renews_on": plan.map(|(_, day)| day),
        "claude_next_renewal": plan.map(|(_, day)| next_renewal(state, now, day)),
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
    after_charge_at(state, run, model, micros, state.clock.now().unix_millis());
}

/// Held while Claude's figures are read, renewed and charged, so two calls ending together
/// both land in the right cycle (review of 30 Oct).
static LEDGER: std::sync::Mutex<()> = std::sync::Mutex::new(());

fn ledger() -> std::sync::MutexGuard<'static, ()> {
    LEDGER
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// What the bench paid Anthropic (blocking): counted after any renewal due, so it lands in the
/// cycle it was spent in. The bench has no HQ turn to tell the owner through.
pub(super) fn after_bench_charge(state: &AppState, micros: i64) {
    let _held = ledger();
    renew_if_due(state, state.clock.now().unix_millis());
    if micros > 0 {
        if let Err(e) = state.store.add_provider_spent("anthropic", micros) {
            eprintln!("credits: could not count {micros} micros the bench paid Anthropic: {e}");
        }
    }
}

fn after_charge_at(state: &AppState, run: i64, model: &str, micros: i64, now: i64) {
    let _held = ledger();
    let provider = if model.starts_with("claude-") {
        "anthropic"
    } else if model.starts_with("deepseek") {
        "deepseek"
    } else {
        return;
    };
    if provider == "anthropic" {
        renew_if_due(state, now);
    }
    if micros > 0 {
        if let Err(e) = state.store.add_provider_spent(provider, micros) {
            eprintln!("credits: could not count {micros} micros paid to {provider}: {e}");
        }
    }
    let kept = Kept::load(state);
    let (left, low, told, key, who, top_up) =
        if provider == "anthropic" {
            let spent = state.store.provider_spent("anthropic").unwrap_or(0);
            let Some(pots) = kept.claude_pots(spent) else {
                return;
            };
            (
            pots.left(),
            kept.claude_low(),
            kept.claude_told,
            "claude_told",
            "Claude's credit (Anthropic) is down to about",
            kept.plan().map_or_else(
                || "top it up in Anthropic's Console, then enter the new balance in Settings → AI"
                    .to_owned(),
                |(grant, day)| {
                    format!(
                        "your plan's {} grant renews on {}; to go on before then, top it up in \
                         Anthropic's Console and enter what you bought in Settings → AI",
                        credits::dollars(grant),
                        next_renewal(state, now, day)
                    )
                },
            ),
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
                "top it up on DeepSeek's platform".to_owned(),
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

#[derive(Deserialize, Default)]
pub(super) struct Change {
    /// The Anthropic balance the owner reads in the Console now, in dollars.
    #[serde(default)]
    claude_balance: Option<f64>,
    /// Below what, in dollars, the owner is told.
    #[serde(default)]
    claude_low: Option<f64>,
    #[serde(default)]
    deepseek_low: Option<f64>,
    /// The plan's grant of API credit each cycle, in dollars (Claude Max: $100); 0 for none.
    #[serde(default)]
    claude_grant: Option<f64>,
    /// The day of the month the plan renews, 1–31 (a short month renews on its last day).
    #[serde(default)]
    claude_renews_on: Option<u32>,
    /// The credit the owner bought that is left now, in dollars: spent only after the grant.
    #[serde(default)]
    claude_purchased: Option<f64>,
}

fn micros_of(dollars: f64) -> Option<i64> {
    if !dollars.is_finite() || !(0.0..=100_000.0).contains(&dollars) {
        return None;
    }
    #[allow(clippy::cast_possible_truncation)]
    Some((dollars * 1_000_000.0).round() as i64)
}

/// Keeps the owner's change at `now` (blocking), or says why it is refused; nothing is kept
/// when any part is refused. A Console balance is split by what is bought (the figure given,
/// else what the estimate says is left of it); a bought figure alone keeps the grant's part as
/// estimated. With no estimate yet, a grant starts one at the grant plus what was bought.
/// Either figure re-arms the warning; a new grant or day re-counts from now, so a renewal that
/// passed before it is not applied again.
fn apply(state: &AppState, req: &Change, now: i64) -> Result<(), &'static str> {
    const MONEY: &str = "an amount is between $0 and $100,000";
    let money = |d: Option<f64>| d.map(|d| micros_of(d).ok_or(MONEY)).transpose();
    let balance = money(req.claude_balance)?;
    let purchased = money(req.claude_purchased)?;
    let grant = money(req.claude_grant)?;
    let lows = [
        ("claude_low", money(req.claude_low)?),
        ("deepseek_low", money(req.deepseek_low)?),
    ];
    if req.claude_renews_on.is_some_and(|d| !(1..=31).contains(&d)) {
        return Err("a plan renews on a day from 1 to 31");
    }
    let _held = ledger();
    renew_if_due(state, now);
    let kept = Kept::load(state);
    let grant_after = grant.or(kept.claude_grant).filter(|g| *g > 0);
    if grant_after.is_some() && req.claude_renews_on.or(kept.claude_renews_on).is_none() {
        return Err("a grant needs the day of the month the plan renews");
    }
    // A first grant over an estimate made before grants existed: how much of it is bought credit
    // is not known, so the owner says it (review of 30 Oct: guessing turned a $400 bought balance
    // into this month's grant). Refused before anything is kept.
    if grant.is_some_and(|g| g > 0)
        && kept.claude_grant.is_none()
        && kept.claude_set.is_some()
        && purchased.is_none()
        && balance.is_none()
    {
        return Err(
            "say how much of the balance is bought credit (\"bought credit left\"), or enter the Console's balance with it",
        );
    }
    if let Some(g) = grant {
        keep(state, "claude_grant", &g.to_string());
    }
    if let Some(day) = req.claude_renews_on {
        keep(state, "claude_renews_on", &day.to_string());
    }
    for (key, low) in lows {
        if let Some(low) = low {
            keep(state, key, &low.to_string());
        }
    }
    let spent = state.store.provider_spent("anthropic").unwrap_or(0);
    let now_pots = kept.claude_pots(spent);
    let replanned = grant.is_some() || req.claude_renews_on.is_some();
    let figures = balance.is_some() || purchased.is_some();
    match now_pots {
        Some(pots) if figures || replanned => {
            let bought = purchased.unwrap_or(pots.purchased);
            // Before any grant the estimate is one total, which the bought figure splits; with a
            // grant, a new bought figure changes only the bought part.
            let set = balance.unwrap_or(if kept.claude_grant.is_none() {
                pots.left()
            } else {
                pots.grant + bought
            });
            anchor(state, set, bought, now, figures, spent);
        }
        None if figures || grant_after.is_some() => {
            let bought = purchased.unwrap_or(0);
            let set = balance.unwrap_or(grant_after.unwrap_or(0) + bought);
            anchor(state, set, bought, now, true, spent);
        }
        _ => {}
    }
    Ok(())
}

/// `POST /v1/credits`: the owner enters the Anthropic balance the Console shows (after a
/// top-up, or to correct the estimate), the plan's monthly grant and renewal day, the credit
/// they bought, or a threshold. See [`apply`].
pub(super) async fn change(
    State(state): State<AppState>,
    Json(req): Json<Change>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let now = state.clock.now().unix_millis();
    apply(&state, &req, now).map_err(|m| {
        ApiError(AppError::BadRequest {
            message: m.to_owned(),
        })
    })?;
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
                ..Change::default()
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

    #[test]
    fn a_first_grant_over_an_older_estimate_asks_what_of_it_is_bought() {
        // Review of 30 Oct: an estimate entered before grants existed has no split; a grant set
        // over it without the bought figure would have turned bought credit into this month's
        // grant, and a false low warning would follow at renewal.
        let (hq_url, _) = hq_as(0, "developer");
        let state = super::super::relay::tests::node(&hq_url, "http://127.0.0.1:9");
        let now = 1_793_000_000_000;
        apply(
            &state,
            &Change {
                claude_balance: Some(400.0),
                ..Change::default()
            },
            now,
        )
        .unwrap();
        let guessed = Change {
            claude_grant: Some(100.0),
            claude_renews_on: Some(15),
            ..Change::default()
        };
        assert!(
            apply(&state, &guessed, now).is_err(),
            "asks for the bought figure"
        );
        let said = Change {
            claude_grant: Some(100.0),
            claude_renews_on: Some(15),
            claude_purchased: Some(300.0),
            ..Change::default()
        };
        apply(&state, &said, now).unwrap();
        let shown = shown(&state, now);
        assert_eq!(shown["claude"]["left"], 400_000_000, "{shown}");
        assert_eq!(shown["claude"]["purchased_left"], 300_000_000, "{shown}");
    }

    #[test]
    fn a_monthly_grant_is_spent_first_and_whole_again_on_the_owners_renewal_day() {
        use credits::{DAY_MS, days_from_civil};
        let (hq_url, to_hq) = hq_as(0, "developer");
        // The owner is five hours behind UTC.
        let state = super::super::relay::tests::node(&hq_url, "http://127.0.0.1:9")
            .with_time_zone(endora_infrastructure::TimeZone::fixed(-300));
        let hour = 3_600_000;
        let jan_20 = days_from_civil(2026, 1, 20) * DAY_MS + 12 * hour;
        let refused = |c: Change| apply(&state, &c, jan_20).is_err();
        assert!(
            refused(Change {
                claude_grant: Some(100.0),
                ..Change::default()
            }),
            "a grant needs its renewal day"
        );
        assert!(refused(Change {
            claude_grant: Some(100.0),
            claude_renews_on: Some(32),
            ..Change::default()
        }));
        assert!(refused(Change {
            claude_renews_on: Some(0),
            ..Change::default()
        }));
        assert!(shown(&state, jan_20)["claude"].is_null(), "nothing kept");
        apply(
            &state,
            &Change {
                claude_grant: Some(100.0),
                claude_renews_on: Some(31),
                claude_purchased: Some(30.0),
                ..Change::default()
            },
            jan_20,
        )
        .unwrap();
        let now = shown(&state, jan_20);
        assert_eq!(now["claude"]["left"], 130_000_000, "{now}");
        assert_eq!(now["claude"]["grant_left"], 100_000_000);
        assert_eq!(now["claude"]["purchased_left"], 30_000_000);
        assert_eq!(now["claude_grant"], 100_000_000);
        assert_eq!(now["claude_renews_on"], 31);
        assert_eq!(now["claude_next_renewal"], "2026-01-31");
        // The grant goes first, then what was bought.
        after_charge_at(&state, 7, "claude-opus-5-5", 110_000_000, jan_20 + hour);
        let now = shown(&state, jan_20 + hour);
        assert_eq!(now["claude"]["grant_left"], 0, "{now}");
        assert_eq!(now["claude"]["purchased_left"], 20_000_000);
        after_charge_at(&state, 7, "claude-opus-5-5", 16_000_000, jan_20 + hour);
        let notices = told(&to_hq);
        assert_eq!(notices.len(), 1, "told once");
        assert!(
            notices[0].contains("$100.00 grant renews on 2026-01-31"),
            "{notices:?}"
        );
        // 23:59 on the 30th by the owner's clock: not yet.
        let renews = days_from_civil(2026, 1, 31) * DAY_MS + 5 * hour;
        assert_eq!(shown(&state, renews - 60_000)["claude"]["left"], 4_000_000);
        // Midnight on the 31st by the owner's clock: the grant again, plus the $4 bought.
        let now = shown(&state, renews);
        assert_eq!(now["claude"]["left"], 104_000_000, "{now}");
        assert_eq!(now["claude"]["purchased_left"], 4_000_000);
        assert_eq!(
            now["claude_next_renewal"], "2026-02-28",
            "February is short"
        );
        assert_eq!(
            shown(&state, renews + hour)["claude"]["left"],
            104_000_000,
            "once"
        );
        // The warning is armed again.
        after_charge_at(&state, 7, "claude-opus-5-5", 101_000_000, renews + 2 * hour);
        assert_eq!(told(&to_hq).len(), 2, "told again after the renewal");
        // A Console figure entered later splits by what was bought.
        apply(
            &state,
            &Change {
                claude_balance: Some(50.0),
                ..Change::default()
            },
            renews + 3 * hour,
        )
        .unwrap();
        let now = shown(&state, renews + 3 * hour);
        assert_eq!(now["claude"]["purchased_left"], 3_000_000, "{now}");
        assert_eq!(now["claude"]["grant_left"], 47_000_000);
        // A charge just after the next renewal counts against the new cycle, not the old.
        let feb = days_from_civil(2026, 2, 28) * DAY_MS + 5 * hour;
        after_charge_at(&state, 7, "claude-opus-5-5", 1_000_000, feb + hour);
        let now = shown(&state, feb + hour);
        assert_eq!(now["claude"]["left"], 102_000_000, "{now}");
        // A grant of nothing turns renewals off.
        apply(
            &state,
            &Change {
                claude_grant: Some(0.0),
                ..Change::default()
            },
            feb + hour,
        )
        .unwrap();
        let now = shown(&state, feb + 40 * DAY_MS);
        assert!(now["claude_grant"].is_null(), "{now}");
        assert_eq!(now["claude"]["left"], 102_000_000);
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
