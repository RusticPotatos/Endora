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
    /// `(path, text)` of each GitHub Actions workflow at its head (ADR 0142, amended
    /// 2026-10-31); `None` from a node that did not tell them.
    pub(super) workflows: Option<Vec<(String, String)>>,
}

impl BranchFiles {
    /// As the setting keeps it.
    pub(super) fn kept(&self) -> String {
        serde_json::json!({ "changed": self.changed, "texts": self.texts,
                            "workflows": self.workflows })
        .to_string()
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
        let pairs = |v: &serde_json::Value| -> Vec<(String, String)> {
            v.as_array()
                .into_iter()
                .flatten()
                .filter_map(|pair| match strings(pair).as_slice() {
                    [path, text] => Some((path.clone(), text.clone())),
                    _ => None,
                })
                .collect()
        };
        Self {
            changed: strings(&kept["changed"]),
            texts: pairs(&kept["texts"]),
            workflows: kept["workflows"]
                .is_array()
                .then(|| pairs(&kept["workflows"])),
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
        workflows: Option<&[(String, String)]>,
    ) -> Done<()> {
        if changed.is_empty() && texts.is_empty() && workflows.is_none() {
            return Ok(());
        }
        let short = |t: &[(String, String)]| -> Vec<(String, String)> {
            t.iter()
                .filter(|(_, t)| t.chars().count() <= TEXT_MOST)
                .cloned()
                .collect()
        };
        let files = BranchFiles {
            changed: changed.to_vec(),
            texts: short(texts),
            // A workflow the node could not hand over whole (empty) or too long to keep is not
            // one HQ can say nothing runs: the workflows are kept as untold, so the check
            // cannot tell rather than misjudge.
            workflows: workflows
                .filter(|w| {
                    w.iter()
                        .all(|(_, t)| !t.trim().is_empty() && t.chars().count() <= TEXT_MOST)
                })
                .map(short),
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
        let worker = match task.holder() {
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
                    this_items: r.task == Some(task.id),
                })
                .collect();
            report.quotes = mechanical::quotes_checked(&post.text, &kept);
        }
        let Some(branch) = self.newest_branch(task.id)? else {
            return Ok(report);
        };
        let files = self.branch_files(&branch)?;
        report.check = self
            .store
            .checks()?
            .into_iter()
            .filter(|c| c.branch == branch.id)
            .max_by_key(|c| c.id)
            .map(|c| mechanical::HeadCheck {
                id: c.id,
                branch: branch.name.clone(),
                ok: c.ok.filter(|_| c.status == "done"),
                summary: c.summary,
            });
        let mut words = format!("{}\n{}", task.title, task.description);
        for c in &task.criteria {
            words.push('\n');
            words.push_str(&c.text);
        }
        report.branch = Some((
            branch.name.clone(),
            mechanical::scope(
                &files.changed,
                &words,
                self.outside_repo_of_task(task.id)?.is_some(),
            ),
        ));
        let criteria: Vec<&str> = task.criteria.iter().map(|c| c.text.as_str()).collect();
        report.lint = mechanical::asks_for_lint(&criteria);
        let asks_for_ci = mechanical::asks_for_ci(&criteria);
        if !asks_for_ci.is_empty() {
            report.ci = Some(mechanical::ci(
                &branch.name,
                asks_for_ci,
                files.workflows.as_deref(),
            ));
        }
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

    /// The newest branch of an item's work (its own or a sub-task's), if any.
    pub(super) fn newest_branch(&self, task: TaskId) -> Done<Option<crate::domain::Branch>> {
        let work = self.work_of(task)?;
        Ok(self
            .store
            .branches()?
            .into_iter()
            .filter(|b| work.contains(&b.task))
            .max_by_key(|b| b.id))
    }

    /// What the node told HQ of a branch's files, or, for a branch recorded before it told
    /// them, the names its document's diff shows.
    pub(super) fn branch_files(&self, branch: &crate::domain::Branch) -> Done<BranchFiles> {
        let kept: Option<serde_json::Value> =
            self.setting_json(&format!("{BRANCH_FILES}{}", branch.id))?;
        Ok(match kept {
            Some(kept) => BranchFiles::read(&kept),
            None => BranchFiles {
                changed: self.changed_in_document(branch.task, &branch.name)?,
                texts: Vec::new(),
                workflows: None,
            },
        })
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
        if let Some(worker) = task.holder() {
            self.wake_held_lock(worker, Some(task.id), WakeReason::SentBack)?;
        }
        if self.loop_holds(task)? {
            self.loop_failed(task.id)?;
        }
        Ok(true)
    }

    /// Refuses a pass of an item whose criteria ask for a CI run that no workflow at its
    /// branch's head runs on a push of (ADR 0142, amended 2026-10-31, D6): HQ decides that by
    /// rule, from the branch as it stands now, not the reviewer.
    pub(super) fn ci_can_be_had(&self, task: &Task) -> Done<()> {
        let report = self.mechanical_checks(task)?;
        let Some(ci) = report.no_ci_run() else {
            return Ok(());
        };
        Err(super::EngineError::Refused(
            crate::domain::Refusal::NoCiRun {
                branch: ci.branch.clone(),
                criteria: ci.criteria.clone(),
                alone: self.solo_of(task)?.is_some(),
            },
        ))
    }

    /// The item's latest mechanical checks, for its reviewer's briefing, in full; empty when
    /// none. The review's briefing carries no thread history, so it is said here, once.
    pub(super) fn mechanical_said(&self, task: TaskId) -> Done<String> {
        let said = self
            .store
            .setting(&format!("{REPORT}{}", task.0))?
            .unwrap_or_default();
        let said = said.trim_end();
        if said.is_empty() {
            return Ok(String::new());
        }
        Ok(format!(
            "{said}\nThese are facts HQ measured: weigh them; a failed check is reason to send \
             the work back unless the thread shows why it does not apply.\n"
        ))
    }
}
