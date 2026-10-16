use crate::Result;
use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::{
    KeyInit, XChaCha20Poly1305, XNonce,
    aead::{Aead, OsRng, Payload, rand_core::RngCore},
};
use zeroize::Zeroizing;

fn key(passphrase: &str, salt: &[u8]) -> Result<Zeroizing<[u8; 32]>> {
    let mut key = Zeroizing::new([0; 32]);
    let params =
        Params::new(19_456, 2, 1, Some(32)).map_err(|_| "Cannot initialize device lock")?;
    Argon2::new(Algorithm::Argon2id, Version::V0x13, params)
        .hash_password_into(passphrase.as_bytes(), salt, &mut *key)
        .map_err(|_| "Cannot derive device key")?;
    Ok(key)
}

pub fn validate_passphrase(passphrase: &str) -> Result<()> {
    if passphrase.chars().count() < 4 {
        return Err("Use a device passcode of at least 4 characters".into());
    }
    Ok(())
}

pub fn seal(passphrase: &str, origin: &str, plaintext: &[u8]) -> Result<Vec<u8>> {
    validate_passphrase(passphrase)?;
    let mut salt = [0; 16];
    let mut nonce = [0; 24];
    OsRng.fill_bytes(&mut salt);
    OsRng.fill_bytes(&mut nonce);
    let key = key(passphrase, &salt)?;
    let encrypted = XChaCha20Poly1305::new((&*key).into())
        .encrypt(
            XNonce::from_slice(&nonce),
            Payload {
                msg: plaintext,
                aad: origin.as_bytes(),
            },
        )
        .map_err(|_| "Cannot encrypt session")?;
    let mut output = b"ETV1".to_vec();
    output.extend(salt);
    output.extend(nonce);
    output.extend(encrypted);
    Ok(output)
}

pub fn open(passphrase: &str, origin: &str, envelope: &[u8]) -> Result<Zeroizing<Vec<u8>>> {
    if envelope.len() < 60 || envelope.len() > 64 * 1024 || &envelope[..4] != b"ETV1" {
        return Err("Invalid encrypted session".into());
    }
    let key = key(passphrase, &envelope[4..20])?;
    let bytes = XChaCha20Poly1305::new((&*key).into())
        .decrypt(
            XNonce::from_slice(&envelope[20..44]),
            Payload {
                msg: &envelope[44..],
                aad: origin.as_bytes(),
            },
        )
        .map_err(|_| "Device passphrase rejected or session damaged")?;
    Ok(Zeroizing::new(bytes))
}
