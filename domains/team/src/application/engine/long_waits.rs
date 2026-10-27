//! Work that waits long for the Mac (the Phase 0 audit's P5): ADR 0131 keeps code, branch and
//! tester work off the house's NAS model and Claude, so it waits for the Mac however long that
//! takes, and the stuck ladder rightly leaves a waiting turn alone. Live, a tester's turns waited
//! ten hours overnight and nobody was told. Past [`mac::LONG_WAIT_MS`] the owner's digest says
//! so, once while the waiting lasts; the work keeps waiting.

use serde_json::json;

use super::{Done, Engine};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::{EventKind, Millis, RunStatus, mac};

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// The tick's look at turns waiting for the Mac: a note in the digest when the oldest has
    /// waited past [`mac::LONG_WAIT_MS`], once while any waits, and the mark cleared when none
    /// does, so the next long wait is said again.
    pub(super) fn long_mac_waits(&self, now: Millis) -> Done<()> {
        let waiting: Vec<_> = self
            .store
            .runs()?
            .into_iter()
            .filter(|r| r.status == RunStatus::Queued && mac::is_waiting_words(&r.end_reason))
            .collect();
        let said = self
            .store
            .setting(mac::LONG_WAIT_SAID)?
            .is_some_and(|v| !v.is_empty());
        let Some(oldest) = waiting.iter().map(|r| r.queued_at).min() else {
            if said {
                self.store.set_setting(mac::LONG_WAIT_SAID, "")?;
            }
            return Ok(());
        };
        if said || now - oldest < mac::LONG_WAIT_MS {
            return Ok(());
        }
        let members = self.store.members()?;
        let who: Vec<String> = waiting
            .iter()
            .map(|r| {
                let name = members
                    .iter()
                    .find(|m| m.id == r.member)
                    .map_or("someone", |m| m.name.as_str());
                match r.task {
                    Some(task) => format!("{name} on #{}", task.0),
                    None => name.to_owned(),
                }
            })
            .collect();
        let why = waiting
            .iter()
            .max_by_key(|r| r.queued_at)
            .map(|r| {
                r.end_reason
                    .trim_start_matches("waiting for the Mac: ")
                    .to_owned()
            })
            .unwrap_or_default();
        let hours = (now - oldest) / 3_600_000;
        let text = mac::long_wait_said(&who, hours, &why);
        let item = self
            .store
            .add_desk("notice", None, None, None, &text, now)?;
        self.quiet_in_the_loops(item)?;
        self.store
            .set_setting(mac::LONG_WAIT_SAID, &now.to_string())?;
        self.emit(
            EventKind::DeskAdded,
            json!({ "item": item.0, "kind": "notice", "why": "mac_long_wait",
                    "waiting": waiting.len() }),
        )?;
        Ok(())
    }
}
