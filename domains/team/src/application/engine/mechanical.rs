//! HQ's mechanical checks when work reaches review (ADR 0142, amended 2026-10-30): the
//! worker's quotes against their receipts, the submitted branch's files against the paths
//! nobody touches and the paths the task names, and each role README against its defaults.
//! The rules are [`crate::domain::mechanical`]'s; this reads the record for them, posts the
//! report on the item, keeps it for the reviewer's briefing, and sends back a branch that
//! touches a forbidden path, with the reason, before any reviewer is woken.

use super::{Author, Done, Engine};
use crate::application::ports::{Clock, TeamStore, WorkerAdapter};
use crate::domain::mechanical::{self, Kept, Report};
use crate::domain::{Task, TaskId, TaskStatus, WakeReason};

/// The setting that holds what the node told HQ of a branch's files, by the branch's id.
pub(super) const BRANCH_FILES: &str = "branch_files:";
/// The setting that holds an item's latest report, by the task's id.
const REPORT: &str = "mechanical:";
/// The most of one file at a branch's head HQ keeps for its checks, in characters.
pub(super) const TEXT_MOST: usize = 32 * 1024;

/// What the node told HQ of a branch's files: their names, and the texts the checks read.
#[derive(Debug, Default)]
pub(super) struct BranchFiles {
    /// Every file it changes.
    pub(super) changed: Vec<String>,
    /// `(path, text)` at its head: each role README it changes and the role's defaults.
    pub(super) texts: Vec<(String, String)>,
}

impl BranchFiles {
    /// As the setting keeps it.
    pub(super) fn kept(&self) -> String {
        serde_json::json!({ "changed": self.changed, "texts": self.texts }).to_string()
    }

    /// From the setting.
    fn read(kept: &serde_json::Value) -> Self {
        let strings = |v: &serde_json::Value| -> Vec<String> {
            v.as_array()
                .into_iter()
                .flatten()
                .filter_map(|s| s.as_str().map(str::to_owned))
                .collect()
        };
        Self {
            changed: strings(&kept["changed"]),
            texts: kept["texts"]
                .as_array()
                .into_iter()
                .flatten()
                .filter_map(|pair| match strings(pair).as_slice() {
                    [path, text] => Some((path.clone(), text.clone())),
                    _ => None,
                })
                .collect(),
        }
    }
}

impl<S: TeamStore, W: WorkerAdapter, C: Clock> Engine<S, W, C> {
    /// Keeps what the node told of a submitted branch's files, for the checks at review. A
    /// file too long to keep whole is not kept: a cut one would be misjudged.
    pub(super) fn keep_branch_files(
        &self,
        branch: i64,
        changed: &[String],
        texts: &[(String, String)],
    ) -> Done<()> {
        if changed.is_empty() && texts.is_empty() {
            return Ok(());
        }
        let files = BranchFiles {
            changed: changed.to_vec(),
            texts: texts
                .iter()
                .filter(|(_, t)| t.chars().count() <= TEXT_MOST)
                .cloned()
                .collect(),
        };
        self.store
            .set_setting(&format!("{BRANCH_FILES}{branch}"), &files.kept())?;
        Ok(())
    }

    /// The mechanical checks of an item as it stands: its worker's newest post's quotes, its
    /// work's newest branch, and its defaults when its criteria ask for a README of them.
    ///
    /// # Errors
    /// A store failure.
    pub fn mechanical_checks(&self, task: &Task) -> Done<Report> {
        let mut report = Report::default();
        let worker = match task.owner.or(task.assignee) {
            Some(id) => Some(self.member(id)?.name),
            None => None,
        };
        let post = worker.and_then(|name| {
            self.store
                .thread(task.id)
                .ok()?
                .into_iter()
                .rev()
                .find(|p| p.author == name)
        });
        if let Some(post) = post {
            let mut receipts = Vec::new();
            for q in mechanical::quotes(&post.text) {
                if receipts
                    .iter()
                    .any(|r: &crate::domain::CallReceipt| r.id == q.receipt)
                {
                    continue;
                }
                if let Some(r) = self.store.call_receipt(q.receipt)? {
                    receipts.push(r);
                }
            }
            let kept: Vec<Kept<'_>> = receipts
                .iter()
                .map(|r| Kept {
                    id: r.id,
                    tool: &r.tool,
                    excerpt: &r.excerpt,
                })
                .collect();
            report.quotes = mechanical::quotes_checked(&post.text, &kept);
        }
        let work = self.work_of(task.id)?;
        let Some(branch) = self
            .store
            .branches()?
            .into_iter()
            .filter(|b| work.contains(&b.task))
            .max_by_key(|b| b.id)
        else {
            return Ok(report);
        };
        let kept: Option<serde_json::Value> =
            self.setting_json(&format!("{BRANCH_FILES}{}", branch.id))?;
        let files = match kept {
            Some(kept) => BranchFiles::read(&kept),
            None => BranchFiles {
                changed: self.changed_in_document(branch.task, &branch.name)?,
                texts: Vec::new(),
            },
        };
        let mut words = format!("{}\n{}", task.title, task.description);
        for c in &task.criteria {
            words.push('\n');
            words.push_str(&c.text);
        }
        report.branch = Some((
            branch.name.clone(),
            mechanical::scope(&files.changed, &words),
        ));
        let criteria: Vec<&str> = task.criteria.iter().map(|c| c.text.as_str()).collect();
        if !mechanical::asks_for_defaults(&criteria) {
            return Ok(report);
        }
        let text_of = |path: &str| {
            files
                .texts
                .iter()
                .find(|(p, _)| p == path)
                .map(|(_, t)| t.as_str())
        };
        for (readme, defaults) in mechanical::role_readmes(&files.changed) {
            match (text_of(&readme), text_of(&defaults)) {
                (Some(r), Some(d)) => report.coverage.push(mechanical::coverage(&readme, r, d)),
                _ => report.unchecked.push(readme),
            }
        }
        Ok(report)
    }

    /// The files a branch changes, from its document's diff, for a branch recorded before the
    /// node told HQ their names.
    fn changed_in_document(&self, task: TaskId, branch: &str) -> Done<Vec<String>> {
        let title = format!("branch {branch} (");
        Ok(self
            .store
            .documents_of(task)?
            .into_iter()
            .filter(|d| d.title.starts_with(&title))
            .max_by_key(|d| d.id)
            .map(|d| mechanical::changed_in_diff(&d.text))
            .unwrap_or_default())
    }

    /// Work reached review: its mechanical checks are run, posted on it and kept for its
    /// reviewer. A branch that touches a forbidden path goes back to its worker at once, with
    /// the reason, and counts as one of a loop item's failures. Returns whether it was sent
    /// back.
    pub(super) fn checked_at_review(&self, task: &Task) -> Done<bool> {
        let report = self.mechanical_checks(task)?;
        let key = format!("{REPORT}{}", task.id.0);
        if report.is_empty() {
            self.store.set_setting(&key, "")?;
            return Ok(false);
        }
        let said = report.said();
        self.store.set_setting(&key, &said)?;
        self.post_held_lock(Author::Endora, task.id, said.trim_end())?;
        if report.forbidden().is_empty() {
            return Ok(false);
        }
        self.store.set_task_status(task.id, TaskStatus::Doing)?;
        self.post_held_lock(
            Author::Endora,
            task.id,
            &format!(
                "Sent back by HQ: the branch touches {}, which holds the owner's own values and \
                 is never to be touched. Drop that change, submit the branch again, then ask for \
                 review.",
                report.forbidden().join(", ")
            ),
        )?;
        self.emit_status(task, TaskStatus::Doing)?;
        if let Some(worker) = task.owner.or(task.assignee) {
            self.wake_held_lock(worker, Some(task.id), WakeReason::SentBack)?;
        }
        if self.loop_holds(task)? {
            self.loop_failed(task.id)?;
        }
        Ok(true)
    }

    /// The item's latest mechanical checks, for its reviewer's briefing; empty when none. When
    /// the briefing already shows the report among the thread's new posts (`shown`), it is
    /// pointed to rather than said twice: small models pay for every line.
    pub(super) fn mechanical_said(&self, task: TaskId, shown: &[&str]) -> Done<String> {
        let said = self
            .store
            .setting(&format!("{REPORT}{}", task.0))?
            .unwrap_or_default();
        let said = said.trim_end();
        if said.is_empty() {
            return Ok(String::new());
        }
        let weigh = "These are facts HQ measured: weigh them; a failed check is reason to send \
                     the work back unless the thread shows why it does not apply.";
        if shown.iter().any(|s| s.trim_end() == said) {
            return Ok(format!(
                "HQ's mechanical checks of this work are in the thread above. {weigh}\n"
            ));
        }
        Ok(format!("{said}\n{weigh}\n"))
    }
}
