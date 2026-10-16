use super::*;
use std::cell::RefCell;

const NOW: i64 = 1_792_065_600_000;
const MINUTE: i64 = 60 * 1000;

/// Spotify's accounts service, faked: answers each token request in turn and records them.
struct Fake {
    answers: RefCell<Vec<Result<Value, &'static str>>>,
    asked: RefCell<Vec<Vec<(String, String)>>>,
}

impl Fake {
    fn answering(answers: Vec<Result<Value, &'static str>>) -> Self {
        Self {
            answers: RefCell::new(answers),
            asked: RefCell::new(Vec::new()),
        }
    }
    fn asked(&self, n: usize, key: &str) -> String {
        self.asked.borrow()[n]
            .iter()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v.clone())
            .unwrap_or_default()
    }
}

impl SpotifyAccounts for Fake {
    fn token(&self, form: &[(&str, &str)]) -> Result<Value, &'static str> {
        self.asked.borrow_mut().push(
            form.iter()
                .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
                .collect(),
        );
        self.answers.borrow_mut().remove(0)
    }
    fn me(&self, _access: &str) -> Result<Value, &'static str> {
        Ok(json!({"display_name": "Jane Doe", "id": "janedoe"}))
    }
}

fn settings() -> crate::ConfigStore {
    let db = endora_persistence::Db::open_in_memory().unwrap();
    crate::migrate(&db).unwrap();
    crate::ConfigStore::new(db)
}

fn tokens(access: &str, refresh: Option<&str>) -> Result<Value, &'static str> {
    let mut v = json!({"access_token": access, "expires_in": 3600});
    if let Some(r) = refresh {
        v["refresh_token"] = json!(r);
    }
    Ok(v)
}

const VERIFIER: &str = "a-random-verifier-0123456789-0123456789-0123456789";
const STATE: &str = "a-random-state-0123456789";

#[test]
fn the_pkce_challenge_is_the_one_the_standard_gives() {
    // RFC 7636, appendix B.
    assert_eq!(
        pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
}

#[test]
fn the_protocols_sample_sign_in_address_is_exactly_one_this_node_builds() {
    // The handheld tests that the sample fits its screen as a QR code; that proves something only
    // while the node's addresses are no longer than it.
    let built = sign_in_url(
        &"ab12".repeat(8),
        &pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        &"cd34".repeat(8),
        PASTE_REDIRECT,
    );
    assert_eq!(
        endora_protocol::samples::connection_waiting().sign_in_url,
        Some(built)
    );
}

#[test]
fn the_code_comes_from_the_pasted_address_and_only_for_this_sign_in() {
    assert_eq!(
        code_from(
            &format!("{PASTE_REDIRECT}?code=the-code&state={STATE}"),
            STATE
        )
        .unwrap(),
        "the-code"
    );
    assert_eq!(
        code_from(&format!("code=the-code&state={STATE}"), STATE).unwrap(),
        "the-code"
    );
    assert!(code_from(&format!("{PASTE_REDIRECT}?code=x&state=another"), STATE).is_err());
    assert!(
        code_from(
            &format!("{PASTE_REDIRECT}?error=access_denied&state={STATE}"),
            STATE
        )
        .is_err()
    );
    assert!(code_from(&format!("{PASTE_REDIRECT}?state={STATE}"), STATE).is_err());
}

#[test]
fn a_sign_in_started_on_one_device_is_seen_and_finished_on_another() {
    let store = settings();
    let fake = Fake::answering(vec![tokens("access-1", Some("refresh-1"))]);
    let spotify = Connection::new(&store, &fake);
    assert_eq!(spotify.status(NOW)["state"], "not_set_up");
    assert!(
        spotify
            .begin("", PASTE_REDIRECT, VERIFIER, STATE, NOW)
            .is_err(),
        "a client id is needed"
    );
    let started = spotify
        .begin("exampleclientid", PASTE_REDIRECT, VERIFIER, STATE, NOW)
        .unwrap();
    assert_eq!(started["state"], "waiting");
    let url = started["sign_in_url"].as_str().unwrap();
    assert!(url.contains("client_id=exampleclientid"), "{url}");
    assert!(url.contains(&format!("code_challenge={}", pkce_challenge(VERIFIER))));
    assert!(
        !url.contains(VERIFIER),
        "the verifier never leaves the node"
    );
    // Another device asking sees the same sign-in, waiting.
    let elsewhere = Connection::new(&store, &fake).status(NOW + MINUTE);
    assert_eq!(elsewhere["state"], "waiting");
    assert_eq!(elsewhere["sign_in_url"], started["sign_in_url"]);
    // Finished there with the address Spotify sent the person to.
    let done = Connection::new(&store, &fake)
        .finish(
            &format!("{PASTE_REDIRECT}?code=the-code&state={STATE}"),
            NOW + 2 * MINUTE,
        )
        .unwrap();
    assert_eq!(done["state"], "connected");
    assert_eq!(done["account"], "Jane Doe");
    assert_eq!(fake.asked(0, "code_verifier"), VERIFIER);
    assert_eq!(fake.asked(0, "redirect_uri"), PASTE_REDIRECT);
    assert_eq!(fake.asked(0, "code"), "the-code");
    // Used up: the same address cannot finish a sign-in twice.
    assert!(
        spotify
            .finish(
                &format!("{PASTE_REDIRECT}?code=the-code&state={STATE}"),
                NOW + 3 * MINUTE
            )
            .is_err()
    );
}

#[test]
fn a_sign_in_left_waiting_too_long_is_not_waiting_any_more() {
    let store = settings();
    let fake = Fake::answering(vec![]);
    let spotify = Connection::new(&store, &fake);
    spotify
        .begin("exampleclientid", PASTE_REDIRECT, VERIFIER, STATE, NOW)
        .unwrap();
    let later = NOW + PENDING_FOR_MS;
    assert_eq!(spotify.status(later)["state"], "not_set_up");
    assert!(
        spotify
            .finish(&format!("{PASTE_REDIRECT}?code=c&state={STATE}"), later)
            .is_err()
    );
}

#[test]
fn a_lent_token_is_reused_until_nearly_spent_and_a_rotated_refresh_token_is_kept() {
    let store = settings();
    let fake = Fake::answering(vec![
        tokens("access-1", Some("refresh-1")),
        // Spotify rotates the refresh token on this refresh...
        tokens("access-2", Some("refresh-2")),
        // ...and not on this one, so the last one stays.
        tokens("access-3", None),
    ]);
    let spotify = Connection::new(&store, &fake);
    spotify
        .begin("exampleclientid", PASTE_REDIRECT, VERIFIER, STATE, NOW)
        .unwrap();
    spotify
        .finish(&format!("code=c&state={STATE}"), NOW)
        .unwrap();
    let (access, left) = spotify.lend(NOW + 30 * MINUTE).unwrap();
    assert_eq!((access.as_str(), left), ("access-1", 1800));
    assert_eq!(
        fake.asked.borrow().len(),
        1,
        "no refresh while it has time left"
    );
    let (access, _) = spotify.lend(NOW + 59 * MINUTE + 30_000).unwrap();
    assert_eq!(access, "access-2", "refreshed with under a minute left");
    assert_eq!(fake.asked(1, "refresh_token"), "refresh-1");
    let (access, _) = spotify.lend(NOW + 2 * 60 * MINUTE).unwrap();
    assert_eq!(access, "access-3");
    assert_eq!(
        fake.asked(2, "refresh_token"),
        "refresh-2",
        "the rotated one is used"
    );
    let (_, _) = spotify.lend(NOW + 2 * 60 * MINUTE + MINUTE).unwrap();
    assert_eq!(
        fake.asked.borrow().len(),
        3,
        "a refresh that gave no new refresh token kept the old one, and the token it gave is reused"
    );
}

#[test]
fn a_sign_in_a_device_already_has_is_taken_over_once_proved() {
    let store = settings();
    let refused = Fake::answering(vec![Err("Spotify refused; sign in again")]);
    assert!(
        Connection::new(&store, &refused)
            .take_over("exampleclientid", "stale", NOW)
            .is_err()
    );
    assert_eq!(
        Connection::new(&store, &refused).status(NOW)["state"],
        "not_set_up",
        "a refused token is not kept"
    );
    let fake = Fake::answering(vec![tokens("access-1", None)]);
    let spotify = Connection::new(&store, &fake);
    let taken = spotify
        .take_over("exampleclientid", "the-handhelds-refresh", NOW)
        .unwrap();
    assert_eq!(taken["state"], "connected");
    assert_eq!(fake.asked(0, "refresh_token"), "the-handhelds-refresh");
    assert!(spotify.take_over("not a client id!", "x", NOW).is_err());
    let gone = spotify.disconnect(NOW).unwrap();
    assert_eq!(gone["state"], "not_set_up");
    assert!(spotify.lend(NOW).is_err());
}
