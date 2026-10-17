//! Endora HQ (ADR 0102): the team service. It holds the team and the work in its own database,
//! runs the tick, and answers only the Endora node, which signs every request with a token the
//! two share. The node is the trust boundary; HQ knows nothing private. A runner, with a token
//! of its own, asks it for the turns to start.

use std::sync::Arc;

mod api;
mod runner;
mod team;

use endora_team::application::engine::{Engine, Settings};
use endora_team::infrastructure::{RemoteRunner, Runner, SimulatedRunner, SqliteTeamStore, demo};

/// The engine HQ runs: its store, its runner (the simulated one for the demo, else the remote
/// one a real runner asks for work), and the system clock.
pub type Hq = Engine<SqliteTeamStore, Runner, api::SystemClock>;

/// The shortest token HQ accepts. Anything shorter is refused at start, so a node and an HQ
/// can never be set up with a guessable one.
const SHORTEST_TOKEN: usize = 32;

/// How many events the feed keeps.
const KEEP_EVENTS: i64 = 20_000;

/// The token the node signs with: `HQ_TOKEN` if set, else the one in `HQ_TOKEN_FILE`, made
/// there (random, readable by its owner only) the first time. The node reads the same file
/// through a volume only the two of them mount, so nobody has to keep a secret by hand.
/// `name` is the variable's, for the runner's token made the same way.
fn the_token(name: &str, env: Option<String>, file: Option<String>) -> Result<String, String> {
    if let Some(token) = env.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()) {
        if token.len() < SHORTEST_TOKEN {
            return Err(format!(
                "{name} is shorter than {SHORTEST_TOKEN} characters"
            ));
        }
        return Ok(token);
    }
    let Some(file) = file.filter(|f| !f.trim().is_empty()) else {
        return Err(format!("neither {name} nor {name}_FILE is set"));
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

/// The token a runner signs with: `HQ_RUNNER_TOKEN`, else `HQ_RUNNER_TOKEN_FILE`, made like
/// the node's. `None` when neither is set: then no runner is answered. Never the node's own,
/// so neither can open the other's routes.
fn the_runner_token(
    node: &str,
    env: Option<String>,
    file: Option<String>,
) -> Result<Option<String>, String> {
    let set = |v: &Option<String>| v.as_ref().is_some_and(|v| !v.trim().is_empty());
    if !set(&env) && !set(&file) {
        return Ok(None);
    }
    let token = the_token("HQ_RUNNER_TOKEN", env, file)?;
    if token == node.trim() {
        return Err("the runner's token is the node's".to_owned());
    }
    Ok(Some(token))
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = match the_token(
        "HQ_TOKEN",
        std::env::var("HQ_TOKEN").ok(),
        std::env::var("HQ_TOKEN_FILE").ok(),
    ) {
        Ok(token) => token,
        Err(why) => {
            eprintln!("hq: {why}. HQ does not start without a token for the node.");
            std::process::exit(2);
        }
    };
    let runner_token = match the_runner_token(
        &token,
        std::env::var("HQ_RUNNER_TOKEN").ok(),
        std::env::var("HQ_RUNNER_TOKEN_FILE").ok(),
    ) {
        Ok(runner_token) => runner_token,
        Err(why) => {
            eprintln!("hq: {why}. HQ does not start with an unusable token for the runner.");
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
    if !demo_mode && runner_token.is_none() {
        eprintln!(
            "hq: neither HQ_RUNNER_TOKEN nor HQ_RUNNER_TOKEN_FILE is set, so no runner can ask \
             for work: turns wait until they time out."
        );
    }

    // The demo's scripted members take their turns on the simulated runner; otherwise a real
    // runner asks for them.
    let runner = if demo_mode {
        Runner::Simulated(SimulatedRunner::new(4))
    } else {
        Runner::Remote(RemoteRunner::default())
    };
    let hq: Arc<Hq> = Arc::new(Engine::new(
        SqliteTeamStore::open(&db)?,
        runner,
        api::SystemClock,
        Settings::default(),
    ));
    let goal = if demo_mode {
        Some(api::demo_goal(&hq)?)
    } else {
        None
    };
    // With the demo off, nothing it left behind becomes a real turn: its goals and their open
    // tasks are put away, and a notice on the owner's desk says so.
    if !demo_mode {
        match demo::retire(&hq) {
            Ok(0) => {}
            Ok(n) => println!("hq: the demo is off: put away its {n} goals and their open work"),
            Err(e) => eprintln!("hq: could not put the demo's work away: {e}"),
        }
    }

    // The tick: every `tick_every` seconds; a late one runs once.
    let ticking = hq.clone();
    let mut ticks: u64 = 0;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(tick_every));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let hq = ticking.clone();
            ticks += 1;
            let prune = ticks % 100 == 0;
            let _ = tokio::task::spawn_blocking(move || {
                if let Err(e) = hq.tick() {
                    eprintln!("hq: the tick failed: {e}");
                }
                // The feed is for display: the newest 20,000 events are kept, and a reader
                // further behind than that starts again from a snapshot.
                if prune {
                    if let Err(e) = hq.prune_events(KEEP_EVENTS) {
                        eprintln!("hq: could not prune the event feed: {e}");
                    }
                }
                if let (Some(goal), Some(scripted)) = (goal, hq.runner().simulated()) {
                    if let Err(e) = demo::act(&hq, scripted, goal) {
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
        runner_token: runner_token.map(Arc::new),
        demo: demo_mode,
        tick_seconds: tick_every,
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
    use super::{the_runner_token, the_token};

    #[test]
    fn a_token_is_given_made_once_kept_and_never_short() {
        let dir = std::env::temp_dir().join(format!("endora-hq-token-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("token").to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&file);
        let made = the_token("HQ_TOKEN", None, Some(file.clone())).unwrap();
        assert_eq!(made.len(), 48);
        assert_eq!(
            the_token("HQ_TOKEN", None, Some(file.clone())).unwrap(),
            made,
            "kept"
        );
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(&file).unwrap().permissions(),
        );
        assert_eq!(mode & 0o777, 0o600, "its owner's only");
        let given = "a-token-of-at-least-thirty-two-characters".to_owned();
        assert_eq!(
            the_token("HQ_TOKEN", Some(given.clone()), Some(file)).unwrap(),
            given
        );
        assert!(the_token("HQ_TOKEN", Some("short".to_owned()), None).is_err());
        assert!(the_token("HQ_TOKEN", None, None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_runners_token_is_made_like_the_nodes_and_is_never_the_same() {
        let dir = std::env::temp_dir().join(format!("endora-hq-runner-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("runner-token").to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&file);
        let node = "a-token-of-at-least-thirty-two-characters";
        assert_eq!(
            the_runner_token(node, None, None),
            Ok(None),
            "no token, no runner"
        );
        let made = the_runner_token(node, None, Some(file.clone()))
            .unwrap()
            .unwrap();
        assert_eq!(made.len(), 48);
        assert_eq!(
            the_runner_token(node, None, Some(file.clone())),
            Ok(Some(made)),
            "kept"
        );
        let short = the_runner_token(node, Some("short".to_owned()), None).unwrap_err();
        assert!(short.contains("HQ_RUNNER_TOKEN"), "{short}");
        assert!(the_runner_token(node, Some(node.to_owned()), None).is_err());
        // The node's own file, named for the runner by mistake, is refused too.
        std::fs::write(&file, node).unwrap();
        assert!(the_runner_token(node, None, Some(file)).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }
}
