//! endora-sound (ADR 0103): the sound hub. It holds the patchbay (sources, speakers and the
//! wires between them) in its own state file and answers only those holding its token: the
//! Endora node, which is its remote. Nothing here plays sound yet; the mixer's engine comes
//! next, and watches the patchbay's revision.

use std::sync::Arc;

mod api;

use endora_sound::application::hub::Hub;
use endora_sound::infrastructure::FileStore;

/// The shortest token the hub accepts. Anything shorter is refused at start, so a node and
/// a hub can never be set up with a guessable one.
const SHORTEST_TOKEN: usize = 32;

/// The token the node signs with: `SOUND_TOKEN` if set, else the one in `SOUND_TOKEN_FILE`,
/// made there (random, readable by its owner only) the first time. The node reads the same
/// file through a volume only the two of them mount. The same scheme as HQ's.
fn the_token(env: Option<String>, file: Option<String>) -> Result<String, String> {
    if let Some(token) = env.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()) {
        if token.len() < SHORTEST_TOKEN {
            return Err(format!(
                "SOUND_TOKEN is shorter than {SHORTEST_TOKEN} characters"
            ));
        }
        return Ok(token);
    }
    let Some(file) = file.filter(|f| !f.trim().is_empty()) else {
        return Err("neither SOUND_TOKEN nor SOUND_TOKEN_FILE is set".to_owned());
    };
    if let Ok(existing) = std::fs::read_to_string(&file) {
        let existing = existing.trim().to_owned();
        if existing.len() >= SHORTEST_TOKEN {
            return Ok(existing);
        }
    }
    let mut bytes = [0u8; 24];
    std::io::Read::read_exact(
        &mut std::fs::File::open("/dev/urandom").map_err(|e| format!("no randomness: {e}"))?,
        &mut bytes,
    )
    .map_err(|e| format!("no randomness: {e}"))?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut out = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&file)
            .map_err(|e| format!("cannot write {file}: {e}"))?;
        out.write_all(token.as_bytes())
            .map_err(|e| format!("cannot write {file}: {e}"))?;
    }
    Ok(token)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = match the_token(
        std::env::var("SOUND_TOKEN").ok(),
        std::env::var("SOUND_TOKEN_FILE").ok(),
    ) {
        Ok(token) => token,
        Err(why) => {
            eprintln!("sound: {why}. The hub does not start without a token for the node.");
            std::process::exit(2);
        }
    };
    let state = std::env::var("SOUND_STATE").unwrap_or_else(|_| "sound.json".to_owned());
    let addr = std::env::var("SOUND_ADDR").unwrap_or_else(|_| "127.0.0.1:8791".to_owned());
    let hub = match Hub::open(FileStore::new(&state)) {
        Ok(hub) => hub,
        Err(e) => {
            // Never start empty over a state file it cannot read: the next save would throw
            // the owner's setups away.
            eprintln!("sound: {e}. The hub does not start over a state it cannot read.");
            std::process::exit(2);
        }
    };
    let app = api::app(api::State {
        hub: Arc::new(hub),
        token: Arc::new(token),
    });
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("sound: listening on {addr}, keeping its state in {state}");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::the_token;

    #[test]
    fn a_token_is_given_made_once_kept_and_never_short() {
        let dir = std::env::temp_dir().join(format!("endora-sound-token-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("token").to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&file);
        let made = the_token(None, Some(file.clone())).unwrap();
        assert_eq!(made.len(), 48);
        assert_eq!(the_token(None, Some(file.clone())).unwrap(), made, "kept");
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(&file).unwrap().permissions(),
        );
        assert_eq!(mode & 0o777, 0o600, "its owner's only");
        let given = "a-token-of-at-least-thirty-two-characters".to_owned();
        assert_eq!(the_token(Some(given.clone()), Some(file)).unwrap(), given);
        assert!(the_token(Some("short".to_owned()), None).is_err());
        assert!(the_token(None, None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
