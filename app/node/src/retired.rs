//! What the chat left on disk, forgotten when the node starts (ADR 0136).
//!
//! The owner chose to delete the brain's pages once they had taken a copy. The database sheds
//! the chat's tables in `endora-infrastructure`; this removes the brain's folder and the nightly
//! copies of it. Both are told by their own marks, so a misconfigured path forgets nothing else.

use std::path::Path;

/// Empties the brain's folder (its pages and their history), when `dir` is one: it holds
/// `memory.md` and a `.git` the brain kept its history in. The folder itself stays, since it may
/// be a mount. Returns how many entries went; nothing is touched in a folder without both marks.
///
/// # Errors
/// An entry that could not be removed.
pub fn forget_the_brain(dir: &Path) -> std::io::Result<usize> {
    if !dir.join("memory.md").is_file() || !dir.join(".git").is_dir() {
        return Ok(0);
    }
    let mut gone = 0;
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            std::fs::remove_dir_all(&path)?;
        } else {
            std::fs::remove_file(&path)?;
        }
        gone += 1;
    }
    Ok(gone)
}

/// Removes the nightly copies of the brain (`brain-daily-*.bundle`, `brain-weekly-*.bundle`)
/// from `backups`, which the node made and nothing reads any more. Nothing else is touched.
/// Returns how many went.
///
/// # Errors
/// A copy that could not be removed.
pub fn forget_the_brains_copies(backups: &Path) -> std::io::Result<usize> {
    let Ok(entries) = std::fs::read_dir(backups) else {
        return Ok(0);
    };
    let mut gone = 0;
    for entry in entries {
        let path = entry?.path();
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or_default();
        let ours = (name.starts_with("brain-daily-") || name.starts_with("brain-weekly-"))
            && name.ends_with(".bundle");
        if ours && path.is_file() {
            std::fs::remove_file(&path)?;
            gone += 1;
        }
    }
    Ok(gone)
}

#[cfg(test)]
mod tests {
    use super::{forget_the_brain, forget_the_brains_copies};

    fn scratch(name: &str) -> std::path::PathBuf {
        let dir =
            std::env::temp_dir().join(format!("endora-retired-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn the_brains_folder_is_emptied_and_stays() {
        let dir = scratch("brain");
        std::fs::write(dir.join("memory.md"), "# Memory\n\n- John Doe likes tea\n").unwrap();
        std::fs::create_dir_all(dir.join(".git/objects")).unwrap();
        std::fs::create_dir_all(dir.join("hobbies")).unwrap();
        std::fs::write(dir.join("hobbies/watches.md"), "# Watches\n").unwrap();
        assert_eq!(forget_the_brain(&dir).unwrap(), 3);
        assert!(dir.is_dir(), "the folder (perhaps a mount) stays");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 0);
        assert_eq!(
            forget_the_brain(&dir).unwrap(),
            0,
            "a second start finds nothing"
        );
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn a_folder_that_is_not_the_brain_is_left_alone() {
        let dir = scratch("not-brain");
        std::fs::write(dir.join("memory.md"), "someone else's notes").unwrap();
        std::fs::write(dir.join("endora.db"), "data").unwrap();
        assert_eq!(forget_the_brain(&dir).unwrap(), 0, "no .git: not the brain");
        assert_eq!(std::fs::read_dir(&dir).unwrap().count(), 2);
        std::fs::remove_dir_all(dir).unwrap();
    }

    #[test]
    fn only_the_brains_own_copies_leave_backups() {
        let dir = scratch("backups");
        for name in [
            "brain-daily-2026-10-20.bundle",
            "brain-weekly-2026-10-18.bundle",
            "daily-2026-10-20.db",
            "pre-secrets-at-rest-20261018.db",
            "brain-notes.txt",
        ] {
            std::fs::write(dir.join(name), "x").unwrap();
        }
        assert_eq!(forget_the_brains_copies(&dir).unwrap(), 2);
        let mut left: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        left.sort();
        assert_eq!(
            left,
            [
                "brain-notes.txt",
                "daily-2026-10-20.db",
                "pre-secrets-at-rest-20261018.db"
            ]
        );
        assert_eq!(forget_the_brains_copies(&dir.join("missing")).unwrap(), 0);
        std::fs::remove_dir_all(dir).unwrap();
    }
}
