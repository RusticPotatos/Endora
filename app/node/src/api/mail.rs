//! The mail digest for the console (ADR 0097): what matters, by bucket, and how far the
//! reading has got. Only what Endora kept: sender, bucket, its checked sentence, why it
//! matters. Never a body.

use super::*;

/// How far back the console's view of the mail goes.
const SHOWN_MS: i64 = 14 * 24 * 60 * 60 * 1000;

pub(super) async fn mail_digest(
    State(state): State<AppState>,
) -> Result<Json<serde_json::Value>, ApiError> {
    let store = state.mail_store.clone();
    let mounted = state.mail_copy.is_some();
    let now = state.clock.now().unix_millis();
    let calendar = state.calendar.clone();
    let clock = state.clock.clone();
    let (digests, (indexed, unread), (upcoming, held_back)) = blocking(move || {
        let all = store.digests_since(now - usecases::READ_BACK_MS)?;
        Ok::<_, AppError>((
            all.iter()
                .filter(|d| d.at_ms >= now - SHOWN_MS)
                .map(|d| usecases::as_shown(d, &all))
                .collect::<Vec<_>>(),
            store.progress(now - usecases::READ_BACK_MS)?,
            usecases::dates_coming_up(&store, Some(calendar.as_ref()), clock.as_ref())?,
        ))
    })
    .await?;
    let item = |d: &endora_mail::Digest| {
        json!({
            "from": usecases::sender(d),
            "at_ms": d.at_ms,
            "bucket": d.bucket.name(),
            "sentence": d.sentence,
            "why": d.reasons.iter().map(|r| r.said()).collect::<Vec<_>>(),
            "amount": d.flags.amount.as_ref().filter(|_| d.flags.money_due),
            "dates": d.flags.dates.iter().map(|(date, what)| json!({"date": date, "what": what})).collect::<Vec<_>>(),
            "cautions": d.cautions,
            "appointment": d.appointment.as_ref().map(|(day, title)| json!({"day": day, "title": title})),
        })
    };
    let buckets: Vec<serde_json::Value> = endora_mail::Bucket::ALL
        .into_iter()
        .map(|b| {
            let mine: Vec<&endora_mail::Digest> =
                digests.iter().filter(|d| d.bucket == b).collect();
            json!({
                "name": b.name(),
                "title": b.title(),
                "count": mine.len(),
                "recent": mine.iter().take(20).map(|d| item(d)).collect::<Vec<_>>(),
            })
        })
        .collect();
    Ok(Json(json!({
        "mounted": mounted,
        "reading": state.own_work && state.mail_reader.is_some(),
        "indexed": indexed,
        "unread_recent": unread,
        "important": digests.iter().filter(|d| !d.reasons.is_empty()).map(item).collect::<Vec<_>>(),
        "buckets": buckets,
        "coming_up": upcoming.iter().map(|u| json!({
            "date": u.date, "what": u.what, "from": u.from, "because": u.because,
        })).collect::<Vec<_>>(),
        "held_back": held_back,
    })))
}
