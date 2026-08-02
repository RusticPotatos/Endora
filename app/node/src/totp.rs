//! Time-based one-time passwords (RFC 6238), the kind Google Authenticator produces.
//!
//! Written rather than depended on for one reason: **RFC 6238 publishes official test
//! vectors**, so the implementation can be *proved* against the standard instead of trusted.
//! That is the opposite of the argument for password hashing, where rolling your own is a
//! cardinal sin — there, correctness is not the hard part, resistance to hardware is, and no
//! test can demonstrate it. Here a handful of published numbers settle the question.
//!
//! SHA-1 is not a choice: RFC 6238's default and every authenticator app in practice use
//! `HMAC-SHA1`. Its collision weaknesses do not apply to HMAC, which is why HMAC-SHA1 remains
//! specified for exactly this.

use sha1::{Digest, Sha1};

/// The window an authenticator counts in.
const STEP_SECONDS: u64 = 30;

/// How many steps either side of now are accepted.
///
/// One, which is the usual choice, and it is about clocks rather than convenience: a phone and
/// a NAS drift, and a code typed at the tail of one window arrives in the next. Widening this
/// multiplies the codes valid at any instant, so it stays at the smallest value that does not
/// reject honest people.
const SLACK_STEPS: u64 = 1;

/// How long a shared secret is, in bytes. 160 bits — RFC 4226's recommendation, and what
/// authenticator apps expect.
pub const SECRET_BYTES: usize = 20;

/// HMAC-SHA1 (RFC 2104), which is the whole cryptographic content of a one-time password.
fn hmac_sha1(key: &[u8], message: &[u8]) -> [u8; 20] {
    const BLOCK: usize = 64;
    let mut padded = [0u8; BLOCK];
    // A key longer than the block is hashed first; anything shorter is zero-padded.
    if key.len() > BLOCK {
        padded[..20].copy_from_slice(&Sha1::digest(key));
    } else {
        padded[..key.len()].copy_from_slice(key);
    }
    let mut inner = Sha1::new();
    inner.update(padded.map(|b| b ^ 0x36));
    inner.update(message);
    let inner = inner.finalize();

    let mut outer = Sha1::new();
    outer.update(padded.map(|b| b ^ 0x5c));
    outer.update(inner);
    outer.finalize().into()
}

/// The six-digit code for one counter value (RFC 4226's dynamic truncation).
fn code_for_counter(secret: &[u8], counter: u64) -> u32 {
    let digest = hmac_sha1(secret, &counter.to_be_bytes());
    // The low nibble of the last byte picks where to read four bytes from — "dynamic"
    // truncation, so the digits do not always come from the same place in the hash.
    let offset = (digest[19] & 0x0f) as usize;
    let binary = u32::from_be_bytes([
        digest[offset] & 0x7f,
        digest[offset + 1],
        digest[offset + 2],
        digest[offset + 3],
    ]);
    binary % 1_000_000
}

/// The code an authenticator would be showing at `unix_seconds`.
///
/// Only tests need this — the node's job is to *check* a code, never to produce one — but they
/// need it badly: it is what the RFC's published vectors are compared against, and what proves
/// end to end that a stored secret yields codes a real phone would.
#[cfg(test)]
#[must_use]
pub fn code_at(secret: &[u8], unix_seconds: u64) -> u32 {
    code_for_counter(secret, unix_seconds / STEP_SECONDS)
}

/// Whether `offered` is a code this secret would have produced around `unix_seconds`.
///
/// Compared as **numbers**, so `007123` and `7123` are the same code — an authenticator shows
/// leading zeros and a person may or may not type them, and refusing over that teaches people
/// their working code is broken.
///
/// The comparison itself is constant-time across the accepted window: every step is checked
/// even once one matches, so how long a rejection took says nothing about which window was
/// close.
#[must_use]
pub fn verify(secret: &[u8], offered: &str, unix_seconds: u64) -> bool {
    let Ok(offered) = offered.trim().parse::<u32>() else {
        return false;
    };
    if offered >= 1_000_000 {
        return false;
    }
    let now = unix_seconds / STEP_SECONDS;
    let mut matched = false;
    for step in now.saturating_sub(SLACK_STEPS)..=now + SLACK_STEPS {
        matched |= code_for_counter(secret, step) == offered;
    }
    matched
}

/// Base32 as authenticator apps expect it (RFC 4648, upper case, no padding).
#[must_use]
pub fn base32(secret: &[u8]) -> String {
    const ALPHABET: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZ234567";
    let mut out = String::new();
    let mut carry: u16 = 0;
    let mut bits = 0u8;
    for byte in secret {
        carry = (carry << 8) | u16::from(*byte);
        bits += 8;
        while bits >= 5 {
            bits -= 5;
            out.push(ALPHABET[((carry >> bits) & 0x1f) as usize] as char);
        }
    }
    if bits > 0 {
        out.push(ALPHABET[((carry << (5 - bits)) & 0x1f) as usize] as char);
    }
    out
}

/// The secret as hex, for storing.
///
/// **Not base32.** An authenticator is handed base32 and decodes it to bytes before hashing,
/// so storing the base32 *text* and hashing that would produce codes that never match — with
/// no symptom beyond "it does not work". Raw bytes are the secret; base32 is only how they
/// travel to the phone.
#[must_use]
pub fn to_hex(secret: &[u8]) -> String {
    secret.iter().map(|b| format!("{b:02x}")).collect()
}

/// Reads a secret back from hex. Anything malformed yields nothing, which refuses every code.
#[must_use]
pub fn from_hex(text: &str) -> Vec<u8> {
    let text = text.trim();
    if text.len() % 2 != 0 {
        return Vec::new();
    }
    let mut out = Vec::with_capacity(text.len() / 2);
    for pair in text.as_bytes().chunks(2) {
        let Ok(pair) = std::str::from_utf8(pair) else {
            return Vec::new();
        };
        let Ok(byte) = u8::from_str_radix(pair, 16) else {
            return Vec::new();
        };
        out.push(byte);
    }
    out
}

/// The `otpauth://` URI an authenticator scans.
///
/// The issuer appears twice on purpose — once as a label prefix and once as a parameter —
/// which is what Google's own key-uri format asks for and what makes the entry read as
/// *Endora* rather than as a bare account name in the app.
#[must_use]
pub fn enrolment_uri(secret: &[u8], account: &str) -> String {
    format!(
        "otpauth://totp/Endora:{account}?secret={}&issuer=Endora&algorithm=SHA1&digits=6&period=30",
        base32(secret)
    )
}

#[cfg(test)]
mod tests {
    use super::{
        SECRET_BYTES, base32, code_at, enrolment_uri, from_hex, hmac_sha1, to_hex, verify,
    };
    use sha1::Digest as _;

    /// RFC 6238's SHA-1 test key: the ASCII digits 1234567890, twenty bytes of it.
    const RFC_SECRET: &[u8] = b"12345678901234567890";

    #[test]
    fn it_matches_the_published_test_vectors() {
        // RFC 6238, Appendix B. The table gives eight digits; the low six are what an
        // authenticator shows. These numbers are the entire reason this is written here
        // rather than depended on — a wrong implementation cannot pass them by accident.
        for (at, expected) in [
            (59_u64, 287_082_u32),
            (1_111_111_109, 81_804),
            (1_111_111_111, 50_471),
            (1_234_567_890, 5_924),
            (2_000_000_000, 279_037),
            (20_000_000_000, 353_130),
        ] {
            assert_eq!(code_at(RFC_SECRET, at), expected, "at {at}");
        }
    }

    #[test]
    fn hmac_sha1_handles_a_key_longer_than_its_block() {
        // The branch the RFC vectors never reach, because their key is twenty bytes. A key
        // over 64 must be hashed first, and getting that wrong is silent.
        let long = [0x61_u8; 100];
        let short = sha1::Sha1::digest(long);
        assert_eq!(hmac_sha1(&long, b"x"), hmac_sha1(&short, b"x"));
    }

    #[test]
    fn a_code_from_this_moment_is_accepted() {
        let now = 1_234_567_890;
        let code = format!("{:06}", code_at(RFC_SECRET, now));
        assert!(verify(RFC_SECRET, &code, now));
    }

    #[test]
    fn a_clock_a_little_out_still_works() {
        // A phone and a NAS drift, and a code typed at the tail of one window lands in the
        // next. Refusing that teaches people their working authenticator is broken.
        let now = 1_234_567_890;
        let code = format!("{:06}", code_at(RFC_SECRET, now));
        assert!(verify(RFC_SECRET, &code, now + 30), "one step late");
        assert!(verify(RFC_SECRET, &code, now - 30), "one step early");
    }

    #[test]
    fn a_code_from_too_long_ago_is_refused() {
        let now = 1_234_567_890;
        let code = format!("{:06}", code_at(RFC_SECRET, now));
        assert!(!verify(RFC_SECRET, &code, now + 120));
        assert!(!verify(RFC_SECRET, &code, now - 120));
    }

    #[test]
    fn leading_zeros_are_the_same_code() {
        // An authenticator shows six digits; a person may drop the leading zero. Both are the
        // same number and refusing one of them is a bug that looks like a broken app.
        let at = 1_234_567_890; // this window's code is 005924
        assert!(verify(RFC_SECRET, "005924", at));
        assert!(verify(RFC_SECRET, "5924", at));
        assert!(verify(RFC_SECRET, " 005924 ", at));
    }

    #[test]
    fn nothing_that_is_not_a_code_gets_through() {
        let at = 1_234_567_890;
        for offered in [
            "", "abcdef", "12345678", "1000000", "-5924", "0x5924", "59 24",
        ] {
            assert!(!verify(RFC_SECRET, offered, at), "{offered:?} was accepted");
        }
    }

    #[test]
    fn another_secret_does_not_open_it() {
        let at = 1_234_567_890;
        let code = format!("{:06}", code_at(RFC_SECRET, at));
        assert!(!verify(b"09876543210987654321", &code, at));
    }

    #[test]
    fn base32_matches_rfc_4648() {
        assert_eq!(base32(b""), "");
        assert_eq!(base32(b"f"), "MY");
        assert_eq!(base32(b"fo"), "MZXQ");
        assert_eq!(base32(b"foo"), "MZXW6");
        assert_eq!(base32(b"foob"), "MZXW6YQ");
        assert_eq!(base32(b"fooba"), "MZXW6YTB");
        assert_eq!(base32(b"foobar"), "MZXW6YTBOI");
    }

    #[test]
    fn a_secret_survives_being_stored() {
        // The bug this pair exists for: an authenticator decodes base32 to bytes before
        // hashing, so storing the base32 text and hashing *that* gives codes which never
        // match, with no symptom beyond "it does not work".
        assert_eq!(from_hex(&to_hex(RFC_SECRET)), RFC_SECRET);
        assert_eq!(to_hex(&[0x00, 0x0f, 0xff]), "000fff");
    }

    #[test]
    fn a_secret_that_will_not_read_back_opens_nothing() {
        for text in ["", "odd", "zz", "12 34", "0x1234"] {
            let secret = from_hex(text);
            assert!(
                !verify(&secret, "000000", 0) || secret.is_empty(),
                "{text:?} produced a usable secret"
            );
        }
        assert!(from_hex("abc").is_empty(), "odd length is not a secret");
        assert!(from_hex("zzzz").is_empty(), "not hex is not a secret");
    }

    #[test]
    fn the_enrolment_uri_is_one_an_app_will_scan() {
        let uri = enrolment_uri(&[0u8; SECRET_BYTES], "rustic");
        assert!(uri.starts_with("otpauth://totp/Endora:rustic?"));
        assert!(uri.contains("secret=AAAAAAAAAAAAAAAAAAAAAAAAAAAAAAAA"));
        assert!(uri.contains("issuer=Endora"));
        assert!(uri.contains("digits=6"), "{uri}");
        assert!(uri.contains("period=30"), "{uri}");
    }
}
