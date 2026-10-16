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

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = std::env::var("HQ_TOKEN").unwrap_or_default();
    if token.trim().len() < SHORTEST_TOKEN {
        eprintln!(
            "HQ_TOKEN must be set to the token the Endora node uses (at least {SHORTEST_TOKEN} \
             characters). HQ does not start without one."
        );
        std::process::exit(2);
    }
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
