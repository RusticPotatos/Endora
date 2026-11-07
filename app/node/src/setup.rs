//! `endora-core setup`: the first run of a headless Core (the product reset, revision 6).
//!
//! Makes the owner on the machine Core runs on: a password and an authenticator, which is how
//! Core signs people in today (ADR 0146). It is the command-line twin of the console's claim
//! (`POST /v1/session/setup`), for an install with no screen in front yet, and it needs no
//! window and no token from the log: a shell on the host is the proof.
//!
//! It provisions **nothing else**. Model and provider keys, tokens and connections are the
//! owner's own steps afterwards, through Console or the API; nothing is made unattended.

use std::error::Error;
use std::io::{BufRead, Write};

use endora_infrastructure::SqliteStore;

/// What setting the owner up hands back: the authenticator's enrolment, shown once.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Made {
    /// The `otpauth://` URI an authenticator app takes.
    pub otpauth: String,
    /// The same secret as base32, for an app that is typed into.
    pub key: String,
}

/// Why setup made nothing.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// The node has an owner already; setup never replaces one.
    AlreadyOwned,
    /// Shorter than the twelve characters every password here needs.
    TooShort,
    /// The store could not be read or written.
    Store(String),
}

impl std::fmt::Display for Refused {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::AlreadyOwned => f.write_str(
                "this node already has an owner; nothing was changed (change the password in \
                 Console's Settings while signed in)",
            ),
            Self::TooShort => f.write_str(
                "a password shorter than twelve characters is not worth hashing; nothing was made",
            ),
            Self::Store(e) => write!(f, "the node's store: {e}"),
        }
    }
}

impl Error for Refused {}

/// Makes the owner in `store` with `password`, unless there is one already.
///
/// # Errors
/// [`Refused`] when the node has an owner, the password is short, or the store fails.
pub fn make_owner(store: &SqliteStore, password: &str) -> Result<Made, Refused> {
    let store_error = |e: endora_application::RepositoryError| Refused::Store(e.to_string());
    if password.chars().count() < 12 {
        return Err(Refused::TooShort);
    }
    let (hash, _) = store.sign_in_setup().map_err(store_error)?;
    if !hash.is_empty() {
        return Err(Refused::AlreadyOwned);
    }
    // The authenticator secret is made once and never regenerated, as the console's claim does.
    crate::api::ensure_enrolment_secret(store, &endora_infrastructure::RandomIdSource)
        .map_err(store_error)?;
    let hash = crate::api::hash_password(password).map_err(Refused::Store)?;
    let (_, secret) = store.sign_in_setup().map_err(store_error)?;
    store
        .set_sign_in_setup(&hash, &secret)
        .map_err(store_error)?;
    let bytes = crate::totp::from_hex(&secret);
    Ok(Made {
        otpauth: crate::totp::enrolment_uri(&bytes, "you"),
        key: crate::totp::base32(&bytes),
    })
}

/// Handles `setup` if that is what `args` say, returning `None` otherwise.
pub fn dispatch(args: &[String], database: &str) -> Option<Result<(), Box<dyn Error>>> {
    if args.first().map(String::as_str) != Some("setup") {
        return None;
    }
    if args.len() != 1 {
        return Some(Err(
            "usage: endora-core setup   (the password is read from stdin)".into(),
        ));
    }
    let stdin = std::io::stdin();
    // At a terminal, the password is not echoed while it is typed; piped, it is one line.
    let hidden = std::io::IsTerminal::is_terminal(&stdin) && echo(false);
    if hidden {
        eprint!("The owner's password (12 characters or more): ");
    }
    let done = run(database, &mut stdin.lock(), &mut std::io::stdout());
    if hidden {
        echo(true);
        eprintln!();
    }
    Some(done)
}

/// Turns the terminal's echo on or off with `stty`, saying whether that worked.
fn echo(on: bool) -> bool {
    std::process::Command::new("stty")
        .arg(if on { "echo" } else { "-echo" })
        .stdin(std::process::Stdio::inherit())
        .status()
        .is_ok_and(|s| s.success())
}

/// The command itself: reads the password as one line of `input`, makes the owner in the store
/// at `database`, and tells `out` how to finish enrolling.
///
/// # Errors
/// As [`make_owner`], or when the store cannot be opened.
pub fn run(
    database: &str,
    input: &mut dyn BufRead,
    out: &mut dyn Write,
) -> Result<(), Box<dyn Error>> {
    let store = crate::secrets::open_store(database)?;
    let mut line = String::new();
    input.read_line(&mut line)?;
    let password = line.trim_end_matches(['\r', '\n']);
    let made = make_owner(&store, password)?;
    writeln!(
        out,
        "The owner is made. Add Endora to your authenticator app now:"
    )?;
    writeln!(out)?;
    if let Ok(qr) = qrcode::QrCode::new(made.otpauth.as_bytes()) {
        let drawn = qr
            .render::<qrcode::render::unicode::Dense1x2>()
            .quiet_zone(true)
            .build();
        writeln!(out, "{drawn}")?;
        writeln!(out)?;
    }
    writeln!(out, "    {}", made.otpauth)?;
    writeln!(out)?;
    writeln!(out, "or type the key: {}", made.key)?;
    writeln!(out)?;
    writeln!(
        out,
        "This is shown once. Then sign in with the password and a code from the app. Model \
         and provider keys, tokens and connections are yours to add afterwards, in Console's \
         Settings or through the API; setup made none of them."
    )?;
    Ok(())
}

#[cfg(test)]
#[path = "setup_scenarios.rs"]
mod scenarios;

#[cfg(test)]
mod tests {
    use super::dispatch;

    #[test]
    fn only_setup_is_taken_and_it_takes_no_arguments() {
        let args = |a: &[&str]| a.iter().map(|s| (*s).to_owned()).collect::<Vec<_>>();
        assert!(dispatch(&args(&[]), "unused.db").is_none());
        assert!(dispatch(&args(&["backup", "x"]), "unused.db").is_none());
        // A password on the command line would sit in the shell's history: refused.
        let refused = dispatch(&args(&["setup", "a-password-here"]), "unused.db")
            .expect("taken")
            .expect_err("refused");
        assert!(refused.to_string().contains("stdin"), "{refused}");
    }
}
