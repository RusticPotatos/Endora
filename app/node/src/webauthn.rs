//! Passkeys (WebAuthn), the little of it the vault unlock needs (ADR 0127): checking a
//! registration and an assertion for one relying party, with ES256 or Ed25519 keys.
//!
//! **Why not a WebAuthn crate.** `webauthn-rs` is the well-maintained one, and it was the first
//! choice. It verifies through OpenSSL, which this workspace has kept out (the node
//! cross-compiles for the NAS with rustls and ring), and it carries attestation, resident-key
//! and policy machinery a single owner with a few passkeys does not use, behind an API that
//! does not hand over the PRF extension's output, which is the one thing this needs. What is
//! left to check is small and fixed by the specification: the client data (type, challenge,
//! origin), the authenticator data (relying party hash, user present and verified, counter),
//! and one signature — done here with `ring`, already in the build through rustls. The COSE key
//! in a registration is read with a CBOR reader of about sixty lines that accepts only
//! definite lengths.
//!
//! **What is not checked: attestation.** The console asks for none (`attestation: "none"`),
//! so a registration proves nothing about the authenticator's make; it is accepted only from
//! a signed-in owner session, which is what decides whose passkey it is.
//!
//! **The PRF output is not signed.** WebAuthn's PRF extension result comes back beside the
//! assertion, not inside what the authenticator signs. So the signature proves the passkey was
//! there for this challenge, and the PRF output proves itself: a wrong one does not open the
//! key it is supposed to unwrap (the AEAD tag fails).

use base64::Engine as _;
use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use ring::digest::{SHA256, digest};
use ring::signature::{ECDSA_P256_SHA256_ASN1, ED25519, UnparsedPublicKey};

/// COSE algorithm: ECDSA with P-256 and SHA-256 (what iCloud Keychain and most keys make).
pub const ES256: i64 = -7;
/// COSE algorithm: Ed25519 (some hardware keys).
pub const EDDSA: i64 = -8;

const USER_PRESENT: u8 = 0x01;
const USER_VERIFIED: u8 = 0x04;
const ATTESTED: u8 = 0x40;

/// Why a passkey ceremony was refused. The message says what did not match, never a value.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Refused(pub String);

impl core::fmt::Display for Refused {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        f.write_str(&self.0)
    }
}

fn refused<T>(why: &str) -> Result<T, Refused> {
    Err(Refused(why.to_owned()))
}

/// A passkey's public key, as kept.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PublicKey {
    /// [`ES256`] or [`EDDSA`].
    pub algorithm: i64,
    /// An uncompressed P-256 point (65 bytes) or an Ed25519 key (32 bytes).
    pub bytes: Vec<u8>,
}

/// What a checked registration gives.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Registered {
    /// The credential id, base64url.
    pub credential_id: String,
    /// Its public key.
    pub public_key: PublicKey,
    /// The counter it started at.
    pub sign_count: u32,
}

// ---- a CBOR reader, definite lengths only ------------------------------------------------

/// One CBOR item, as much of CBOR as a COSE key and an attestation object use.
#[derive(Debug, Clone, PartialEq, Eq)]
enum Cbor {
    Int(i128),
    Bytes(Vec<u8>),
    Text(String),
    Array(Vec<Cbor>),
    Map(Vec<(Cbor, Cbor)>),
    Simple(u8),
}

impl Cbor {
    fn get(&self, key: &Cbor) -> Option<&Cbor> {
        let Self::Map(entries) = self else {
            return None;
        };
        entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }
}

/// Reads one item from the front of `input`; returns it and how many bytes it took.
fn cbor(input: &[u8], depth: u8) -> Result<(Cbor, usize), Refused> {
    if depth > 8 {
        return refused("the CBOR is nested too deeply");
    }
    let Some(&first) = input.first() else {
        return refused("the CBOR ended early");
    };
    let major = first >> 5;
    let info = first & 0x1f;
    let (value, mut used) = match info {
        0..=23 => (u64::from(info), 1),
        24..=27 => {
            let width = 1usize << (info - 24);
            let bytes = input
                .get(1..=width)
                .ok_or_else(|| Refused("the CBOR ended early".to_owned()))?;
            (
                bytes.iter().fold(0u64, |n, b| (n << 8) | u64::from(*b)),
                1 + width,
            )
        }
        _ => return refused("the CBOR uses an indefinite length"),
    };
    let take = |used: usize, n: u64| -> Result<&[u8], Refused> {
        let n = usize::try_from(n).map_err(|_| Refused("the CBOR is too long".to_owned()))?;
        input
            .get(used..used.saturating_add(n))
            .ok_or_else(|| Refused("the CBOR ended early".to_owned()))
    };
    let item = match major {
        0 => Cbor::Int(i128::from(value)),
        1 => Cbor::Int(-1 - i128::from(value)),
        2 => {
            let bytes = take(used, value)?.to_vec();
            used += bytes.len();
            Cbor::Bytes(bytes)
        }
        3 => {
            let bytes = take(used, value)?;
            used += bytes.len();
            Cbor::Text(
                String::from_utf8(bytes.to_vec())
                    .map_err(|_| Refused("the CBOR text is not UTF-8".to_owned()))?,
            )
        }
        4 | 5 => {
            if value > 64 {
                return refused("the CBOR holds too many items");
            }
            let mut items = Vec::new();
            let mut pairs = Vec::new();
            for _ in 0..value {
                let (a, n) = cbor(&input[used..], depth + 1)?;
                used += n;
                if major == 4 {
                    items.push(a);
                    continue;
                }
                let (b, n) = cbor(&input[used..], depth + 1)?;
                used += n;
                pairs.push((a, b));
            }
            if major == 4 {
                Cbor::Array(items)
            } else {
                Cbor::Map(pairs)
            }
        }
        7 => Cbor::Simple(u8::try_from(value).unwrap_or(u8::MAX)),
        _ => return refused("the CBOR holds a tagged item"),
    };
    Ok((item, used))
}

// ---- the parts of a ceremony -------------------------------------------------------------

struct AuthenticatorData {
    sign_count: u32,
    credential: Option<(Vec<u8>, Cbor)>,
}

fn authenticator_data(rp_id: &str, data: &[u8]) -> Result<AuthenticatorData, Refused> {
    if data.len() < 37 {
        return refused("the authenticator data is too short");
    }
    if data[..32] != *digest(&SHA256, rp_id.as_bytes()).as_ref() {
        return refused("the passkey is for another site");
    }
    let flags = data[32];
    if flags & USER_PRESENT == 0 || flags & USER_VERIFIED == 0 {
        return refused("the passkey did not verify you (Face ID, Touch ID or a PIN is needed)");
    }
    let sign_count = u32::from_be_bytes([data[33], data[34], data[35], data[36]]);
    if flags & ATTESTED == 0 {
        return Ok(AuthenticatorData {
            sign_count,
            credential: None,
        });
    }
    // 16 bytes of AAGUID, then the credential id's length and the id, then its COSE key.
    let rest = &data[37..];
    if rest.len() < 18 {
        return refused("the authenticator data is too short");
    }
    let id_len = usize::from(u16::from_be_bytes([rest[16], rest[17]]));
    let id = rest
        .get(18..18 + id_len)
        .ok_or_else(|| Refused("the credential id is cut short".to_owned()))?
        .to_vec();
    let (key, _) = cbor(&rest[18 + id_len..], 0)?;
    Ok(AuthenticatorData {
        sign_count,
        credential: Some((id, key)),
    })
}

fn cose_key(key: &Cbor) -> Result<PublicKey, Refused> {
    let int = |n: i128| Cbor::Int(n);
    let bytes = |label: i128| match key.get(&int(label)) {
        Some(Cbor::Bytes(b)) if b.len() == 32 => Ok(b.clone()),
        _ => refused("the passkey's public key is not one this understands"),
    };
    match (key.get(&int(1)), key.get(&int(3)), key.get(&int(-1))) {
        // EC2, ES256, P-256.
        (Some(Cbor::Int(2)), Some(Cbor::Int(-7)), Some(Cbor::Int(1))) => {
            let mut point = vec![0x04];
            point.extend(bytes(-2)?);
            point.extend(bytes(-3)?);
            Ok(PublicKey {
                algorithm: ES256,
                bytes: point,
            })
        }
        // OKP, EdDSA, Ed25519.
        (Some(Cbor::Int(1)), Some(Cbor::Int(-8)), Some(Cbor::Int(6))) => Ok(PublicKey {
            algorithm: EDDSA,
            bytes: bytes(-2)?,
        }),
        _ => refused("the passkey uses a key type other than ES256 or Ed25519"),
    }
}

/// Checks the browser's client data: the ceremony's type, the challenge this node issued, and
/// an origin of `https://<rp_id>` on any port.
fn client_data(json: &[u8], kind: &str, challenge: &[u8], rp_id: &str) -> Result<(), Refused> {
    let data: serde_json::Value = serde_json::from_slice(json)
        .map_err(|_| Refused("the client data is not JSON".to_owned()))?;
    if data["type"].as_str() != Some(kind) {
        return refused("the client data is for another kind of ceremony");
    }
    let offered = data["challenge"]
        .as_str()
        .and_then(|c| URL_SAFE_NO_PAD.decode(c.trim_end_matches('=')).ok());
    if offered.as_deref() != Some(challenge) {
        return refused("the challenge does not match the one this node issued");
    }
    if data["crossOrigin"].as_bool() == Some(true) {
        return refused("the ceremony came from inside another site");
    }
    let origin = data["origin"].as_str().unwrap_or("");
    let host = origin
        .strip_prefix("https://")
        .map(|rest| rest.split(':').next().unwrap_or(rest));
    if host != Some(rp_id) {
        return refused(&format!(
            "a passkey works only at https://{rp_id}, not at the address this was opened by"
        ));
    }
    Ok(())
}

/// Checks a registration (`navigator.credentials.create`): client data for `rp_id` and this
/// `challenge`, the user verified, and the credential's id and key from the attestation
/// object's authenticator data.
///
/// # Errors
/// [`Refused`] saying what did not match.
pub fn check_registration(
    rp_id: &str,
    challenge: &[u8],
    client_data_json: &[u8],
    attestation_object: &[u8],
    raw_id: &[u8],
) -> Result<Registered, Refused> {
    client_data(client_data_json, "webauthn.create", challenge, rp_id)?;
    let (object, _) = cbor(attestation_object, 0)?;
    let Some(Cbor::Bytes(auth_data)) = object.get(&Cbor::Text("authData".to_owned())) else {
        return refused("the attestation object holds no authenticator data");
    };
    let parsed = authenticator_data(rp_id, auth_data)?;
    let Some((id, key)) = parsed.credential else {
        return refused("the registration carries no credential");
    };
    if id != raw_id {
        return refused("the credential id does not match the one the browser gave");
    }
    Ok(Registered {
        credential_id: URL_SAFE_NO_PAD.encode(&id),
        public_key: cose_key(&key)?,
        sign_count: parsed.sign_count,
    })
}

/// Checks an assertion (`navigator.credentials.get`) by a known passkey: client data, the
/// user verified, the signature over the authenticator data and the client data's hash, and a
/// counter that moved forward (a counter of 0 is a synced passkey's, and is not compared).
/// Returns the new counter.
///
/// # Errors
/// [`Refused`] saying what did not match.
pub fn check_assertion(
    rp_id: &str,
    challenge: &[u8],
    client_data_json: &[u8],
    authenticator_data_bytes: &[u8],
    signature: &[u8],
    key: &PublicKey,
    last_count: u32,
) -> Result<u32, Refused> {
    client_data(client_data_json, "webauthn.get", challenge, rp_id)?;
    let parsed = authenticator_data(rp_id, authenticator_data_bytes)?;
    let mut signed = authenticator_data_bytes.to_vec();
    signed.extend_from_slice(digest(&SHA256, client_data_json).as_ref());
    let algorithm: &dyn ring::signature::VerificationAlgorithm = match key.algorithm {
        ES256 => &ECDSA_P256_SHA256_ASN1,
        EDDSA => &ED25519,
        _ => return refused("the passkey's key type is not one this understands"),
    };
    if UnparsedPublicKey::new(algorithm, &key.bytes)
        .verify(&signed, signature)
        .is_err()
    {
        return refused("the passkey's signature does not verify");
    }
    if parsed.sign_count != 0 && parsed.sign_count <= last_count {
        return refused("the passkey's counter went backwards: it may have been copied");
    }
    Ok(parsed.sign_count)
}

#[cfg(test)]
#[path = "webauthn_tests.rs"]
pub(crate) mod tests;
