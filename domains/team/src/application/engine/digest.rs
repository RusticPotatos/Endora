//! The owner's desk as the screen shows it (ADR 0132): what needs their word, and the digest of
//! what they may want to look at but need not. Live, over 17 and 18 October, 30 items reached the
//! desk in 38.6 hours and six were real decisions or approvals; stuck work, a guard's finding and
//! a notice each became a card, and the same task was flagged four times. Now the desk holds what
//! interrupts, and the rest waits in one "while you were away" digest, newest first, each item
//! with the choices it always had.

use serde_json::{Value, json};

use super::{Done, Engine};
use crate::application::ports::{Clock, DeskItem, TeamStore, WorkerAdapter};
use crate::domain::chain::Decision;
use crate::domain::desk;

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// One waiting desk item as the screen shows it: what it says and what it is about, and the
    /// other tasks that put the same ask with it (ADR 0124).
    fn desk_item_shown(&self, d: &DeskItem) -> Done<Value> {
        let mut item = json!({
            "id": d.id.0, "kind": d.kind, "task": d.task.map(|t| t.0),
            "goal": d.goal.map(|g| g.0), "project": d.project.map(|p| p.0),
            "text": d.text, "at": d.at,
            "also": self.store.attached(d.id.0)?.iter().map(|(t, _)| t.0).collect::<Vec<_>>(),
        });
        // The CEO's decision for the board, in its three parts (ADR 0111).
        if let Some(decided) = (d.kind == "decision")
            .then(|| Decision::read(&d.text))
            .flatten()
        {
            item["decision"] = json!({ "decision": decided.decision,
                "recommendation": decided.recommendation, "why": decided.why });
        }
        Ok(item)
    }

    /// What waits on the owner and needs their word: a decision, an approval, an answer, a push,
    /// money (ADR 0132), oldest first.
    pub(super) fn desk_shown(&self) -> Done<Value> {
        let mut items = Vec::new();
        for d in self
            .store
            .desk()?
            .iter()
            .filter(|d| d.status == "waiting" && desk::interrupts(&d.kind))
        {
            items.push(self.desk_item_shown(d)?);
        }
        Ok(Value::Array(items))
    }

    /// "While you were away" (ADR 0132): stuck work, a guard's finding and a notice, newest first.
    /// Each keeps the choices it has always had, and acting on one takes it out. One that is a
    /// finding says which, and how often it has been found.
    pub(super) fn digest_shown(&self) -> Done<Value> {
        let findings = self.store.findings()?;
        let mut waiting: Vec<DeskItem> = self
            .store
            .desk()?
            .into_iter()
            .filter(|d| d.status == "waiting" && !desk::interrupts(&d.kind))
            .collect();
        waiting.sort_by_key(|d| std::cmp::Reverse((d.at, d.id.0)));
        let mut items = Vec::new();
        for d in &waiting {
            let mut item = self.desk_item_shown(d)?;
            if let Some(f) = findings.iter().find(|f| f.desk == Some(d.id.0)) {
                item["finding"] = json!({ "id": f.id, "guard": f.guard.name(),
                    "episodes": f.episodes, "seen_at": f.seen_at });
            }
            items.push(item);
        }
        Ok(Value::Array(items))
    }
}
