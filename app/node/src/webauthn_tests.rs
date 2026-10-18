//! Passkey ceremonies, with a pretend authenticator that signs for real (ring's own P-256 and
//! Ed25519 keys): what a browser would hand the node, built byte by byte. A real WebAuthn
//! ceremony, with a phone and Face ID, cannot run in a test; everything after the browser can.

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::rand::SystemRandom;
use ring::signature::{ECDSA_P256_SHA256_ASN1_SIGNING, EcdsaKeyPair, Ed25519KeyPair, KeyPair as _};

use super::*;

const RP: &str = "endora.home.arpa";
const CHALLENGE: &[u8] = b"a-challenge-of-thirty-two-bytes!";

fn head(major: u8, n: usize) -> Vec<u8> {
    let n = n as u64;
    match n {
        0..=23 => vec![(major << 5) | u8::try_from(n).unwrap()],
        24..=0xff => vec![(major << 5) | 24, u8::try_from(n).unwrap()],
        _ => {
            let mut v = vec![(major << 5) | 25];
            v.extend(u16::try_from(n).unwrap().to_be_bytes());
            v
        }
    }
}
fn int(n: i64) -> Vec<u8> {
    if n >= 0 {
        head(0, usize::try_from(n).unwrap())
    } else {
        head(1, usize::try_from(-1 - n).unwrap())
    }
}
fn bytes(b: &[u8]) -> Vec<u8> {
    [head(2, b.len()), b.to_vec()].concat()
}
fn text(t: &str) -> Vec<u8> {
    [head(3, t.len()), t.as_bytes().to_vec()].concat()
}
fn map(entries: &[(Vec<u8>, Vec<u8>)]) -> Vec<u8> {
    let mut out = head(5, entries.len());
    for (k, v) in entries {
        out.extend(k);
        out.extend(v);
    }
    out
}

enum Key {
    P256(EcdsaKeyPair),
    Ed(Ed25519KeyPair),
}

/// A pretend authenticator: one credential, its key and its counter.
struct Authenticator {
    key: Key,
    id: Vec<u8>,
    count: u32,
}

impl Authenticator {
    fn p256(count: u32) -> Self {
        let rng = SystemRandom::new();
        let pkcs8 = EcdsaKeyPair::generate_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, &rng).unwrap();
        let key = EcdsaKeyPair::from_pkcs8(&ECDSA_P256_SHA256_ASN1_SIGNING, pkcs8.as_ref(), &rng)
            .unwrap();
        Self {
            key: Key::P256(key),
            id: b"credential-id-one".to_vec(),
            count,
        }
    }
    fn ed25519() -> Self {
        let pkcs8 = Ed25519KeyPair::generate_pkcs8(&SystemRandom::new()).unwrap();
        Self {
            key: Key::Ed(Ed25519KeyPair::from_pkcs8(pkcs8.as_ref()).unwrap()),
            id: b"credential-id-two".to_vec(),
            count: 0,
        }
    }
    fn cose(&self) -> Vec<u8> {
        match &self.key {
            Key::P256(k) => {
                let point = k.public_key().as_ref();
                map(&[
                    (int(1), int(2)),
                    (int(3), int(-7)),
                    (int(-1), int(1)),
                    (int(-2), bytes(&point[1..33])),
                    (int(-3), bytes(&point[33..65])),
                ])
            }
            Key::Ed(k) => map(&[
                (int(1), int(1)),
                (int(3), int(-8)),
                (int(-1), int(6)),
                (int(-2), bytes(k.public_key().as_ref())),
            ]),
        }
    }
    fn public(&self) -> PublicKey {
        match &self.key {
            Key::P256(k) => PublicKey {
                algorithm: ES256,
                bytes: k.public_key().as_ref().to_vec(),
            },
            Key::Ed(k) => PublicKey {
                algorithm: EDDSA,
                bytes: k.public_key().as_ref().to_vec(),
            },
        }
    }
    fn auth_data(&self, rp: &str, flags: u8, attested: bool) -> Vec<u8> {
        let mut data = digest(&SHA256, rp.as_bytes()).as_ref().to_vec();
        data.push(flags | if attested { 0x40 } else { 0 });
        data.extend(self.count.to_be_bytes());
        if attested {
            data.extend([0u8; 16]);
            data.extend(u16::try_from(self.id.len()).unwrap().to_be_bytes());
            data.extend(&self.id);
            data.extend(self.cose());
        }
        data
    }
    fn sign(&self, message: &[u8]) -> Vec<u8> {
        match &self.key {
            Key::P256(k) => k
                .sign(&SystemRandom::new(), message)
                .unwrap()
                .as_ref()
                .to_vec(),
            Key::Ed(k) => k.sign(message).as_ref().to_vec(),
        }
    }
}

fn client(kind: &str, challenge: &[u8], origin: &str) -> Vec<u8> {
    serde_json::json!({
        "type": kind,
        "challenge": URL_SAFE_NO_PAD.encode(challenge),
        "origin": origin,
        "crossOrigin": false,
    })
    .to_string()
    .into_bytes()
}

fn attestation(auth_data: &[u8]) -> Vec<u8> {
    map(&[
        (text("fmt"), text("none")),
        (text("attStmt"), map(&[])),
        (text("authData"), bytes(auth_data)),
    ])
}

/// An assertion as the browser would send it: (client data, authenticator data, signature).
fn assertion(auth: &Authenticator, origin: &str, flags: u8) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
    let client_data = client("webauthn.get", CHALLENGE, origin);
    let data = auth.auth_data(RP, flags, false);
    let mut signed = data.clone();
    signed.extend(digest(&SHA256, &client_data).as_ref());
    (client_data, data.clone(), auth.sign(&signed))
}

const ORIGIN: &str = "https://endora.home.arpa:8787";

#[test]
fn a_registration_gives_the_credential_and_its_p256_key() {
    let auth = Authenticator::p256(0);
    let got = check_registration(
        RP,
        CHALLENGE,
        &client("webauthn.create", CHALLENGE, ORIGIN),
        &attestation(&auth.auth_data(RP, 0x05, true)),
        &auth.id,
    )
    .unwrap();
    assert_eq!(got.credential_id, URL_SAFE_NO_PAD.encode(&auth.id));
    assert_eq!(got.public_key, auth.public());
    assert_eq!(got.public_key.bytes.len(), 65);
}

#[test]
fn a_registration_with_an_ed25519_key_is_understood_too() {
    let auth = Authenticator::ed25519();
    let got = check_registration(
        RP,
        CHALLENGE,
        &client("webauthn.create", CHALLENGE, ORIGIN),
        &attestation(&auth.auth_data(RP, 0x05, true)),
        &auth.id,
    )
    .unwrap();
    assert_eq!(got.public_key, auth.public());
}

#[test]
fn a_registration_is_refused_for_the_wrong_challenge_site_origin_or_without_verification() {
    let auth = Authenticator::p256(0);
    let good_data = attestation(&auth.auth_data(RP, 0x05, true));
    let cases: Vec<(&str, Vec<u8>, Vec<u8>, &str)> = vec![
        (
            "another challenge",
            client("webauthn.create", b"some-other-challenge", ORIGIN),
            good_data.clone(),
            "challenge",
        ),
        (
            "opened by its address",
            client("webauthn.create", CHALLENGE, "https://192.168.1.10:8787"),
            good_data.clone(),
            "only at https://endora.home.arpa",
        ),
        (
            "plain http",
            client("webauthn.create", CHALLENGE, "http://endora.home.arpa:8787"),
            good_data.clone(),
            "only at https://endora.home.arpa",
        ),
        (
            "a sign-in, not a registration",
            client("webauthn.get", CHALLENGE, ORIGIN),
            good_data.clone(),
            "another kind",
        ),
        (
            "another site's passkey",
            client("webauthn.create", CHALLENGE, ORIGIN),
            attestation(&auth.auth_data("example.com", 0x05, true)),
            "another site",
        ),
        (
            "present but not verified",
            client("webauthn.create", CHALLENGE, ORIGIN),
            attestation(&auth.auth_data(RP, 0x01, true)),
            "did not verify you",
        ),
    ];
    for (what, client_data, object, says) in cases {
        let error = check_registration(RP, CHALLENGE, &client_data, &object, &auth.id)
            .unwrap_err()
            .0;
        assert!(error.contains(says), "{what}: {error}");
    }
    let error = check_registration(
        RP,
        CHALLENGE,
        &client("webauthn.create", CHALLENGE, ORIGIN),
        &good_data,
        b"another-id",
    )
    .unwrap_err();
    assert!(error.0.contains("credential id"), "{error}");
}

#[test]
fn an_assertion_by_the_passkey_verifies_and_moves_its_counter() {
    let mut auth = Authenticator::p256(7);
    let (client_data, data, signature) = assertion(&auth, ORIGIN, 0x05);
    let count = check_assertion(
        RP,
        CHALLENGE,
        &client_data,
        &data,
        &signature,
        &auth.public(),
        6,
    )
    .unwrap();
    assert_eq!(count, 7);
    auth.count = 0;
    let (client_data, data, signature) = assertion(&auth, ORIGIN, 0x05);
    assert_eq!(
        check_assertion(
            RP,
            CHALLENGE,
            &client_data,
            &data,
            &signature,
            &auth.public(),
            0
        ),
        Ok(0),
        "a synced passkey's counter is always 0"
    );
}

#[test]
fn an_ed25519_assertion_verifies() {
    let auth = Authenticator::ed25519();
    let (client_data, data, signature) = assertion(&auth, ORIGIN, 0x05);
    assert!(
        check_assertion(
            RP,
            CHALLENGE,
            &client_data,
            &data,
            &signature,
            &auth.public(),
            0
        )
        .is_ok()
    );
}

#[test]
fn an_assertion_is_refused_for_another_key_a_tampered_signature_or_a_counter_gone_back() {
    let auth = Authenticator::p256(5);
    let (client_data, data, signature) = assertion(&auth, ORIGIN, 0x05);
    let other = Authenticator::p256(0).public();
    assert!(
        check_assertion(RP, CHALLENGE, &client_data, &data, &signature, &other, 0)
            .unwrap_err()
            .0
            .contains("signature")
    );
    let mut tampered = signature.clone();
    let last = tampered.len() - 1;
    tampered[last] ^= 1;
    assert!(
        check_assertion(
            RP,
            CHALLENGE,
            &client_data,
            &data,
            &tampered,
            &auth.public(),
            0
        )
        .is_err()
    );
    let mut altered_client = client_data.clone();
    altered_client.push(b' ');
    assert!(
        check_assertion(
            RP,
            CHALLENGE,
            &altered_client,
            &data,
            &signature,
            &auth.public(),
            0
        )
        .is_err(),
        "the signature covers the client data"
    );
    assert!(
        check_assertion(
            RP,
            CHALLENGE,
            &client_data,
            &data,
            &signature,
            &auth.public(),
            5
        )
        .unwrap_err()
        .0
        .contains("counter")
    );
    let (client_data, data, signature) = assertion(&auth, "https://192.168.1.10:8787", 0x05);
    assert!(
        check_assertion(
            RP,
            CHALLENGE,
            &client_data,
            &data,
            &signature,
            &auth.public(),
            0
        )
        .unwrap_err()
        .0
        .contains("https://endora.home.arpa")
    );
    let (client_data, data, signature) = assertion(&auth, ORIGIN, 0x01);
    assert!(
        check_assertion(
            RP,
            CHALLENGE,
            &client_data,
            &data,
            &signature,
            &auth.public(),
            0
        )
        .unwrap_err()
        .0
        .contains("did not verify")
    );
}

#[test]
fn cbor_that_is_cut_short_indefinite_or_too_deep_is_refused_not_panicked_on() {
    for input in [
        vec![],
        vec![0x58],
        vec![0x5f],
        vec![0x9f],
        vec![0xa1, 0x01],
        vec![0x81; 20],
        vec![0x5a, 0xff, 0xff, 0xff, 0xff],
    ] {
        assert!(cbor(&input, 0).is_err(), "{input:?}");
    }
    assert_eq!(cbor(&int(-7), 0).unwrap().0, Cbor::Int(-7));
}

/// A pretend phone, for the vault's tests: one passkey on an authenticator that signs for
/// real and answers the PRF extension the way the specification says
/// (`HMAC-SHA256(its secret, SHA-256("WebAuthn PRF" || 0x00 || salt))`).
pub(crate) struct Device {
    auth: Authenticator,
    prf_secret: [u8; 32],
}

impl Device {
    /// A device whose passkey has the credential id `id`.
    pub(crate) fn new(id: &[u8]) -> Self {
        let mut auth = Authenticator::p256(0);
        auth.id = id.to_vec();
        let mut prf_secret = [0u8; 32];
        ring::rand::SecureRandom::fill(&SystemRandom::new(), &mut prf_secret).unwrap();
        Self { auth, prf_secret }
    }

    /// The credential id, base64url.
    pub(crate) fn id(&self) -> String {
        URL_SAFE_NO_PAD.encode(&self.auth.id)
    }

    /// What `navigator.credentials.create` hands back for `challenge` at `origin`:
    /// (client data, attestation object).
    pub(crate) fn create(&self, challenge: &[u8], origin: &str) -> (Vec<u8>, Vec<u8>) {
        (
            client("webauthn.create", challenge, origin),
            attestation(&self.auth.auth_data(RP, 0x05, true)),
        )
    }

    /// What `navigator.credentials.get` hands back: (client data, authenticator data,
    /// signature). The counter moves on.
    pub(crate) fn get(&mut self, challenge: &[u8], origin: &str) -> (Vec<u8>, Vec<u8>, Vec<u8>) {
        self.auth.count += 1;
        let client_data = client("webauthn.get", challenge, origin);
        let data = self.auth.auth_data(RP, 0x05, false);
        let mut signed = data.clone();
        signed.extend(digest(&SHA256, &client_data).as_ref());
        (client_data, data, self.auth.sign(&signed))
    }

    /// The PRF output for `salt`.
    pub(crate) fn prf(&self, salt: &[u8]) -> Vec<u8> {
        let mut input = b"WebAuthn PRF\x00".to_vec();
        input.extend_from_slice(salt);
        let hashed = digest(&SHA256, &input);
        let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &self.prf_secret);
        ring::hmac::sign(&key, hashed.as_ref()).as_ref().to_vec()
    }
}
