//! Endora HQ (ADR 0102): the team service. It holds the team and the work in its own database,
//! runs the tick, and answers only the Endora node, which signs every request with a token the
//! two share. The node is the trust boundary; HQ knows nothing private.

use std::sync::Arc;

mod api;

use endora_team::application::engine::{Engine, Settings};
use endora_team::infrastructure::{SimulatedRunner, SqliteTeamStore, demo};

/// The engine HQ runs: its store, the simulated runner (phase 2: no real workers yet), and the
/// system clock.
pub type Hq = Engine<SqliteTeamStore, SimulatedRunner, api::SystemClock>;

/// The shortest token HQ accepts. Anything shorter is refused at start, so a node and an HQ
/// can never be set up with a guessable one.
const SHORTEST_TOKEN: usize = 32;

/// The token the node signs with: `HQ_TOKEN` if set, else the one in `HQ_TOKEN_FILE`, made
/// there (random, readable by its owner only) the first time. The node reads the same file
/// through a volume only the two of them mount, so nobody has to keep a secret by hand.
fn the_token(env: Option<String>, file: Option<String>) -> Result<String, String> {
    if let Some(token) = env.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()) {
        if token.len() < SHORTEST_TOKEN {
            return Err(format!(
                "HQ_TOKEN is shorter than {SHORTEST_TOKEN} characters"
            ));
        }
        return Ok(token);
    }
    let Some(file) = file.filter(|f| !f.trim().is_empty()) else {
        return Err("neither HQ_TOKEN nor HQ_TOKEN_FILE is set".to_owned());
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
        std::env::var("HQ_TOKEN").ok(),
        std::env::var("HQ_TOKEN_FILE").ok(),
    ) {
        Ok(token) => token,
        Err(why) => {
            eprintln!("hq: {why}. HQ does not start without a token for the node.");
            std::process::exit(2);
        }
    };
    let db = std::env::var("HQ_DB").unwrap_or_else(|_| "hq.db".to_owned());
    let addr = std::env::var("HQ_ADDR").unwrap_or_else(|_| "127.0.0.1:8790".to_owned());
    let demo_mode = std::env::var("HQ_DEMO").is_ok_and(|v| v == "1");
    let tick_every = std::env::var("HQ_TICK_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|s| *s >= 1)
        .unwrap_or(30);

    let hq: Arc<Hq> = Arc::new(Engine::new(
        SqliteTeamStore::open(&db)?,
        SimulatedRunner::new(4),
        api::SystemClock,
        Settings::default(),
    ));
    let goal = if demo_mode {
        Some(api::demo_goal(&hq)?)
    } else {
        None
    };

    // The tick: every `tick_every` seconds; a late one runs once.
    let ticking = hq.clone();
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(tick_every));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let hq = ticking.clone();
            let _ = tokio::task::spawn_blocking(move || {
                if let Err(e) = hq.tick() {
                    eprintln!("hq: the tick failed: {e}");
                }
                if let Some(goal) = goal {
                    if let Err(e) = demo::act(&hq, goal) {
                        eprintln!("hq: the demo team stumbled: {e}");
                    }
                }
            })
            .await;
        }
    });

    let app = api::app(api::State {
        hq,
        token: Arc::new(token.trim().to_owned()),
        demo: demo_mode,
    });
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!(
        "hq: listening on {addr}, ticking every {tick_every} s{}",
        if demo_mode { ", demo team on" } else { "" }
    );
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::the_token;

    #[test]
    fn a_token_is_given_made_once_kept_and_never_short() {
        let dir = std::env::temp_dir().join(format!("endora-hq-token-{}", std::process::id()));
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
