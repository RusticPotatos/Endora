//! OpenBao (ADR 0127). Every share, token and key here is a placeholder; no server runs
//! except in the one ignored test at the end, which starts the real image when asked to.

use std::io::{Read as _, Write as _};
use std::sync::Arc;

use endora_persistence::Db;
use endora_persistence::secrets::{DataKey, KeySource};
use zeroize::Zeroizing;

use super::fake::FakeOpenBao;
use super::*;

/// A vault initialised, unsealed and set up the way the node does it; its AppRole login.
fn set_up(bao: &FakeOpenBao) -> AppRole {
    let made = initialise(bao).unwrap();
    unseal_with(bao, &made.shares).unwrap();
    set_up_endora(bao, &made.root_token).unwrap()
}

#[test]
fn initialising_makes_three_shares_of_which_two_open_it() {
    let bao = FakeOpenBao::new();
    let made = initialise(&bao).unwrap();
    assert_eq!(made.shares.len(), 3);
    assert!(bao.is_sealed());
    let status = unseal_with(&bao, &made.shares[1..2]);
    assert!(status.is_err(), "one share is not enough");
    let status = unseal_with(&bao, &made.shares[1..]).unwrap();
    assert!(!status.sealed);
    assert!(
        format!("{made:?}").find("placeholder").is_none(),
        "never printed"
    );
}

#[test]
fn a_vault_is_initialised_once() {
    let bao = FakeOpenBao::new();
    initialise(&bao).unwrap();
    let again = initialise(&bao).unwrap_err().to_string();
    assert!(again.contains("already initialised"), "{again}");
}

#[test]
fn setup_makes_kv_transit_a_derived_key_and_a_narrow_approle_then_revokes_the_root_token() {
    let bao = FakeOpenBao::new();
    let role = set_up(&bao);
    assert_eq!(bao.mounts(), vec!["approle", "secret", "transit"]);
    assert_eq!(bao.transit_key_derived(), Some(true));
    let policy = bao.policy().unwrap();
    assert!(policy.contains("transit/encrypt/endora-node"));
    assert!(!policy.contains("sys/"), "no sys path: {policy}");
    assert!(!policy.contains("rotate"), "{policy}");
    assert!(bao.root_revoked(), "the root token is gone");
    assert_eq!(
        bao.calls().last().map(String::as_str),
        Some("auth/token/revoke-self")
    );
    assert!(!role.role_id.is_empty() && !role.secret_id.is_empty());
    assert_eq!(format!("{role:?}"), "AppRole(..)");
}

#[test]
fn a_setup_that_fails_half_way_still_revokes_the_root_token() {
    let bao = FakeOpenBao::new();
    let made = initialise(&bao).unwrap();
    unseal_with(&bao, &made.shares).unwrap();
    bao.fail_at("sys/policies/acl/endora-node");
    let error = set_up_endora(&bao, &made.root_token)
        .unwrap_err()
        .to_string();
    assert!(error.contains("writing the node's policy"), "{error}");
    assert!(bao.root_revoked());
}

#[test]
fn the_node_token_reaches_only_its_transit_key_and_its_kv_corner() {
    let bao = Arc::new(FakeOpenBao::new());
    let role = set_up(&bao);
    let login = bao
        .call(
            "",
            Verb::Post,
            "auth/approle/login",
            Some(&json!({ "role_id": &*role.role_id, "secret_id": &*role.secret_id })),
        )
        .unwrap();
    let token = login["auth"]["client_token"].as_str().unwrap();
    let refused = bao.call(token, Verb::Post, "sys/mounts/other", Some(&json!({})));
    assert!(matches!(
        refused,
        Err(VaultError::Answered { status: 403, .. })
    ));
}

#[test]
fn an_approle_login_survives_being_written_down_and_read_back() {
    let role = AppRole {
        role_id: Zeroizing::new("placeholder-role".to_owned()),
        secret_id: Zeroizing::new("placeholder-secret".to_owned()),
    };
    let back = AppRole::from_text(&role.to_text()).unwrap();
    assert_eq!(*back.role_id, *role.role_id);
    assert_eq!(*back.secret_id, *role.secret_id);
    assert!(AppRole::from_text("no-newline").is_none());
    assert!(AppRole::from_text("\nsecret").is_none());
}

#[test]
fn transit_wraps_a_data_key_it_alone_can_unwrap_and_only_for_its_own_id() {
    let bao = Arc::new(FakeOpenBao::new());
    let keys = TransitKeys::new(bao.clone(), set_up(&bao));
    let key = DataKey::random();
    let wrapped = keys.wrap("0011223344556677", &key).unwrap();
    assert!(wrapped.starts_with("openbao:v1:vault:"), "{wrapped}");
    let back = keys.unwrap("0011223344556677", &wrapped).unwrap();
    assert_eq!(back.as_bytes(), key.as_bytes());
    assert!(
        keys.unwrap("8899aabbccddeeff", &wrapped).is_err(),
        "the key id is the derived key's context"
    );
    let other = keys
        .unwrap("0011223344556677", "file:v1:x:y")
        .unwrap_err()
        .to_string();
    assert!(other.contains("'file'"), "{other}");
}

#[test]
fn a_sealed_vault_unwraps_nothing_and_says_it_is_locked() {
    let bao = Arc::new(FakeOpenBao::new());
    let keys = TransitKeys::new(bao.clone(), set_up(&bao));
    let wrapped = keys.wrap("0011223344556677", &DataKey::random()).unwrap();
    bao.seal();
    let error = keys
        .unwrap("0011223344556677", &wrapped)
        .unwrap_err()
        .to_string();
    assert!(error.contains("the vault is locked"), "{error}");
    assert!(keys.check().is_err());
}

#[test]
fn a_wrong_login_is_found_by_checking_not_by_the_first_secret() {
    let bao = Arc::new(FakeOpenBao::new());
    set_up(&bao);
    let wrong = AppRole {
        role_id: Zeroizing::new("placeholder-role".to_owned()),
        secret_id: Zeroizing::new("placeholder-wrong".to_owned()),
    };
    assert!(TransitKeys::new(bao, wrong).check().is_err());
}

#[test]
fn the_nodes_data_keys_move_into_transit_and_open_from_it_after_a_restart() {
    let bao = Arc::new(FakeOpenBao::new());
    let role = set_up(&bao);
    let dir = std::env::temp_dir().join(format!("endora-openbao-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("node.db").to_string_lossy().into_owned();
    let file = dir.join("secrets.key");
    let db = Db::open(&path).unwrap();
    db.attach_key_source(Arc::new(
        endora_persistence::secrets::KeyFile::open(&file, true).unwrap(),
    ))
    .unwrap();
    let sealed = db.seal("placeholder-provider-key-0001").unwrap();
    db.rewrap(Arc::new(TransitKeys::new(bao.clone(), role.clone())))
        .unwrap();
    drop(db);

    let db = Db::open(&path).unwrap();
    assert_eq!(
        db.key_sources_on_disk().unwrap(),
        vec!["openbao".to_owned()]
    );
    assert!(
        db.attach_key_source(Arc::new(
            endora_persistence::secrets::KeyFile::open(&file, false).unwrap()
        ))
        .is_err(),
        "the key file no longer opens them"
    );
    db.attach_key_source(Arc::new(TransitKeys::new(bao, role)))
        .unwrap();
    assert_eq!(
        db.unseal("t.c", &sealed).unwrap(),
        "placeholder-provider-key-0001"
    );
    drop(db);
    let _ = std::fs::remove_dir_all(&dir);
}

/// One request, headers and body: a body can arrive in a later packet than its headers.
fn whole_request(socket: &mut std::net::TcpStream) -> String {
    let mut got = Vec::new();
    let mut buf = [0u8; 4096];
    loop {
        let n = socket.read(&mut buf).unwrap();
        got.extend_from_slice(&buf[..n]);
        let text = String::from_utf8_lossy(&got).into_owned();
        let Some(end) = text.find("\r\n\r\n") else {
            if n == 0 {
                return text;
            }
            continue;
        };
        let length = text[..end]
            .lines()
            .find_map(|l| {
                l.to_lowercase()
                    .strip_prefix("content-length:")
                    .map(|v| v.trim().parse::<usize>().unwrap_or(0))
            })
            .unwrap_or(0);
        if n == 0 || got.len() >= end + 4 + length {
            return text;
        }
    }
}

/// Answers each request in turn with the next `(status, body)`, and hands back what it was
/// sent.
fn answers(
    replies: Vec<(&'static str, &'static str)>,
) -> (String, std::thread::JoinHandle<Vec<String>>) {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    let seen = std::thread::spawn(move || {
        let mut seen = Vec::new();
        for (status, body) in replies {
            let (mut socket, _) = listener.accept().unwrap();
            seen.push(whole_request(&mut socket));
            let reply = format!(
                "HTTP/1.1 {status}\r\ncontent-type: application/json\r\ncontent-length: {}\r\nconnection: close\r\n\r\n{body}",
                body.len()
            );
            socket.write_all(reply.as_bytes()).unwrap();
        }
        seen
    });
    (url, seen)
}

#[test]
fn over_http_init_asks_for_three_shares_two_to_open_and_reads_them_back() {
    let (url, seen) = answers(vec![(
        "200 OK",
        r#"{"keys":["aa","bb","cc"],"keys_base64":["placeholder-a","placeholder-b","placeholder-c"],"root_token":"placeholder-root"}"#,
    )]);
    let made = HttpOpenBao::new(&url).init(SHARES, THRESHOLD).unwrap();
    assert_eq!(made.shares.len(), 3);
    assert_eq!(&*made.shares[0], "placeholder-a");
    let request = seen.join().unwrap().remove(0);
    assert!(request.starts_with("PUT /v1/sys/init "), "{request}");
    let compact: String = request.split_whitespace().collect();
    assert!(compact.contains(r#""secret_shares":3"#), "{request}");
    assert!(compact.contains(r#""secret_threshold":2"#), "{request}");
}

#[test]
fn over_http_a_call_carries_its_token_and_a_refusal_comes_back_as_what_was_said() {
    let (url, seen) = answers(vec![
        ("403 Forbidden", r#"{"errors":["permission denied"]}"#),
        (
            "503 Service Unavailable",
            r#"{"errors":["Vault is sealed"]}"#,
        ),
        (
            "200 OK",
            r#"{"type":"shamir","initialized":true,"sealed":true,"t":2,"n":3,"progress":1}"#,
        ),
    ]);
    let bao = HttpOpenBao::new(&url);
    let refused = bao.call(
        "placeholder-token",
        Verb::Post,
        "transit/encrypt/endora-node",
        None,
    );
    assert_eq!(
        refused,
        Err(VaultError::Answered {
            status: 403,
            message: "permission denied".to_owned()
        })
    );
    assert_eq!(
        bao.call("t", Verb::Get, "secret/data/endora/x", None),
        Err(VaultError::Sealed)
    );
    let status = bao.seal_status().unwrap();
    assert_eq!(
        status,
        SealStatus {
            initialized: true,
            sealed: true,
            threshold: 2,
            shares: 3,
            progress: 1
        }
    );
    let requests = seen.join().unwrap();
    assert!(
        requests[0]
            .to_lowercase()
            .contains("x-vault-token: placeholder-token"),
        "{}",
        requests[0]
    );
    assert!(
        !requests[2].to_lowercase().contains("x-vault-token"),
        "status needs no token"
    );
}

#[test]
fn a_vault_that_is_not_there_is_unreachable() {
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let url = format!("http://{}", listener.local_addr().unwrap());
    drop(listener);
    assert!(matches!(
        HttpOpenBao::new(&url).seal_status(),
        Err(VaultError::Unreachable(_))
    ));
}

/// The real image, end to end: init, unseal, setup, root revoked, Transit round trip, sealed
/// after a restart. Ignored: it needs Docker and the pinned image. Run it with
///
/// ```text
/// ENDORA_OPENBAO_IMAGE=openbao/openbao@sha256:… cargo test -p endora-infrastructure \
///     openbao::tests::against_the_real_image -- --ignored
/// ```
///
/// It uses whatever `DOCKER_CONTEXT` the shell has, so point it at a local Docker, never the
/// house's server.
#[test]
#[ignore = "needs Docker and the OpenBao image"]
fn against_the_real_image() {
    let image = std::env::var("ENDORA_OPENBAO_IMAGE").expect("ENDORA_OPENBAO_IMAGE");
    let name = format!("endora-openbao-test-{}", std::process::id());
    let config = r#"{"ui":false,"disable_mlock":true,"storage":{"raft":{"path":"/openbao/file","node_id":"test"}},"listener":[{"tcp":{"address":"0.0.0.0:8200","tls_disable":true,"disable_unauthed_rekey_endpoints":false}}],"api_addr":"http://127.0.0.1:8200","cluster_addr":"http://127.0.0.1:8201","audit":[{"type":"file","path":"endora-audit","options":{"file_path":"/openbao/file/audit.log"}}]}"#;
    let docker = |args: &[&str]| {
        std::process::Command::new("docker")
            .args(args)
            .output()
            .expect("docker")
    };
    let run = |name: &str| {
        let started = docker(&[
            "run",
            "-d",
            "--name",
            name,
            "--read-only",
            "--cap-drop",
            "ALL",
            "--memory",
            "256m",
            "-e",
            "SKIP_CHOWN=1",
            "--tmpfs",
            "/openbao/config:mode=1777",
            "-v",
            &format!("{name}-data:/openbao/file"),
            "-e",
            &format!("BAO_LOCAL_CONFIG={config}"),
            "-p",
            "127.0.0.1::8200",
            &image,
            "server",
        ]);
        assert!(
            started.status.success(),
            "{}",
            String::from_utf8_lossy(&started.stderr)
        );
        let port = docker(&["port", name, "8200"]);
        let port = String::from_utf8_lossy(&port.stdout)
            .trim()
            .rsplit(':')
            .next()
            .unwrap()
            .to_owned();
        format!("http://127.0.0.1:{port}")
    };
    let wait = |bao: &HttpOpenBao| {
        for _ in 0..100 {
            if bao.seal_status().is_ok() {
                return;
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
        panic!("the vault did not start");
    };
    let outcome = std::panic::catch_unwind(|| {
        let bao = Arc::new(HttpOpenBao::new(&run(&name)));
        wait(&bao);
        let made = initialise(bao.as_ref()).unwrap();
        unseal_with(bao.as_ref(), &made.shares[..2]).unwrap();
        let role = set_up_endora(bao.as_ref(), &made.root_token).unwrap();
        assert!(matches!(
            bao.call(&made.root_token, Verb::Get, "sys/mounts", None),
            Err(VaultError::Answered { status: 403, .. })
        ));
        let keys = TransitKeys::new(bao.clone(), role.clone());
        let key = DataKey::random();
        let wrapped = keys.wrap("0011223344556677", &key).unwrap();
        assert_eq!(
            keys.unwrap("0011223344556677", &wrapped)
                .unwrap()
                .as_bytes(),
            key.as_bytes()
        );
        assert!(keys.unwrap("8899aabbccddeeff", &wrapped).is_err());
        assert!(docker(&["restart", &name]).status.success());
        let port = docker(&["port", &name, "8200"]);
        let port = String::from_utf8_lossy(&port.stdout)
            .trim()
            .rsplit(':')
            .next()
            .unwrap()
            .to_owned();
        let bao = Arc::new(HttpOpenBao::new(&format!("http://127.0.0.1:{port}")));
        wait(&bao);
        assert!(bao.seal_status().unwrap().sealed, "a restart seals it");
        let keys = TransitKeys::new(bao.clone(), role);
        assert!(keys.unwrap("0011223344556677", &wrapped).is_err());
        unseal_with(bao.as_ref(), &made.shares[1..]).unwrap();
        assert_eq!(
            keys.unwrap("0011223344556677", &wrapped)
                .unwrap()
                .as_bytes(),
            key.as_bytes()
        );
        // The break-glass recovery: new shares from two old ones; after a restart only the
        // new ones open it.
        let fresh = rekey(bao.as_ref(), &made.shares[..2]).unwrap();
        assert_eq!(fresh.len(), 3);
        assert!(docker(&["restart", &name]).status.success());
        let port = docker(&["port", &name, "8200"]);
        let port = String::from_utf8_lossy(&port.stdout)
            .trim()
            .rsplit(':')
            .next()
            .unwrap()
            .to_owned();
        let bao = Arc::new(HttpOpenBao::new(&format!("http://127.0.0.1:{port}")));
        wait(&bao);
        assert!(unseal_with(bao.as_ref(), &made.shares).is_err());
        unseal_with(bao.as_ref(), &fresh[1..]).unwrap();
    });
    docker(&["rm", "-f", &name]);
    docker(&["volume", "rm", &format!("{name}-data")]);
    if let Err(panic) = outcome {
        std::panic::resume_unwind(panic);
    }
}

#[test]
fn rekeying_with_two_old_shares_makes_three_new_ones_and_the_old_no_longer_open_it() {
    let bao = FakeOpenBao::new();
    let made = initialise(&bao).unwrap();
    unseal_with(&bao, &made.shares).unwrap();
    assert!(
        rekey(&bao, &made.shares[..1]).is_err(),
        "one share is not enough"
    );
    let wrong = vec![Zeroizing::new("placeholder-wrong".to_owned()); 2];
    assert!(rekey(&bao, &wrong).is_err(), "shares that are not its own");
    let fresh = rekey(&bao, &made.shares[1..]).unwrap();
    assert_eq!(fresh.len(), 3);
    bao.seal();
    assert!(
        unseal_with(&bao, &made.shares).is_err(),
        "the old ones are gone"
    );
    unseal_with(&bao, &fresh[..2]).unwrap();
}
