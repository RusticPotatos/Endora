//! The owner's scenarios for reaching the house's vault (`app/node/features/vault/`), ADR 0127
//! amended 2026-11-07: the vault is its own service, and the node a client of it at the
//! address it is given. Steps read the address as the node does at start, and ask the vault's
//! status as the console does, against a pretend OpenBao.

use std::sync::Arc;
use std::sync::atomic::{AtomicU32, Ordering};

use cucumber::{World, given, then, when};
use endora_infrastructure::SqliteStore;
use endora_infrastructure::openbao::OpenBaoApi as _;
use endora_infrastructure::openbao::fake::FakeOpenBao;

use super::{DEFAULT_RP_ID, Status, Vault};

#[derive(World, Default)]
struct VaultWorld {
    raw: String,
    read: Option<Result<Option<String>, String>>,
    bao: Option<Arc<FakeOpenBao>>,
    status: Option<Status>,
}

impl std::fmt::Debug for VaultWorld {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("VaultWorld")
            .field("read", &self.read)
            .field("status", &self.status)
            .finish_non_exhaustive()
    }
}

impl VaultWorld {
    fn bao(&mut self) -> Arc<FakeOpenBao> {
        let raw = self.raw.clone();
        self.bao
            .get_or_insert_with(|| {
                let bao = FakeOpenBao::new();
                bao.set_address(&raw);
                Arc::new(bao)
            })
            .clone()
    }
}

#[given(expr = "the vault's address is {string}")]
fn address_is(w: &mut VaultWorld, raw: String) {
    w.raw = raw;
}

#[given(expr = "nothing answers there")]
fn nothing_answers(w: &mut VaultWorld) {
    w.bao().set_unreachable(true);
}

#[given(expr = "the vault there was initialised by the owner and is sealed")]
fn initialised_by_the_owner(w: &mut VaultWorld) {
    let bao = w.bao();
    bao.init(3, 2).expect("the pretend vault initialises");
    bao.seal();
}

#[when(expr = "the node reads its vault address")]
fn reads_address(w: &mut VaultWorld) {
    w.read = Some(crate::secrets::vault_address(Some(&w.raw)));
}

#[when(expr = "the console asks how the vault is")]
fn console_asks(w: &mut VaultWorld) {
    static NEXT: AtomicU32 = AtomicU32::new(0);
    let dir = std::env::temp_dir().join(format!(
        "endora-vault-scenario-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::Relaxed)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).expect("scratch folder");
    let db = dir.join("endora.db").to_string_lossy().into_owned();
    let store = Arc::new(SqliteStore::open_sealed(&db, &dir.join("secrets.key")).expect("a store"));
    let api: Arc<dyn endora_infrastructure::openbao::OpenBaoApi> = w.bao();
    let vault = Vault::new(Some(api), store, None, DEFAULT_RP_ID);
    w.status = Some(vault.status(0).expect("a status"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[then(expr = "the node reaches the vault at {string}")]
fn reaches_at(w: &mut VaultWorld, url: String) {
    assert_eq!(w.read, Some(Ok(Some(url))));
}

#[then(expr = "the node has no vault")]
fn no_vault(w: &mut VaultWorld) {
    assert_eq!(w.read, Some(Ok(None)));
}

#[then(expr = "the node refuses it, saying {string}")]
fn refuses(w: &mut VaultWorld, words: String) {
    match &w.read {
        Some(Err(why)) => assert!(why.contains(&words), "refused with: {why}"),
        other => panic!("not refused: {other:?}"),
    }
}

#[then(expr = "the refusal does not contain {string}")]
fn refusal_hides(w: &mut VaultWorld, secret: String) {
    let Some(Err(why)) = &w.read else {
        panic!("not refused")
    };
    assert!(!why.contains(&secret), "the refusal repeats the credential");
}

#[then(expr = "it is told the vault is {string} at {string}")]
fn told(w: &mut VaultWorld, word: String, url: String) {
    let s = w.status.as_ref().expect("asked");
    assert_eq!(s.vault, word);
    assert_eq!(s.address.as_deref(), Some(url.as_str()));
}

#[then(expr = "it is told Endora did not set it up")]
fn not_set_up(w: &mut VaultWorld) {
    assert!(!w.status.as_ref().expect("asked").set_up);
}

/// Runs every scenario; a failed, skipped or unparsed step fails this test.
#[test]
fn the_owners_scenarios_for_reaching_the_vault_pass() {
    use cucumber::cli::Parser as _;
    use cucumber::writer::Stats as _;
    let none = cucumber::cli::Opts::<
        cucumber::parser::basic::Cli,
        cucumber::runner::basic::Cli,
        cucumber::writer::basic::Cli,
    >::try_parse_from(["cucumber"])
    .expect("cucumber's own options");
    let writer = tokio::runtime::Builder::new_current_thread()
        .build()
        .expect("a runtime")
        .block_on(
            VaultWorld::cucumber()
                .with_cli(none)
                .fail_on_skipped()
                .run("features/vault"),
        );
    assert!(
        !writer.execution_has_failed() && writer.parsing_errors() == 0,
        "a scenario failed: see the steps above"
    );
    assert!(writer.passed_steps() > 0, "no scenario ran");
}
