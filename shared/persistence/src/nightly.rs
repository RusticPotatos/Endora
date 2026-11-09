//! Nightly copies of a database, made so the house's backup always finds a whole one (ADR 0098).
//!
//! A file copied while a service writes to it can look whole and fail to open on the day it is
//! needed. So at 01:30 by the person's clock the service writes a consistent, integrity-checked
//! copy beside the database (`daily-YYYY-MM-DD.db`, and on Sundays `weekly-YYYY-MM-DD.db`) and
//! keeps the newest seven of each. Files it did not name are never touched.
//!
//! The schedule, the names, the pruning and the copy are here once, for every store whose copy
//! Core owns: the node's `endora.db` and HQ's `hq.db` each run a [`Nightly`] from their own
//! ten-minute loop and say in their own words what happened. A copy is **proven by restore**
//! ([`crate::prove_restore`]) before it is named a daily; one that fails is deleted and tried
//! again on the next tick, at most [`MAX_ATTEMPTS_PER_DAY`] times a day.

use std::path::Path;
use std::sync::{Arc, Mutex, PoisonError};

/// When tonight's copy is due, in minutes after local midnight.
const COPY_AT_MINUTE: i64 = 90;

/// How many of each kind are kept.
const KEEP_EACH: usize = 7;

const DAY_MS: i64 = 24 * 60 * 60 * 1000;

/// The person's day as a count since 1970-01-01, at `now_ms` (`offset_minutes` from UTC then).
#[must_use]
pub fn local_day(now_ms: i64, offset_minutes: i64) -> i64 {
    (now_ms + offset_minutes * 60 * 1000).div_euclid(DAY_MS)
}

/// `YYYY-MM-DD` for a day count since 1970-01-01 (Howard Hinnant's civil-from-days).
fn civil(days: i64) -> String {
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    format!("{y:04}-{m:02}-{d:02}")
}

/// The copies due tonight, or `None` before 01:30 by the person's clock (`offset_minutes` from
/// UTC at `now_ms`). A daily always; a weekly too on a Sunday.
pub fn tonights_copies(now_ms: i64, offset_minutes: i64) -> Option<Vec<String>> {
    let local = now_ms + offset_minutes * 60 * 1000;
    let day = local_day(now_ms, offset_minutes);
    if local.rem_euclid(DAY_MS) < COPY_AT_MINUTE * 60 * 1000 {
        return None;
    }
    let date = civil(day);
    let mut names = vec![format!("daily-{date}.db")];
    // 1970-01-01 was a Thursday, so a day count of 3 more than a multiple of 7 is a Sunday.
    if day.rem_euclid(7) == 3 {
        names.push(format!("weekly-{date}.db"));
    }
    Some(names)
}

/// Whether `text` is exactly `YYYY-MM-DD`, as this module names its copies.
fn is_date(text: &str) -> bool {
    let b = text.as_bytes();
    b.len() == 10
        && b.iter().enumerate().all(|(i, c)| match i {
            4 | 7 => *c == b'-',
            _ => c.is_ascii_digit(),
        })
}

/// Which of `existing` to remove so the newest [`KEEP_EACH`] of each kind remain. Only names
/// this module writes are considered; anything else in the folder is left alone.
pub fn to_remove(existing: &[String]) -> Vec<String> {
    let ours = |kind: &str, suffix: &str| {
        let mut mine: Vec<&String> = existing
            .iter()
            .filter(|n| {
                n.strip_prefix(kind)
                    .and_then(|rest| rest.strip_suffix(suffix))
                    .is_some_and(is_date)
            })
            .collect();
        // The date is in the name, so the names sort oldest first.
        mine.sort();
        let surplus = mine.len().saturating_sub(KEEP_EACH);
        mine.into_iter().take(surplus).cloned().collect::<Vec<_>>()
    };
    let mut gone = ours("daily-", ".db");
    gone.extend(ours("weekly-", ".db"));
    gone
}

/// How many times a night the copy is tried before it gives up and says so.
pub const MAX_ATTEMPTS_PER_DAY: u32 = 3;

/// What one ten-minute tick of the nightly did.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Tick {
    /// Nothing was due, tonight's copy is already proven, or tonight's attempts are used up.
    Nothing,
    /// Tonight's copy was written, proven by restore and kept; its size in bytes.
    Proven {
        /// The size of the daily written.
        bytes: u64,
    },
    /// An attempt failed and its copy was removed. `giving_up` is true on the last attempt
    /// allowed tonight: the one failure worth telling the person about.
    Failed {
        /// What went wrong, in words for the person.
        reason: String,
        /// Whether this was the last attempt allowed tonight.
        giving_up: bool,
    },
}

impl Tick {
    /// What to tell the person about this tick, if anything, for the store named `subject`
    /// ("Endora's data", "HQ's data"): a line the night a copy is proven, and one line, however
    /// many tries it took, when the last try fails. The node says it in Activity, HQ in its log;
    /// the words are the same so the two cannot drift.
    #[must_use]
    pub fn summary(&self, subject: &str) -> Option<String> {
        match self {
            Self::Nothing
            | Self::Failed {
                giving_up: false, ..
            } => None,
            Self::Failed {
                reason,
                giving_up: true,
            } => Some(format!(
                "Couldn't save tonight's copy of {subject} after {MAX_ATTEMPTS_PER_DAY} tries: \
                 {reason}"
            )),
            Self::Proven { bytes } => {
                #[allow(clippy::cast_precision_loss)]
                let megabytes = *bytes as f64 / 1_000_000.0;
                Some(format!(
                    "Saved tonight's copy of {subject} ({megabytes:.1} MB), proven by restore: it \
                     was restored to a scratch file and checked whole, with the same tables and \
                     rows; the last 7 nights and 7 weeks are kept"
                ))
            }
        }
    }
}

/// The nightly's memory between ticks: which day it is trying, and how many attempts it has
/// used. Nothing else is kept in memory; what the proof has and has not seen is on disk.
///
/// A handle: clones share the one memory, so a loop can hand a clone to `spawn_blocking` each
/// tick.
#[derive(Debug, Default, Clone)]
pub struct Nightly {
    tried: Arc<Mutex<Tried>>,
}

#[derive(Debug, Default)]
struct Tried {
    day: Option<i64>,
    attempts: u32,
}

impl Nightly {
    /// A nightly that has not yet ticked.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// One tick, with the real copy and the real proof. Blocking: call it from
    /// `spawn_blocking`.
    pub fn tick(&self, database: &Path, folder: &Path, now_ms: i64, offset: i64) -> Tick {
        self.tick_with(
            database,
            folder,
            now_ms,
            offset,
            &|from, to| crate::snapshot(from, to).map_err(|e| e.to_string()),
            &|copy, live| crate::prove_restore(copy, live).map_err(|e| e.to_string()),
        )
    }

    /// [`tick`](Self::tick) with the copy and the proof given, so a test can break either.
    pub fn tick_with(
        &self,
        database: &Path,
        folder: &Path,
        now_ms: i64,
        offset: i64,
        copy: &dyn Fn(&Path, &Path) -> Result<(), String>,
        prove: &dyn Fn(&Path, &Path) -> Result<(), String>,
    ) -> Tick {
        let Some(names) = tonights_copies(now_ms, offset) else {
            return Tick::Nothing;
        };
        let day = local_day(now_ms, offset);
        // Held for the whole tick: a second tick never starts while one is copying.
        let mut tried = self.tried.lock().unwrap_or_else(PoisonError::into_inner);
        if tried.day != Some(day) {
            *tried = Tried {
                day: Some(day),
                attempts: 0,
            };
        }
        // A daily by its own name has been proven: it is only ever named once it is.
        if folder.join(&names[0]).exists() || tried.attempts >= MAX_ATTEMPTS_PER_DAY {
            return Tick::Nothing;
        }
        tried.attempts += 1;
        match attempt(database, folder, &names, copy, prove) {
            Ok(bytes) => Tick::Proven { bytes },
            Err(reason) => Tick::Failed {
                reason,
                giving_up: tried.attempts >= MAX_ATTEMPTS_PER_DAY,
            },
        }
    }
}

/// Where a database's copies go: `backups/` beside it.
#[must_use]
pub fn folder_of(database: &Path) -> std::path::PathBuf {
    database
        .parent()
        .unwrap_or_else(|| Path::new("."))
        .join("backups")
}

#[cfg(test)]
thread_local! {
    static LOG: std::cell::RefCell<Vec<String>> = const { std::cell::RefCell::new(Vec::new()) };
}

/// Takes what the durability steps logged on this thread, in order (tests only).
#[cfg(test)]
pub(crate) fn take_log() -> Vec<String> {
    LOG.with(|log| std::mem::take(&mut *log.borrow_mut()))
}

/// Notes a durability step for the tests that check their order; nothing in a real run.
#[allow(clippy::needless_pass_by_value)]
fn note(step: String) {
    #[cfg(test)]
    LOG.with(|log| log.borrow_mut().push(step));
    #[cfg(not(test))]
    let _ = step;
}

fn name_of(path: &Path) -> String {
    path.file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default()
}

/// Flushes a finished file to the disk, so its name is never durable before its bytes.
fn sync_file(path: &Path) -> Result<(), String> {
    std::fs::File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|e| format!("could not sync {}: {e}", path.display()))?;
    note(format!("sync {}", name_of(path)));
    Ok(())
}

/// Flushes a folder's entries (the renames and new names in it) to the disk.
fn sync_folder(folder: &Path) -> Result<(), String> {
    std::fs::File::open(folder)
        .and_then(|dir| dir.sync_all())
        .map_err(|e| format!("could not sync {}: {e}", folder.display()))?;
    note("sync-dir".to_owned());
    Ok(())
}

#[cfg(feature = "runtime")]
type Step = Arc<dyn Fn(&Path, &Path) -> Result<(), String> + Send + Sync>;

#[cfg(feature = "runtime")]
impl Nightly {
    /// One tick on a blocking thread, with the given copy and proof. A panic there is one
    /// attempt used (it was counted before the work began), said like any other failure.
    pub(crate) async fn blocking_tick(
        &self,
        database: std::path::PathBuf,
        folder: std::path::PathBuf,
        now_ms: i64,
        offset: i64,
        copy: Step,
        prove: Step,
    ) -> Tick {
        let me = self.clone();
        let ran = tokio::task::spawn_blocking(move || {
            me.tick_with(&database, &folder, now_ms, offset, &*copy, &*prove)
        })
        .await;
        ran.unwrap_or_else(|error| {
            let tried = self.tried.lock().unwrap_or_else(PoisonError::into_inner);
            Tick::Failed {
                reason: format!("the copy stopped unexpectedly: {error}"),
                giving_up: tried.attempts >= MAX_ATTEMPTS_PER_DAY,
            }
        })
    }

    /// The loop both stores run: every ten minutes, one tick with the real copy and proof on a
    /// blocking thread, into `backups/` beside `database`. `when` gives now (ms) and the
    /// person's UTC offset (minutes) at that moment; `tell` is called with every tick worth
    /// saying anything about (anything but [`Tick::Nothing`]).
    pub async fn run<W, T>(self, database: std::path::PathBuf, when: W, tell: T)
    where
        W: Fn() -> (i64, i64) + Send + 'static,
        T: Fn(&Tick) + Send + 'static,
    {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(10 * 60));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        let folder = folder_of(&database);
        let copy: Step = Arc::new(|from, to| crate::snapshot(from, to).map_err(|e| e.to_string()));
        let prove: Step =
            Arc::new(|copy, live| crate::prove_restore(copy, live).map_err(|e| e.to_string()));
        loop {
            ticker.tick().await;
            let (now_ms, offset) = when();
            let tick = self
                .blocking_tick(
                    database.clone(),
                    folder.clone(),
                    now_ms,
                    offset,
                    copy.clone(),
                    prove.clone(),
                )
                .await;
            if tick != Tick::Nothing {
                tell(&tick);
            }
        }
    }
}

/// The suffix of a daily that has been copied and not yet proven. The marker is the name: a
/// restart finds the file and proves it instead of copying again, and nothing else ever sees a
/// file by this name as a backup.
const UNPROVEN: &str = ".unproven";

/// One attempt at tonight's copy: copy (unless a restart left one to prove), prove, and only
/// then name it, make the weekly and prune. A copy that does not prove is deleted.
fn attempt(
    database: &Path,
    folder: &Path,
    names: &[String],
    copy: &dyn Fn(&Path, &Path) -> Result<(), String>,
    prove: &dyn Fn(&Path, &Path) -> Result<(), String>,
) -> Result<u64, String> {
    std::fs::create_dir_all(folder)
        .map_err(|e| format!("could not make {}: {e}", folder.display()))?;
    let daily = folder.join(&names[0]);
    let pending = folder.join(format!("{}{UNPROVEN}", names[0]));
    remove_old_unproven(folder, &pending);
    if !pending.exists() {
        copy(database, &pending)?;
    }
    let proved = prove(&pending, database)
        .map_err(|why| format!("tonight's copy did not restore, so it was removed: {why}"));
    let named = proved.and_then(|()| name_proven(folder, &pending, &daily, &names[1..]));
    if let Err(error) = named {
        let _ = std::fs::remove_file(&pending);
        return Err(error);
    }
    prune(folder);
    Ok(std::fs::metadata(&daily).map_err(|e| e.to_string())?.len())
}

/// Makes the weekly (the same proven bytes) and then names the daily.
fn name_proven(
    folder: &Path,
    pending: &Path,
    daily: &Path,
    weeklies: &[String],
) -> Result<(), String> {
    // Bytes first, names after: nothing is called proven until what was proven is on the disk.
    sync_file(pending)?;
    for weekly in weeklies {
        let target = folder.join(weekly);
        let made = std::fs::copy(pending, &target)
            .map_err(|e| e.to_string())
            .and_then(|_| sync_file(&target));
        if let Err(error) = made {
            let _ = std::fs::remove_file(&target);
            return Err(error);
        }
    }
    std::fs::rename(pending, daily).map_err(|e| e.to_string())?;
    note(format!("rename {}", name_of(daily)));
    sync_folder(folder)
}

/// An unproven daily from an earlier night is nobody's to prove now.
fn remove_old_unproven(folder: &Path, tonights: &Path) {
    let Ok(entries) = std::fs::read_dir(folder) else {
        return;
    };
    for entry in entries.filter_map(Result::ok) {
        let name = entry.file_name().to_string_lossy().into_owned();
        let ours = name
            .strip_prefix("daily-")
            .and_then(|rest| rest.strip_suffix(&format!(".db{UNPROVEN}")))
            .is_some_and(is_date);
        if ours && entry.path() != tonights {
            let _ = std::fs::remove_file(entry.path());
        }
    }
}

/// Keeps the newest [`KEEP_EACH`] of each kind. Runs only after a copy has proven, so the
/// newest proven copy is the newest there is and is never among those removed.
fn prune(folder: &Path) {
    let existing: Vec<String> = std::fs::read_dir(folder)
        .map(|entries| {
            entries
                .filter_map(|entry| entry.ok()?.file_name().into_string().ok())
                .collect()
        })
        .unwrap_or_default();
    for old in to_remove(&existing) {
        let _ = std::fs::remove_file(folder.join(old));
    }
}

#[cfg(test)]
#[path = "nightly_tests.rs"]
mod tests;
