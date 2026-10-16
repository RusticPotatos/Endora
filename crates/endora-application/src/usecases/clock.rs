//! Keeping the daily brief on the person's clock (it is stored as a UTC hour, which drifts by an
//! hour at every clock change).

use super::*;
use endora_capabilities::CapabilitySettingsRepository;

/// Where the offset the hours were last set under is kept, as one stored setting.
const SETTING: (&str, &str) = ("clock", "hours_set_under_minutes");

/// Moves the brief's UTC hour when the person's distance from UTC is no longer the one it was
/// set under, so it stays at the same local hour. The first time, it only records the current
/// offset: the hour was set under it. Returns whether anything moved.
///
/// # Errors
/// [`AppError::Repository`] if a schedule or the setting cannot be read or written.
pub fn keep_hours_on_the_clock(
    briefs: &dyn BriefScheduleRepository,
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
    record()?;
    Ok(true)
}

#[cfg(test)]
mod tests;
