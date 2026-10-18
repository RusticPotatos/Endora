//! The node's secrets key (ADR 0125): where it lives, opening the store under it, and the
//! two commands that look after it.
//!
//! - `endora-node secrets rekey` — seals every stored credential again under a new data key
//!   and forgets the old one. The key file does not change.
//! - `endora-node secrets forget --i-have-lost-the-key` — the way out when the key file is
//!   gone for good and no copy survives: empties every sealed credential and the password so
//!   the node can start again with a new key. Everything has to be entered again.
//!
//! Both are handled before the node starts, like `backup`, and print ids and counts — never
//! a secret.

use std::error::Error;
use std::path::{Path, PathBuf};

use endora_infrastructure::SqliteStore;

/// The file name of the key beside the database, when `ENDORA_SECRETS_KEY_FILE` is unset.
const BESIDE_THE_DATABASE: &str = "secrets.key";

/// Where the secrets key is: `configured` (`ENDORA_SECRETS_KEY_FILE`) if set, else
/// `secrets.key` in the database's folder.
#[must_use]
pub fn key_file(database: &str, configured: Option<&str>) -> PathBuf {
    if let Some(file) = configured.map(str::trim).filter(|f| !f.is_empty()) {
        return PathBuf::from(file);
    }
    Path::new(database)
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."))
        .join(BESIDE_THE_DATABASE)
}

/// The key file this process should use, from its environment.
fn key_file_from_env(database: &str) -> PathBuf {
    key_file(
        database,
        std::env::var("ENDORA_SECRETS_KEY_FILE").ok().as_deref(),
    )
}

/// Opens the node's store with its secrets sealed under the key file — made on the first
/// start, required on every later one.
///
/// # Errors
/// If the key file is missing over sealed secrets, does not open them, or the database
/// cannot be opened or migrated.
pub fn open_store(database: &str) -> Result<SqliteStore, Box<dyn Error>> {
    Ok(SqliteStore::open_sealed(
        database,
        &key_file_from_env(database),
    )?)
}

/// Handles `secrets …` if that is what `args` say, returning `None` otherwise.
pub fn dispatch(args: &[String], database: &str) -> Option<Result<(), Box<dyn Error>>> {
    if args.first().map(String::as_str) != Some("secrets") {
        return None;
    }
    let args: Vec<&str> = args.iter().map(String::as_str).collect();
    Some(match args.as_slice() {
        [_, "rekey"] => rekey(database),
        [_, "forget", "--i-have-lost-the-key"] => forget(database),
        _ => Err("usage: endora-node secrets rekey | secrets forget --i-have-lost-the-key".into()),
    })
}

fn rekey(database: &str) -> Result<(), Box<dyn Error>> {
    let kid = open_store(database)?.rekey_secrets()?;
    println!("Every stored credential is now sealed under data key {kid}; the old one is gone.");
    Ok(())
}

fn forget(database: &str) -> Result<(), Box<dyn Error>> {
    let db = endora_persistence::Db::open(database)?;
    let emptied = endora_infrastructure::forget_sealed_secrets(&db)?;
    println!(
        "Emptied {emptied} sealed credential(s) and the sign-in password. Start the node: it \
         makes a new key, prints its bootstrap token, and every provider key, token and \
         password has to be entered again."
    );
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::{dispatch, key_file};
    use std::path::PathBuf;

    #[test]
    fn the_key_sits_beside_the_database_unless_placed_elsewhere() {
        assert_eq!(
            key_file("/data/endora.db", None),
            PathBuf::from("/data/secrets.key")
        );
        assert_eq!(key_file("endora.db", None), PathBuf::from("./secrets.key"));
        assert_eq!(
            key_file("/data/endora.db", Some("/run/keys/endora")),
            PathBuf::from("/run/keys/endora")
        );
        assert_eq!(
            key_file("/data/endora.db", Some("  ")),
            PathBuf::from("/data/secrets.key"),
            "set-but-empty is unset, as compose leaves a blanked variable"
        );
    }

    #[test]
    fn only_the_secrets_command_is_taken_and_forgetting_needs_saying_so() {
        let args = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(dispatch(&args(&["backup", "x"]), "unused.db").is_none());
        assert!(dispatch(&args(&[]), "unused.db").is_none());
        for wrong in [
            &["secrets"][..],
            &["secrets", "forget"],
            &["secrets", "forget", "--yes"],
            &["secrets", "show"],
        ] {
            let outcome = dispatch(&args(wrong), "unused.db").expect("taken, not started");
            assert!(outcome.is_err(), "{wrong:?}");
        }
    }

    #[test]
    fn a_rekey_from_the_command_keeps_every_credential_and_the_key_file() {
        use endora_capabilities::{CapabilitySettingsRepository, ConfigStore};
        let dir = std::env::temp_dir().join(format!("endora-node-rekey-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let database = dir.join("endora.db").to_string_lossy().into_owned();
        let key = key_file(&database, None);
        {
            let store = endora_infrastructure::SqliteStore::open_sealed(&database, &key).unwrap();
            ConfigStore::new(store.db())
                .set_setting("home-assistant", "token", "placeholder-token-0003")
                .unwrap();
        }
        let key_before = std::fs::read(&key).unwrap();

        let store = endora_infrastructure::SqliteStore::open_sealed(&database, &key).unwrap();
        store.rekey_secrets().unwrap();
        drop(store);

        assert_eq!(
            std::fs::read(&key).unwrap(),
            key_before,
            "the key file stays"
        );
        let store = endora_infrastructure::SqliteStore::open_sealed(&database, &key).unwrap();
        let settings = ConfigStore::new(store.db()).all_settings().unwrap();
        assert!(settings.contains(&(
            "home-assistant".to_owned(),
            "token".to_owned(),
            "placeholder-token-0003".to_owned()
        )));
        drop(store);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
