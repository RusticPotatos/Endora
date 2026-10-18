//! The vault and its passkey unlock (ADR 0127), end to end against a pretend OpenBao and a
//! pretend phone whose passkey signs and answers PRF for real. Everything after the browser is
//! exercised; the browser's own ceremony (Face ID, the secure enclave) cannot be.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use endora_capabilities::{CapabilitySettingsRepository as _, ConfigStore};
use endora_infrastructure::openbao::OpenBaoApi;
use endora_infrastructure::openbao::fake::FakeOpenBao;

use super::*;
use crate::webauthn::tests::Device;

const ORIGIN: &str = "https://endora.home.arpa:8787";
const T0: i64 = 1_000_000;

struct Scratch(std::path::PathBuf);
impl Scratch {
    fn new() -> Self {
        static NEXT: AtomicU32 = AtomicU32::new(0);
        let dir = std::env::temp_dir().join(format!(
            "endora-vault-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        Self(dir)
    }
    fn db(&self) -> String {
        self.0.join("endora.db").to_string_lossy().into_owned()
    }
    fn key(&self) -> std::path::PathBuf {
        self.0.join("secrets.key")
    }
}
impl Drop for Scratch {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

fn vault_over(store: Arc<SqliteStore>, bao: &Arc<FakeOpenBao>, scratch: &Scratch) -> Vault {
    let api: Arc<dyn OpenBaoApi> = bao.clone();
    let key = Arc::new(KeyFile::open(&scratch.key(), false).unwrap());
    Vault::new(Some(api), store, Some(key), DEFAULT_RP_ID)
}

fn rig() -> (Vault, Arc<FakeOpenBao>, Arc<SqliteStore>, Scratch) {
    let scratch = Scratch::new();
    let store = Arc::new(SqliteStore::open_sealed(&scratch.db(), &scratch.key()).unwrap());
    let bao = Arc::new(FakeOpenBao::new());
    (
        vault_over(store.clone(), &bao, &scratch),
        bao,
        store,
        scratch,
    )
}

fn field(value: &Value, path: &[&str]) -> Vec<u8> {
    let mut at = value;
    for key in path {
        at = &at[*key];
    }
    URL_SAFE_NO_PAD.decode(at.as_str().unwrap()).unwrap()
}

fn register(vault: &Vault, device: &Device, now_ms: i64) -> Value {
    let options = vault
        .begin_registration("Jane Doe's phone", now_ms)
        .unwrap();
    let challenge = field(&options, &["challenge"]);
    let (client_data, attestation) = device.create(&challenge, ORIGIN);
    vault
        .finish_registration(
            &Registration {
                challenge: b64(&challenge),
                id: device.id(),
                client_data: b64(&client_data),
                attestation_object: b64(&attestation),
                prf: None,
            },
            now_ms,
        )
        .unwrap()
}

/// What the console sends after `navigator.credentials.get` for `purpose`, from `origin`.
fn tap_at(
    vault: &Vault,
    device: &mut Device,
    purpose: &str,
    origin: &str,
    now_ms: i64,
) -> Assertion {
    let id = device.id();
    let options = vault.begin_assertion(purpose, Some(&id), now_ms).unwrap();
    let challenge = field(&options, &["challenge"]);
    let salt = field(
        &options,
        &["extensions", "prf", "evalByCredential", &id, "first"],
    );
    let (client_data, data, signature) = device.get(&challenge, origin);
    Assertion {
        challenge: b64(&challenge),
        id,
        client_data: b64(&client_data),
        authenticator_data: b64(&data),
        signature: b64(&signature),
        prf: Some(b64(&device.prf(&salt))),
    }
}

fn tap(vault: &Vault, device: &mut Device, purpose: &str, now_ms: i64) -> Assertion {
    tap_at(vault, device, purpose, ORIGIN, now_ms)
}

/// Every cell of the vault's two tables, as text, for looking for what must not be there.
fn every_cell(store: &SqliteStore) -> String {
    let db = store.db();
    let conn = db.lock().unwrap();
    let mut all = String::new();
    for table in ["vault_state", "vault_passkeys", "secret_keys", "audit_log"] {
        let Ok(mut stmt) = conn.prepare(&format!("SELECT * FROM {table}")) else {
            continue;
        };
        let columns = stmt.column_count();
        let rows = stmt
            .query_map([], |r| {
                let mut row = String::new();
                for i in 0..columns {
                    if let Ok(text) = r.get::<_, String>(i) {
                        row.push_str(&text);
                    }
                }
                Ok(row)
            })
            .unwrap();
        for row in rows.flatten() {
            all.push_str(&row);
        }
    }
    all
}

#[test]
fn initialising_with_a_passkey_shows_the_shares_once_and_keeps_none_in_the_clear() {
    let (vault, bao, store, _scratch) = rig();
    let mut phone = Device::new(b"phone");
    register(&vault, &phone, T0);
    assert_eq!(vault.status(T0).unwrap().vault, "uninitialised");

    let shares = vault
        .initialise(&tap(&vault, &mut phone, "initialise", T0), T0)
        .unwrap();
    assert_eq!(shares.len(), 3, "the break-glass copy");
    assert!(bao.root_revoked());
    let status = vault.status(T0).unwrap();
    assert_eq!(
        (status.vault, status.set_up, status.can_unlock),
        ("unlocked", true, 1)
    );
    let cells = every_cell(&store);
    assert!(
        !cells.contains("placeholder-share"),
        "no share in the clear"
    );
    assert!(
        !cells.contains("placeholder-secret-id"),
        "no login in the clear"
    );
    assert!(!cells.contains("placeholder-root"), "no root token");
    assert!(
        vault
            .initialise(&tap(&vault, &mut phone, "initialise", T0), T0)
            .is_err(),
        "once"
    );
}

#[test]
fn after_a_reboot_one_tap_unlocks_the_vault() {
    let (vault, bao, _store, _scratch) = rig();
    let mut phone = Device::new(b"phone");
    register(&vault, &phone, T0);
    vault
        .initialise(&tap(&vault, &mut phone, "initialise", T0), T0)
        .unwrap();
    bao.seal();
    assert_eq!(vault.status(T0).unwrap().vault, "locked");

    vault
        .unlock(&tap(&vault, &mut phone, "unlock", T0 + 1), T0 + 1)
        .unwrap();
    assert!(!bao.is_sealed());
    assert_eq!(vault.status(T0).unwrap().vault, "unlocked");
}

#[test]
fn a_challenge_answers_one_ceremony_and_expires() {
    let (vault, bao, _store, _scratch) = rig();
    let mut phone = Device::new(b"phone");
    register(&vault, &phone, T0);
    vault
        .initialise(&tap(&vault, &mut phone, "initialise", T0), T0)
        .unwrap();
    bao.seal();
    let once = tap(&vault, &mut phone, "unlock", T0);
    vault.unlock(&once, T0).unwrap();
    bao.seal();
    let error = vault.unlock(&once, T0).unwrap_err().to_string();
    assert!(error.contains("expired"), "replayed: {error}");
    let late = tap(&vault, &mut phone, "unlock", T0);
    assert!(
        vault.unlock(&late, T0 + 3 * 60 * 1_000).is_err(),
        "too late"
    );
    let wrong_purpose = tap(&vault, &mut phone, "enrol", T0);
    assert!(vault.unlock(&wrong_purpose, T0).is_err());
}

#[test]
fn opened_by_its_address_the_passkey_is_refused_and_told_where_to_go() {
    let (vault, _bao, _store, _scratch) = rig();
    let mut phone = Device::new(b"phone");
    register(&vault, &phone, T0);
    let by_ip = tap_at(
        &vault,
        &mut phone,
        "initialise",
        "https://192.168.1.10:8787",
        T0,
    );
    let error = vault.initialise(&by_ip, T0).unwrap_err().to_string();
    assert!(error.contains("https://endora.home.arpa"), "{error}");
}

#[test]
fn a_wrong_prf_output_opens_nothing_and_repeated_failures_are_throttled() {
    let (vault, bao, _store, _scratch) = rig();
    let mut phone = Device::new(b"phone");
    register(&vault, &phone, T0);
    vault
        .initialise(&tap(&vault, &mut phone, "initialise", T0), T0)
        .unwrap();
    bao.seal();
    let mut last = String::new();
    for i in 0..8 {
        let mut a = tap(&vault, &mut phone, "unlock", T0 + i);
        a.prf = Some(b64(&[7u8; 32]));
        last = vault.unlock(&a, T0 + i).unwrap_err().to_string();
    }
    assert!(last.contains("too many attempts"), "{last}");
    assert!(bao.is_sealed());
    let good = tap(&vault, &mut phone, "unlock", T0 + 10);
    assert!(
        vault.unlock(&good, T0 + 10).is_err(),
        "still throttled, even for the right answer"
    );
    let good = tap(&vault, &mut phone, "unlock", T0 + 2 * 60 * 60 * 1_000);
    vault.unlock(&good, T0 + 2 * 60 * 60 * 1_000).unwrap();
}

#[test]
fn a_passkey_without_prf_is_told_what_it_needs() {
    let (vault, _bao, _store, _scratch) = rig();
    let mut phone = Device::new(b"phone");
    register(&vault, &phone, T0);
    let mut a = tap(&vault, &mut phone, "initialise", T0);
    a.prf = None;
    let error = vault.initialise(&a, T0).unwrap_err().to_string();
    assert!(error.contains("PRF"), "{error}");
}

#[test]
fn a_second_passkey_gets_its_own_copy_and_then_unlocks_alone() {
    let (vault, bao, _store, _scratch) = rig();
    let mut phone = Device::new(b"phone");
    let mut key = Device::new(b"security-key");
    register(&vault, &phone, T0);
    vault
        .initialise(&tap(&vault, &mut phone, "initialise", T0), T0)
        .unwrap();
    // Past initialisation's own ticket: the second passkey needs the first one's tap.
    let later = T0 + 10 * 60 * 1_000;
    let added = register(&vault, &key, later);
    assert_eq!(added["can_unlock"], false);
    assert!(
        vault
            .adopt(&tap(&vault, &mut key, "adopt", later), later)
            .is_err(),
        "no ticket"
    );
    vault
        .enrol(&tap(&vault, &mut phone, "enrol", later), later)
        .unwrap();
    assert!(vault.status(later).unwrap().adopting);
    vault
        .adopt(&tap(&vault, &mut key, "adopt", later), later)
        .unwrap();
    assert!(!vault.status(later).unwrap().adopting, "one use");
    assert_eq!(vault.status(later).unwrap().can_unlock, 2);

    bao.seal();
    vault
        .unlock(&tap(&vault, &mut key, "unlock", later), later)
        .unwrap();
    assert!(!bao.is_sealed());
}

#[test]
fn the_last_passkey_that_can_unlock_is_not_removed_and_there_are_at_most_five() {
    let (vault, _bao, _store, _scratch) = rig();
    let mut phone = Device::new(b"phone");
    register(&vault, &phone, T0);
    vault
        .initialise(&tap(&vault, &mut phone, "initialise", T0), T0)
        .unwrap();
    let error = vault.remove_passkey(&phone.id()).unwrap_err().to_string();
    assert!(error.contains("only passkey"), "{error}");
    for i in 0..4u8 {
        register(&vault, &Device::new(&[b'k', i]), T0);
    }
    assert!(vault.begin_registration("one more", T0).is_err());
    assert!(vault.remove_passkey(&Device::new(&[b'k', 0]).id()).unwrap());
}

#[test]
fn the_nodes_keys_move_into_the_vault_and_a_sealed_restart_waits_for_the_unlock() {
    let (vault, bao, store, scratch) = rig();
    ConfigStore::new(store.db())
        .set_setting("home-assistant", "token", "placeholder-token-0042")
        .unwrap();
    let mut phone = Device::new(b"phone");
    register(&vault, &phone, T0);
    vault
        .initialise(&tap(&vault, &mut phone, "initialise", T0), T0)
        .unwrap();
    vault.keys_into_vault().unwrap();
    assert!(vault.status(T0).unwrap().keys_in_vault);
    assert!(vault.keys_into_vault().is_err(), "once");
    assert!(
        vault.start_over().is_err(),
        "its state can no longer be forgotten"
    );
    drop(vault);
    drop(store);

    // The NAS reboots: the vault is sealed, the node starts waiting rather than failing.
    bao.seal();
    let api: Arc<dyn OpenBaoApi> = bao.clone();
    let store = Arc::new(open_node_store(&scratch.db(), &scratch.key(), Some(&api)).unwrap());
    assert!(!store.db().has_key_source());
    let error = ConfigStore::new(store.db())
        .all_settings()
        .unwrap_err()
        .to_string();
    assert!(error.contains("vault is locked"), "{error}");
    assert!(
        SqliteStore::open_sealed(&scratch.db(), &scratch.key()).is_err(),
        "the key file alone opens nothing now"
    );

    let vault = vault_over(store.clone(), &bao, &scratch);
    assert!(vault.status(T0).unwrap().node_waiting);
    let opened = vault
        .unlock(&tap(&vault, &mut phone, "unlock", T0 + 5), T0 + 5)
        .unwrap();
    assert!(opened, "the unlock opened the node's keys too");
    let settings = ConfigStore::new(store.db()).all_settings().unwrap();
    assert!(
        settings
            .iter()
            .any(|(_, _, v)| v == "placeholder-token-0042")
    );
    drop(vault);
    drop(store);

    // A restart while the vault is open opens straight away.
    let store = open_node_store(&scratch.db(), &scratch.key(), Some(&api)).unwrap();
    assert!(store.db().has_key_source());
}

#[test]
fn starting_over_forgets_the_shares_and_every_copy_of_the_key() {
    let (vault, _bao, _store, _scratch) = rig();
    let mut phone = Device::new(b"phone");
    register(&vault, &phone, T0);
    vault
        .initialise(&tap(&vault, &mut phone, "initialise", T0), T0)
        .unwrap();
    vault.start_over().unwrap();
    let status = vault.status(T0).unwrap();
    assert!(!status.set_up);
    assert_eq!((status.passkeys, status.can_unlock), (1, 0));
}

#[test]
fn the_unlock_key_is_bound_to_the_passkey_its_salt_and_its_output() {
    let salt = [1u8; 32];
    let a = unlock_key(&[2u8; 32], &salt, "one");
    assert_ne!(*a, *unlock_key(&[2u8; 32], &salt, "two"));
    assert_ne!(*a, *unlock_key(&[3u8; 32], &salt, "one"));
    assert_ne!(*a, *unlock_key(&[2u8; 32], &[9u8; 32], "one"));
    assert_eq!(*a, *unlock_key(&[2u8; 32], &salt, "one"));
}

/// The whole flow against a real, fresh OpenBao (the pinned image): register, initialise,
/// move the node's keys in, restart sealed and wait, unlock with the passkey. Ignored: it needs
/// a vault nobody has initialised, at `ENDORA_OPENBAO_TEST_URL` (for example one started with
/// `docker run` as the infrastructure crate's ignored test does), and it initialises it.
#[test]
#[ignore = "needs a fresh OpenBao at ENDORA_OPENBAO_TEST_URL"]
fn against_a_real_openbao() {
    let url = std::env::var("ENDORA_OPENBAO_TEST_URL").expect("ENDORA_OPENBAO_TEST_URL");
    let api: Arc<dyn OpenBaoApi> = Arc::new(endora_infrastructure::openbao::HttpOpenBao::new(&url));
    let scratch = Scratch::new();
    let store = Arc::new(SqliteStore::open_sealed(&scratch.db(), &scratch.key()).unwrap());
    ConfigStore::new(store.db())
        .set_setting("home-assistant", "token", "placeholder-token-0077")
        .unwrap();
    let key = Arc::new(KeyFile::open(&scratch.key(), false).unwrap());
    let vault = Vault::new(
        Some(api.clone()),
        store.clone(),
        Some(key.clone()),
        DEFAULT_RP_ID,
    );
    let mut phone = Device::new(b"phone");
    register(&vault, &phone, T0);
    let shares = vault
        .initialise(&tap(&vault, &mut phone, "initialise", T0), T0)
        .unwrap();
    assert_eq!(shares.len(), 3);
    vault.keys_into_vault().unwrap();
    drop(vault);
    drop(store);

    // A reboot: the container named by ENDORA_OPENBAO_TEST_CONTAINER is restarted, which
    // seals it, and the node starts waiting for the unlock.
    let container =
        std::env::var("ENDORA_OPENBAO_TEST_CONTAINER").expect("ENDORA_OPENBAO_TEST_CONTAINER");
    let restarted = std::process::Command::new("docker")
        .args(["restart", &container])
        .status()
        .unwrap();
    assert!(restarted.success());
    for _ in 0..100 {
        if api.seal_status().is_ok() {
            break;
        }
        std::thread::sleep(std::time::Duration::from_millis(100));
    }
    assert!(api.seal_status().unwrap().sealed, "a restart seals it");
    let store = Arc::new(open_node_store(&scratch.db(), &scratch.key(), Some(&api)).unwrap());
    assert!(!store.db().has_key_source(), "the node waits");
    let vault = Vault::new(Some(api), store.clone(), Some(key), DEFAULT_RP_ID);
    assert!(
        vault
            .unlock(&tap(&vault, &mut phone, "unlock", T0 + 1), T0 + 1)
            .unwrap()
    );
    let settings = ConfigStore::new(store.db()).all_settings().unwrap();
    assert!(
        settings
            .iter()
            .any(|(_, _, v)| v == "placeholder-token-0077")
    );
}
