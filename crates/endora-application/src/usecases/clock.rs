//! Keeping the person's daily hours on their clock (the brief and the nightly loop are stored
//! as UTC hours, which drift by an hour at every clock change).

use super::*;
use endora_capabilities::CapabilitySettingsRepository;

/// Where the offset the hours were last set under is kept, as one stored setting.
const SETTING: (&str, &str) = ("clock", "hours_set_under_minutes");

/// Moves the brief's and the nightly loop's UTC hours when the person's distance from UTC is
/// no longer the one they were set under, so each stays at the same local hour. The first
/// time, it only records the current offset: the hours were set under it. Returns whether
/// anything moved.
///
/// # Errors
/// [`AppError::Repository`] if a schedule or the setting cannot be read or written.
pub fn keep_hours_on_the_clock(
    briefs: &dyn BriefScheduleRepository,
    nights: &dyn NightlyLoopScheduleRepository,
    settings: &dyn CapabilitySettingsRepository,
    offset_now_minutes: i64,
) -> Result<bool, AppError> {
    let set_under = settings
        .all_settings()?
        .into_iter()
        .find(|(id, key, _)| (id.as_str(), key.as_str()) == SETTING)
        .and_then(|(_, _, value)| value.parse::<i64>().ok());
    let record = || settings.set_setting(SETTING.0, SETTING.1, &offset_now_minutes.to_string());
    let Some(set_under) = set_under else {
        record()?;
        return Ok(false);
    };
    if set_under == offset_now_minutes {
        return Ok(false);
    }
    if let Some(mut brief) = briefs.get()? {
        brief.hour_utc =
            endora_scheduling::same_local_hour(brief.hour_utc, set_under, offset_now_minutes);
        briefs.set(&brief)?;
    }
    if let Some(mut night) = nights.get()? {
        night.hour_utc =
            endora_scheduling::same_local_hour(night.hour_utc, set_under, offset_now_minutes);
        nights.set(&night)?;
    }
    record()?;
    Ok(true)
}

#[cfg(test)]
mod tests;
