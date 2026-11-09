//! Endora HQ (ADR 0102): the team service. It holds the team and the work in its own database,
//! runs the tick, and answers only the Endora node, which signs every request with a token the
//! two share. The node is the trust boundary; HQ knows nothing private. A runner, with a token
//! of its own, asks it for the turns to start.

use std::sync::Arc;

mod api;
mod backups;
mod drain_cli;
mod runner;
mod team;
mod ticker;
mod time_zone;

/// Endora's public library, embedded at build time for every member's `read_doc`.
pub(crate) mod public_docs {
    include!(concat!(env!("OUT_DIR"), "/public_docs.rs"));
}

use endora_team::application::engine::{Engine, Settings};
use endora_team::domain::money;
use endora_team::infrastructure::{RemoteRunner, Runner, SqliteTeamStore};

/// The engine HQ runs: its store, its runner (the remote one a real runner asks for work; the
/// simulated one in tests), and the system clock.
pub type Hq = Engine<SqliteTeamStore, Runner, api::SystemClock>;

/// The shortest token HQ accepts. Anything shorter is refused at start, so a node and an HQ
/// can never be set up with a guessable one.
const SHORTEST_TOKEN: usize = 32;

/// How many events the feed keeps, not counting the ticks.
const KEEP_EVENTS: i64 = 20_000;

/// What the home lab's routine asks of its member each time.
const LAB_CHECK: &str = "Look at the home lab with homelab_status, read logs of anything \
    unhealthy, fix what a restart can fix, check back after a fix, and put anything else to the \
    owner as a plan.";

/// The owner's distance from UTC in minutes, for where their day starts: the zone the node is
/// given (`ENDORA_TIME_ZONE`, e.g. `America/New_York`), else the fixed
/// `ENDORA_UTC_OFFSET_MINUTES`, else UTC. Taken once, at `now`: after a clock change it is an
/// hour out until HQ next starts, which is still a day that begins at the owner's midnight
/// rather than at eight in the evening. A zone nobody knows is an error, never quietly UTC.
fn owners_offset(env: impl Fn(&str) -> Option<String>, now: i64) -> Result<i64, String> {
    Ok(owners_zone(env)?.offset_minutes_at(now))
}

/// The owner's time zone itself, which follows clock changes, for what runs every night
/// (`backups`) and so cannot keep one offset taken at start.
fn owners_zone(env: impl Fn(&str) -> Option<String>) -> Result<time_zone::TimeZone, String> {
    let zone = env("ENDORA_TIME_ZONE").unwrap_or_default();
    let fixed = match env("ENDORA_UTC_OFFSET_MINUTES")
        .map(|v| v.trim().to_owned())
        .filter(|v| !v.is_empty())
    {
        None => 0,
        Some(minutes) => minutes.parse::<i64>().map_err(|_| {
            format!("ENDORA_UTC_OFFSET_MINUTES must be whole minutes from UTC, not {minutes}")
        })?,
    };
    time_zone::TimeZone::named_or_fixed(&zone, fixed)
}

/// Now, in milliseconds since the epoch.
fn now_ms() -> i64 {
    use endora_team::application::ports::Clock as _;
    api::SystemClock.now()
}

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
    // `endora-hq drain` and `endora-hq resume` (ADR 0134) talk to the HQ that is running and
    // exit. Any argument at all is a command or an error: it never starts a second HQ over the
    // same database.
    let args: Vec<String> = std::env::args().skip(1).collect();
    if let Some(code) = drain_cli::handle(&args) {
        std::process::exit(code);
    }
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
    let tick_every = std::env::var("HQ_TICK_SECONDS")
        .ok()
        .and_then(|v| v.parse::<u64>().ok())
        .filter(|s| *s >= 1)
        .unwrap_or(30);
    if runner_token.is_none() {
        eprintln!(
            "hq: neither HQ_RUNNER_TOKEN nor HQ_RUNNER_TOKEN_FILE is set, so no runner can ask \
             for work: turns wait until they time out."
        );
    }

    // HQ's own nightly copy of its store, in `backups/` beside it (milestone f).
    backups::spawn(
        std::path::PathBuf::from(&db),
        owners_zone(|name| std::env::var(name).ok())?,
    );

    let runner = Runner::Remote(RemoteRunner::default());
    let settings = Settings {
        offset_minutes: owners_offset(|name| std::env::var(name).ok(), now_ms())?,
        // Downtime (ADR 0119): a member with nothing to do tidies the office or its notebook,
        // free on the house's model.
        downtime: true,
        // Ping-pong and pool between free members (ADR 0121): played in code, free.
        games: true,
        // The nightly quiz on the map (ADR 0126): a free member, in a downtime turn on the
        // house's model, scored by code.
        quiz: true,
        pool_per_month: THE_HOUSES_POOL,
        ..Settings::default()
    };
    // The owner's new work asks the ticker for a pass at once (ADR 0102, amended 2026-11-04).
    let pass_asked = Arc::new(ticker::PassAsked::default());
    let asks = pass_asked.clone();
    let hq: Arc<Hq> = Arc::new(
        Engine::new(
            SqliteTeamStore::open(&db)?,
            runner,
            api::SystemClock,
            settings,
        )
        .with_docs(public_docs::PUBLIC_DOCS)
        .with_pass_requests(Arc::new(move || asks.ask())),
    );
    // Before seeding work, publishing HTTP or starting ticks: no v1 peer work
    // may execute in this process. Failure aborts startup; the next startup
    // repeats the bounded cleanup and settles only still-live legacy turns.
    hq.retire_legacy_peer_work()?;
    {
        // The team's map (ADR 0126): a page for every project and goal, made once each from the
        // team as it stands. Nothing is changed or deleted.
        match hq.migrate_map() {
            Ok(made) if made.total() == 0 => {}
            Ok(made) => println!(
                "hq: made the map's pages: {} projects, {} goals",
                made.projects, made.goals
            ),
            Err(e) => eprintln!("hq: could not make the map's pages: {e}"),
        }
        // The office's decorations as data (ADR 0119), once: the team moves them from there.
        match hq.seed_office() {
            Ok(true) => println!("hq: put the office's decorations on the floor"),
            Ok(false) => {}
            Err(e) => eprintln!("hq: could not seed the office: {e}"),
        }
        // Routines from before their origin was kept, whose words are exactly HQ's own: their
        // tasks are HQ's words, not unknown ones (ADR 0138 A2b).
        match hq.vouch_template_routines(&[("Check the home lab", LAB_CHECK)]) {
            Ok(0) => {}
            Ok(n) => println!("hq: {n} routines are HQ's own words"),
            Err(e) => eprintln!("hq: could not vouch for HQ's routines: {e}"),
        }
    }

    // The tick: every `tick_every` seconds, a late one run once, and one more whenever the
    // engine asks (the owner's new work, ADR 0102, amended 2026-11-04); one pass at a time. When
    // each one finished is kept for `/health` (ADR 0124).
    let ticking = hq.clone();
    let last_tick: Arc<std::sync::atomic::AtomicI64> = Arc::default();
    let ticked = last_tick.clone();
    let mut ticks: u64 = 0;
    tokio::spawn(ticker::run(
        std::time::Duration::from_secs(tick_every),
        pass_asked,
        move || {
            let hq = ticking.clone();
            let ticked = ticked.clone();
            ticks += 1;
            let prune = ticks % 100 == 0;
            async move {
                let _ = tokio::task::spawn_blocking(move || {
                    match hq.tick() {
                        Ok(_) => ticked.store(
                            endora_team::application::Clock::now(&api::SystemClock),
                            std::sync::atomic::Ordering::SeqCst,
                        ),
                        Err(e) => eprintln!("hq: the tick failed: {e}"),
                    }
                    // The feed is for display: the newest 20,000 events that are not ticks are kept
                    // (and the ticks among the newest 20,000 of all), and a reader further behind
                    // than that starts again from a snapshot.
                    if prune {
                        if let Err(e) = hq.prune_events(KEEP_EVENTS) {
                            eprintln!("hq: could not prune the event feed: {e}");
                        }
                    }
                })
                .await;
            }
        },
    ));

    let app = api::app(api::State {
        hq,
        token: Arc::new(token.trim().to_owned()),
        runner_token: runner_token.map(Arc::new),
        tick_seconds: tick_every,
        last_tick,
    });
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("hq: listening on {addr}, ticking every {tick_every} s");
    axum::serve(listener, app).await?;
    Ok(())
}

/// The team's pool a month (ADR 0113): the owner raised it from $70 to $95 on 2026-10-25, $10
/// of it the model league's cap (ADR 0135) and the rest the agents'. Since 2026-10-31 it is the
/// floor: each day's pool is sized from the Claude grant left, between this over 30 days and
/// three times that (ADR 0113, amended). The engine's own default
/// stays $70, the figure its tests and the scripted demo are written around.
const THE_HOUSES_POOL: money::Micros = 95 * money::DOLLAR;

#[cfg(test)]
mod tests {
    use super::{THE_HOUSES_POOL, money, owners_offset, the_runner_token, the_token};

    /// The owner, 2026-10-25: the team's pool is $95 a month, $10 of it the model league's cap
    /// and the rest the agents' (ADR 0113, amended).
    #[test]
    fn the_houses_pool_is_the_owners_95_dollars_a_month() {
        assert_eq!(THE_HOUSES_POOL, 95 * money::DOLLAR);
        assert_eq!(money::pool_a_day(THE_HOUSES_POOL), 3_166_666);
    }

    fn env(pairs: &[(&str, &str)]) -> impl Fn(&str) -> Option<String> {
        let pairs: Vec<(String, String)> = pairs
            .iter()
            .map(|(k, v)| ((*k).to_owned(), (*v).to_owned()))
            .collect();
        move |name| {
            pairs
                .iter()
                .find(|(k, _)| k == name)
                .map(|(_, v)| v.clone())
        }
    }

    #[test]
    fn the_owners_day_starts_where_the_node_says_it_does() {
        // 2026-10-18 00:09 UTC: eight in the evening of the 17th in New York.
        let evening = 1_792_282_140_000;
        assert_eq!(owners_offset(env(&[]), evening), Ok(0), "nothing said: UTC");
        assert_eq!(
            owners_offset(env(&[("ENDORA_UTC_OFFSET_MINUTES", "-240")]), evening),
            Ok(-240)
        );
        assert_eq!(
            owners_offset(
                env(&[
                    ("ENDORA_TIME_ZONE", "America/New_York"),
                    ("ENDORA_UTC_OFFSET_MINUTES", "0")
                ]),
                evening
            ),
            Ok(-240),
            "the zone wins, and knows it is still summer time"
        );
        assert_eq!(
            owners_offset(
                env(&[("ENDORA_TIME_ZONE", "America/New_York")]),
                1_768_000_000_000
            ),
            Ok(-300),
            "in January the same zone is five hours west"
        );
        assert!(owners_offset(env(&[("ENDORA_TIME_ZONE", "Mars/Olympus")]), evening).is_err());
        assert!(owners_offset(env(&[("ENDORA_UTC_OFFSET_MINUTES", "four")]), evening).is_err());
    }

    /// HQ is built from the team and nothing of the butler's (ADR 0136): the node's adapters,
    /// and through them its chat, brain, mail and agenda, are not HQ's to compile, and retiring
    /// them must not touch HQ.
    #[test]
    fn hq_builds_without_the_butlers_crates() {
        let manifest = include_str!("../Cargo.toml");
        for butler in [
            "endora-infrastructure",
            "endora-application",
            "endora-conversation",
            "endora-understanding",
            "endora-scheduling",
            "endora-mail",
            "endora-agenda",
        ] {
            assert!(
                !manifest.contains(butler),
                "app/hq depends on {butler}, one of the butler's crates"
            );
        }
    }

    #[test]
    fn the_teams_library_is_the_public_records_plans_drafts_and_standards_and_nothing_else() {
        let docs = crate::public_docs::PUBLIC_DOCS;
        for wanted in [
            "docs/adr/0111-the-ceo-and-the-chain-of-command.md",
            "docs/plans/endora-hq.md",
            "docs/standards.md",
            "docs/drafts/ansible-nas-AGENTS.md",
        ] {
            let (_, text) = docs.iter().find(|(p, _)| *p == wanted).expect(wanted);
            assert!(text.starts_with("# "), "{wanted}");
        }
        for (path, _) in docs {
            assert!(
                endora_team::domain::docs::asked(path).is_ok(),
                "{path} is not one read_doc reads"
            );
        }
        assert!(!docs.iter().any(|(p, _)| p.contains("archive")));
    }

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
