//! The brain as a folder of markdown files with its own git history (ADR 0101).
//!
//! The folder can be edited from outside (a share, an editor on the Mac), and the node
//! runs as root, so two things are never trusted: what is in `.git` (every call turns
//! hooks and fsmonitor off, and ignores system and global configuration) and what a name
//! points at (a symlink is never read or written through).

use std::fs;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::Mutex;

use endora_kernel::RepositoryError;
use endora_kernel::ids::{PreferenceId, Timestamp};

use crate::application::{Brain, PageChange, PageInfo, PreferenceRepository};
use crate::domain::brain::{self, PagePath};
use crate::domain::{Preference, PreferenceKind};

/// The largest page the brain will read. A page is notes, not a document; anything
/// bigger was not written by Endora and is not handed to a model.
const LARGEST_PAGE: u64 = 256 * 1024;

/// What `log.md` starts as.
const NEW_LOG: &str = "# Log\n\nWhat changed in the brain, newest last.\n";

/// The brain on disk. One lock serialises every change, so a read-modify-write
/// cannot interleave with another.
pub struct BrainFolder {
    root: PathBuf,
    lock: Mutex<()>,
    /// Removed when dropped: a brain made for a test.
    temporary: bool,
}

impl Drop for BrainFolder {
    fn drop(&mut self) {
        if self.temporary {
            let _ = fs::remove_dir_all(&self.root);
        }
    }
}

impl BrainFolder {
    /// Opens the brain at `root`, starting it (folder, history, memory, index and log)
    /// if it is not there yet, and committing anything edited outside since.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the folder or its history cannot be made.
    pub fn open(root: &Path) -> Result<Self, RepositoryError> {
        fs::create_dir_all(root).map_err(io)?;
        let folder = Self {
            root: root.to_path_buf(),
            lock: Mutex::new(()),
            temporary: false,
        };
        folder.start()?;
        Ok(folder)
    }

    /// A brain in a new folder under the system's temporary directory, removed with it
    /// when dropped. For tests, which then use the real folder and history.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the folder or its history cannot be made.
    pub fn temporary() -> Result<Self, RepositoryError> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static MADE: AtomicUsize = AtomicUsize::new(0);
        let root = std::env::temp_dir().join(format!(
            "endora-brain-temporary-{}-{}",
            std::process::id(),
            MADE.fetch_add(1, Ordering::SeqCst)
        ));
        let _ = fs::remove_dir_all(&root);
        let mut folder = Self::open(&root)?;
        folder.temporary = true;
        Ok(folder)
    }

    fn start(&self) -> Result<(), RepositoryError> {
        let new = !self.root.join(".git").exists();
        if new {
            // No template: nothing, not even sample hooks, is copied in.
            self.git(&["init", "-q", "-b", "main", "--template="])?;
        }
        if self.text(&PagePath::memory().file())?.is_none() {
            self.put(&PagePath::memory().file(), &brain::new_memory())?;
        }
        if self.text("log.md")?.is_none() {
            self.put("log.md", NEW_LOG)?;
        }
        self.put("index.md", &self.render_index()?)?;
        self.commit(if new {
            "Start the brain"
        } else {
            "Edited outside Endora"
        })
    }

    /// Runs git on this folder alone. Hooks, fsmonitor, signing and every configuration
    /// file but the folder's own are off: the folder is writable from outside, and a hook
    /// planted there would otherwise run as the node.
    fn git(&self, args: &[&str]) -> Result<String, RepositoryError> {
        let root = self.root.to_string_lossy().into_owned();
        let output = Command::new("git")
            .arg("-C")
            .arg(&self.root)
            .args(["-c", "core.hooksPath=/dev/null"])
            .args(["-c", "core.fsmonitor=false"])
            .args(["-c", "commit.gpgsign=false"])
            .args(["-c", "user.name=Endora"])
            .args(["-c", "user.email=endora@localhost"])
            .args(["-c", &format!("safe.directory={root}")])
            .args(args)
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_TERMINAL_PROMPT", "0")
            .env_remove("GIT_DIR")
            .env_remove("GIT_WORK_TREE")
            .env_remove("GIT_INDEX_FILE")
            .output()
            .map_err(|e| RepositoryError::Backend(format!("git could not run: {e}")))?;
        if !output.status.success() {
            return Err(RepositoryError::Backend(format!(
                "git {}: {}",
                args.first().unwrap_or(&""),
                String::from_utf8_lossy(&output.stderr).trim()
            )));
        }
        Ok(String::from_utf8_lossy(&output.stdout).into_owned())
    }

    /// Commits everything that changed, if anything did.
    fn commit(&self, why: &str) -> Result<(), RepositoryError> {
        if self.git(&["status", "--porcelain"])?.trim().is_empty() {
            return Ok(());
        }
        self.git(&["add", "-A"])?;
        self.git(&["commit", "-q", "-m", why])?;
        Ok(())
    }

    /// The full path for a name relative to the root, refusing any symlink on the way:
    /// a link planted from outside must not lead a read or a write anywhere else.
    fn inside(&self, relative: &str) -> Result<PathBuf, RepositoryError> {
        let mut at = self.root.clone();
        for part in relative.split('/') {
            at.push(part);
            if let Ok(meta) = fs::symlink_metadata(&at) {
                if meta.file_type().is_symlink() {
                    return Err(RepositoryError::Corrupt(format!(
                        "{relative} goes through a link, which the brain does not follow"
                    )));
                }
            }
        }
        Ok(at)
    }

    /// A file's text, `None` if it is not there.
    fn text(&self, relative: &str) -> Result<Option<String>, RepositoryError> {
        let path = self.inside(relative)?;
        let Ok(meta) = fs::symlink_metadata(&path) else {
            return Ok(None);
        };
        if !meta.is_file() {
            return Ok(None);
        }
        if meta.len() > LARGEST_PAGE {
            return Err(RepositoryError::Corrupt(format!(
                "{relative} is larger than a page the brain reads"
            )));
        }
        fs::read_to_string(&path).map(Some).map_err(io)
    }

    /// Writes a file whole: to a temporary name first, then renamed over, so a reader
    /// never sees half a page.
    fn put(&self, relative: &str, text: &str) -> Result<(), RepositoryError> {
        let path = self.inside(relative)?;
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).map_err(io)?;
        }
        let temporary = path.with_extension("md.writing");
        fs::write(&temporary, text).map_err(io)?;
        fs::rename(&temporary, &path).map_err(io)
    }

    fn append_log(&self, when: &str, why: &str, path: &PagePath) -> Result<(), RepositoryError> {
        let log = self.inside("log.md")?;
        let mut file = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(log)
            .map_err(io)?;
        if when.is_empty() {
            return writeln!(file, "- {path} · {why}").map_err(io);
        }
        writeln!(file, "- {when} · {path} · {why}").map_err(io)
    }

    fn list(&self) -> Result<Vec<PageInfo>, RepositoryError> {
        let mut found = Vec::new();
        self.walk("", 0, &mut found)?;
        found.sort_by(|a, b| a.path.cmp(&b.path));
        Ok(found)
    }

    fn walk(
        &self,
        relative: &str,
        depth: usize,
        found: &mut Vec<PageInfo>,
    ) -> Result<(), RepositoryError> {
        if depth > 3 {
            return Ok(());
        }
        let dir = if relative.is_empty() {
            self.root.clone()
        } else {
            self.inside(relative)?
        };
        let Ok(entries) = fs::read_dir(&dir) else {
            return Ok(());
        };
        for entry in entries.flatten() {
            let name = entry.file_name().to_string_lossy().into_owned();
            if name.starts_with('.') {
                continue;
            }
            let child = if relative.is_empty() {
                name.clone()
            } else {
                format!("{relative}/{name}")
            };
            let Ok(kind) = entry.file_type() else {
                continue;
            };
            if kind.is_dir() {
                self.walk(&child, depth + 1, found)?;
                continue;
            }
            if !kind.is_file() {
                continue;
            }
            let Some(path) = PagePath::from_file(&child) else {
                continue;
            };
            // A page that cannot be read (too big) is left out rather than failing all.
            let Ok(Some(text)) = self.text(&child) else {
                continue;
            };
            found.push(PageInfo {
                title: brain::title_of(&text, &path),
                line: brain::one_line(&brain::summary_of(&text)),
                path,
            });
        }
        Ok(())
    }

    /// Writes the whole brain, history included, to one file `git clone` can restore
    /// from. Anything edited outside is committed first, so the copy has it.
    ///
    /// # Errors
    /// [`RepositoryError::Backend`] if the history cannot be read or the file written.
    pub fn bundle(&self, to: &Path) -> Result<(), RepositoryError> {
        let _held = self.guard()?;
        self.commit("Edited outside Endora")?;
        let to = to.to_string_lossy().into_owned();
        self.git(&["bundle", "create", "-q", &to, "--all"])?;
        Ok(())
    }

    fn render_index(&self) -> Result<String, RepositoryError> {
        let pages: Vec<(PagePath, String, String)> = self
            .list()?
            .into_iter()
            .map(|p| (p.path, p.title, p.line))
            .collect();
        Ok(brain::index(&pages))
    }

    fn guard(&self) -> Result<std::sync::MutexGuard<'_, ()>, RepositoryError> {
        self.lock
            .lock()
            .map_err(|_| RepositoryError::Backend("the brain's lock was poisoned".to_owned()))
    }
}

impl Brain for BrainFolder {
    fn read(&self, path: &PagePath) -> Result<Option<String>, RepositoryError> {
        self.text(&path.file())
    }

    fn pages(&self) -> Result<Vec<PageInfo>, RepositoryError> {
        self.list()
    }

    fn index(&self) -> Result<String, RepositoryError> {
        Ok(self.text("index.md")?.unwrap_or_default())
    }

    fn change(
        &self,
        path: &PagePath,
        when: &str,
        why: &str,
        edit: &mut dyn FnMut(Option<&str>) -> Option<String>,
    ) -> Result<bool, RepositoryError> {
        let _held = self.guard()?;
        // Whatever was edited outside goes into the history first, as theirs.
        self.commit("Edited outside Endora")?;
        let before = self.text(&path.file())?;
        let Some(after) = edit(before.as_deref()) else {
            return Ok(false);
        };
        if before.as_deref() == Some(after.as_str()) {
            return Ok(false);
        }
        self.put(&path.file(), &after)?;
        self.put("index.md", &self.render_index()?)?;
        self.append_log(when, why, path)?;
        self.commit(&format!("{path}: {why}"))?;
        Ok(true)
    }

    fn delete(&self, path: &PagePath, when: &str, why: &str) -> Result<bool, RepositoryError> {
        if path.is_memory() {
            return Err(RepositoryError::Corrupt(
                "memory is not removed, only emptied".to_owned(),
            ));
        }
        let _held = self.guard()?;
        self.commit("Edited outside Endora")?;
        let file = self.inside(&path.file())?;
        if !file.is_file() {
            return Ok(false);
        }
        fs::remove_file(&file).map_err(io)?;
        // Folders left empty go too, up to the root.
        let mut parent = file.parent().map(Path::to_path_buf);
        while let Some(dir) = parent {
            if dir == self.root || fs::remove_dir(&dir).is_err() {
                break;
            }
            parent = dir.parent().map(Path::to_path_buf);
        }
        self.put("index.md", &self.render_index()?)?;
        self.append_log(when, why, path)?;
        self.commit(&format!("{path}: {why}"))?;
        Ok(true)
    }

    fn history(&self, path: &PagePath) -> Result<Vec<PageChange>, RepositoryError> {
        let _held = self.guard()?;
        let file = path.file();
        let log = self.git(&["log", "-n", "50", "--format=%cI%x09%s", "--", &file])?;
        Ok(log
            .lines()
            .filter_map(|line| line.split_once('\t'))
            .map(|(at, subject)| PageChange {
                at: at.to_owned(),
                why: subject
                    .strip_prefix(&format!("{path}: "))
                    .unwrap_or(subject)
                    .to_owned(),
            })
            .collect())
    }

    fn forget_everything(&self) -> Result<(), RepositoryError> {
        let _held = self.guard()?;
        // Everything inside the root, the history included; not the root itself, which
        // may be a mount.
        for entry in fs::read_dir(&self.root).map_err(io)?.flatten() {
            let path = entry.path();
            let removed = match entry.file_type() {
                Ok(kind) if kind.is_dir() => fs::remove_dir_all(&path),
                _ => fs::remove_file(&path),
            };
            removed.map_err(io)?;
        }
        self.start()
    }
}

/// What the person told Endora, read from and written to memory's "What you told me".
/// The settings screen, the location and the form of address all go through here, so
/// there is one place for them and for every model (ADR 0101).
impl PreferenceRepository for BrainFolder {
    fn save(&self, preference: &Preference) -> Result<(), RepositoryError> {
        let line = preference
            .text()
            .split_whitespace()
            .collect::<Vec<_>>()
            .join(" ");
        let when = brain::date_of(preference.at().unix_millis());
        self.change(&PagePath::memory(), &when, "told by you", &mut |text| {
            let text = text.map_or_else(brain::new_memory, str::to_owned);
            let mut told = brain::section(&text, brain::TOLD);
            if told.iter().any(|said| brain::same_words(said, &line)) {
                return None;
            }
            told.push(line.clone());
            Some(brain::with_section(&text, brain::TOLD, &told))
        })?;
        Ok(())
    }

    fn list_all(&self) -> Result<Vec<Preference>, RepositoryError> {
        let text = self
            .text(&PagePath::memory().file())?
            .unwrap_or_else(brain::new_memory);
        brain::section(&text, brain::TOLD)
            .iter()
            .map(|line| {
                Preference::new(
                    told_id(line),
                    line,
                    told_kind(line),
                    Timestamp::from_unix_millis(0),
                )
                .map_err(|e| RepositoryError::Corrupt(e.to_string()))
            })
            .collect()
    }

    fn delete(&self, id: PreferenceId) -> Result<(), RepositoryError> {
        self.change(&PagePath::memory(), "", "removed by you", &mut |text| {
            let text = text?;
            let told = brain::section(text, brain::TOLD);
            let kept: Vec<String> = told
                .iter()
                .filter(|line| told_id(line) != id)
                .cloned()
                .collect();
            (kept.len() != told.len()).then(|| brain::with_section(text, brain::TOLD, &kept))
        })?;
        Ok(())
    }
}

/// A stable id for one line of what they told it: the same words, the same id, so the
/// settings screen can remove the line it shows.
fn told_id(line: &str) -> PreferenceId {
    // Two FNV-1a halves with different offsets: stable across builds, no dependency.
    let half = |offset: u64| {
        line.bytes().fold(offset, |hash, byte| {
            (hash ^ u64::from(byte)).wrapping_mul(0x0100_0000_01b3)
        })
    };
    let high = half(0xcbf2_9ce4_8422_2325);
    let low = half(0x8422_2325_cbf2_9ce4);
    PreferenceId::new((u128::from(high) << 64) | u128::from(low))
}

/// "Based in" and "Address me as" are context; anything else is a taste.
fn told_kind(line: &str) -> PreferenceKind {
    let lower = line.to_lowercase();
    if lower.starts_with("based in") || lower.starts_with("address me as") {
        return PreferenceKind::Context;
    }
    PreferenceKind::Taste
}

fn io(error: std::io::Error) -> RepositoryError {
    RepositoryError::Backend(error.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    /// A fresh folder per test, removed when the test ends.
    struct Scratch(PathBuf);
    impl Scratch {
        fn new() -> Self {
            static N: AtomicUsize = AtomicUsize::new(0);
            let path = std::env::temp_dir().join(format!(
                "endora-brain-{}-{}",
                std::process::id(),
                N.fetch_add(1, Ordering::SeqCst)
            ));
            let _ = fs::remove_dir_all(&path);
            Self(path)
        }
    }
    impl Drop for Scratch {
        fn drop(&mut self) {
            let _ = fs::remove_dir_all(&self.0);
        }
    }

    fn page(raw: &str) -> PagePath {
        PagePath::parse(raw).unwrap()
    }

    fn commits(folder: &BrainFolder) -> Vec<String> {
        folder
            .git(&["log", "--format=%s"])
            .unwrap()
            .lines()
            .map(str::to_owned)
            .collect()
    }

    fn note(folder: &BrainFolder, path: &PagePath, fact: &str) -> bool {
        folder
            .change(
                path,
                "2026-10-16",
                "noted from chat",
                &mut |text| match text {
                    Some(text) => brain::with_timeline_entry(text, "2026-10-16", fact),
                    None => Some(brain::new_page(&path.title_guess(), "2026-10-16", fact)),
                },
            )
            .unwrap()
    }

    #[test]
    fn a_new_brain_has_memory_an_index_a_log_and_a_history() {
        let dir = Scratch::new();
        let folder = BrainFolder::open(&dir.0).unwrap();
        assert!(
            folder
                .read(&PagePath::memory())
                .unwrap()
                .unwrap()
                .contains("## What you told me")
        );
        assert!(folder.index().unwrap().starts_with("# Index"));
        assert!(dir.0.join("log.md").is_file());
        assert_eq!(commits(&folder).len(), 1);
        // Opening it again changes nothing.
        let again = BrainFolder::open(&dir.0).unwrap();
        assert_eq!(commits(&again).len(), 1);
    }

    #[test]
    fn a_note_is_a_page_an_index_line_a_log_line_and_one_commit() {
        let dir = Scratch::new();
        let folder = BrainFolder::open(&dir.0).unwrap();
        let watches = page("hobbies/watches");
        assert!(note(&folder, &watches, "Collects dive watches"));
        assert!(note(&folder, &watches, "Wants a GMT next"));
        assert!(
            !note(&folder, &watches, "wants a GMT next."),
            "the same fact twice is one"
        );

        let text = folder.read(&watches).unwrap().unwrap();
        assert_eq!(brain::timeline_of(&text).len(), 2);
        assert!(
            folder
                .index()
                .unwrap()
                .contains("[[hobbies/watches]] Watches: Collects dive watches")
        );
        let log = fs::read_to_string(dir.0.join("log.md")).unwrap();
        assert!(log.contains("- 2026-10-16 · hobbies/watches · noted from chat"));
        assert_eq!(
            commits(&folder)[0],
            "hobbies/watches: noted from chat",
            "newest first"
        );
        assert_eq!(commits(&folder).len(), 3);
        assert_eq!(folder.pages().unwrap().len(), 2, "memory and watches");
    }

    #[test]
    fn an_edit_from_outside_is_committed_as_theirs_before_endora_writes() {
        let dir = Scratch::new();
        let folder = BrainFolder::open(&dir.0).unwrap();
        let watches = page("hobbies/watches");
        note(&folder, &watches, "Collects dive watches");
        // The person opens the file on the Mac and adds a line.
        let file = dir.0.join("hobbies/watches.md");
        let edited = fs::read_to_string(&file).unwrap() + "\nMy own note.\n";
        fs::write(&file, &edited).unwrap();

        note(&folder, &watches, "Wants a GMT next");
        let history = commits(&folder);
        assert_eq!(history[0], "hobbies/watches: noted from chat");
        assert_eq!(history[1], "Edited outside Endora");
        assert!(
            folder
                .read(&watches)
                .unwrap()
                .unwrap()
                .contains("My own note.")
        );
    }

    #[test]
    fn a_link_planted_in_the_folder_is_never_followed() {
        let dir = Scratch::new();
        let folder = BrainFolder::open(&dir.0).unwrap();
        let outside = Scratch::new();
        fs::create_dir_all(&outside.0).unwrap();
        fs::write(outside.0.join("secret.md"), "# Secret\n\nnot yours\n").unwrap();
        std::os::unix::fs::symlink(outside.0.join("secret.md"), dir.0.join("secret.md")).unwrap();
        std::os::unix::fs::symlink(&outside.0, dir.0.join("elsewhere")).unwrap();

        assert!(folder.read(&page("secret")).is_err());
        assert!(folder.read(&page("elsewhere/secret")).is_err());
        assert!(
            !folder
                .pages()
                .unwrap()
                .iter()
                .any(|p| p.path.as_str().contains("secret"))
        );
        let wrote = folder.change(&page("elsewhere/new"), "", "test", &mut |_| {
            Some("# New\n".to_owned())
        });
        assert!(wrote.is_err());
        assert!(
            !outside.0.join("new.md").exists(),
            "nothing was written outside"
        );
    }

    #[test]
    fn a_hook_planted_in_the_history_never_runs() {
        let dir = Scratch::new();
        let folder = BrainFolder::open(&dir.0).unwrap();
        let hook = dir.0.join(".git/hooks/pre-commit");
        let proof = dir.0.join(".git/hook-ran");
        fs::create_dir_all(hook.parent().unwrap()).unwrap();
        fs::write(&hook, format!("#!/bin/sh\ntouch '{}'\n", proof.display())).unwrap();
        use std::os::unix::fs::PermissionsExt as _;
        fs::set_permissions(&hook, fs::Permissions::from_mode(0o755)).unwrap();

        note(&folder, &page("routine"), "Up at six on weekdays");
        assert!(!proof.exists(), "a hook in the folder ran");
    }

    #[test]
    fn deleting_a_page_keeps_it_in_the_history_and_forgetting_does_not() {
        let dir = Scratch::new();
        let folder = BrainFolder::open(&dir.0).unwrap();
        let watches = page("hobbies/watches");
        note(&folder, &watches, "Collects dive watches");
        assert!(Brain::delete(&folder, &watches, "2026-10-16", "deleted by you").unwrap());
        assert!(folder.read(&watches).unwrap().is_none());
        assert!(!dir.0.join("hobbies").exists(), "the empty folder went too");
        assert!(!folder.index().unwrap().contains("watches"));
        let history = folder.history(&watches).unwrap();
        assert_eq!(history[0].why, "deleted by you");
        assert_eq!(history.len(), 2);
        assert!(Brain::delete(&folder, &PagePath::memory(), "", "x").is_err());

        folder.forget_everything().unwrap();
        assert!(
            folder.history(&watches).unwrap().is_empty(),
            "the history is gone too"
        );
        assert_eq!(commits(&folder).len(), 1);
        assert!(folder.read(&PagePath::memory()).unwrap().is_some());
    }

    #[test]
    fn what_they_told_it_is_memory_and_reads_back_as_before() {
        let dir = Scratch::new();
        let folder = BrainFolder::open(&dir.0).unwrap();
        let at = Timestamp::from_unix_millis(1_792_170_952_163);
        let based = Preference::new(
            PreferenceId::new(1),
            "Based in: New York",
            PreferenceKind::Context,
            at,
        )
        .unwrap();
        let terse = Preference::new(
            PreferenceId::new(2),
            "Short  answers please",
            PreferenceKind::Taste,
            at,
        )
        .unwrap();
        PreferenceRepository::save(&folder, &based).unwrap();
        PreferenceRepository::save(&folder, &terse).unwrap();
        PreferenceRepository::save(&folder, &terse).unwrap();

        let told = folder.list_all().unwrap();
        let lines: Vec<&str> = told.iter().map(Preference::text).collect();
        assert_eq!(lines, ["Based in: New York", "Short answers please"]);
        assert_eq!(told[0].kind(), PreferenceKind::Context);
        assert_eq!(told[1].kind(), PreferenceKind::Taste);
        let memory = folder.read(&PagePath::memory()).unwrap().unwrap();
        assert!(memory.contains("- Based in: New York\n"));

        PreferenceRepository::delete(&folder, told[1].id()).unwrap();
        let lines: Vec<String> = folder
            .list_all()
            .unwrap()
            .iter()
            .map(|p| p.text().to_owned())
            .collect();
        assert_eq!(lines, ["Based in: New York"]);
    }
}
