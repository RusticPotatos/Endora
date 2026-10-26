//! Endora HQ (ADR 0102): the team service. It holds the team and the work in its own database,
//! runs the tick, and answers only the Endora node, which signs every request with a token the
//! two share. The node is the trust boundary; HQ knows nothing private. A runner, with a token
//! of its own, asks it for the turns to start.

use std::sync::Arc;

mod api;
mod drain_cli;
mod runner;
mod team;
mod time_zone;

/// Endora's public library, embedded at build time for every member's `read_doc`.
pub(crate) mod public_docs {
    include!(concat!(env!("OUT_DIR"), "/public_docs.rs"));
}

use endora_team::application::engine::{Engine, Settings};
use endora_team::infrastructure::{RemoteRunner, Runner, SimulatedRunner, SqliteTeamStore, demo};

/// The engine HQ runs: its store, its runner (the simulated one for the demo, else the remote
/// one a real runner asks for work), and the system clock.
pub type Hq = Engine<SqliteTeamStore, Runner, api::SystemClock>;

/// The shortest token HQ accepts. Anything shorter is refused at start, so a node and an HQ
/// can never be set up with a guessable one.
const SHORTEST_TOKEN: usize = 32;

/// How many events the feed keeps, not counting the ticks.
const KEEP_EVENTS: i64 = 20_000;

/// The owner's goal for the home lab, as they set it.
pub(crate) const LAB_GOAL: &str = "Keep the home lab healthy";

/// What the home lab's routine asks of its member each time.
const LAB_CHECK: &str = "Look at the home lab with homelab_status, read logs of anything \
    unhealthy, fix what a restart can fix, check back after a fix, and put anything else to the \
    owner as a plan.";

/// The CEO's name when HQ seats one (ADR 0111): the owner may rename it.
const CEO_NAME: &str = "Mara";

/// What the CEO's daily review asks of it each day (ADR 0111, rebuilt by ADR 0120).
const DAILY_REVIEW: &str = endora_team::application::engine::DAILY_REVIEW;

/// The owner's routine for the home lab (ADR 0107), made once: with no routine yet but the
/// CEO's, an active goal called "Keep the home lab healthy" and a lead, the lead checks the
/// home lab every 6 hours, and takes its alarms. It is the owner's to switch off or run, like
/// any other; nothing about it is in the schema. Returns the routine, if it made one.
pub(crate) fn seed_routine(
    hq: &Hq,
) -> Result<Option<endora_team::domain::RoutineId>, endora_team::application::EngineError> {
    use endora_team::application::ports::{NewRoutine, TeamStore as _};
    use endora_team::domain::{GoalStatus, Role};
    let store = hq.store();
    let members = store.members()?;
    let the_ceos = |assignee| {
        members
            .iter()
            .any(|m| m.id == assignee && m.role == Role::Ceo)
    };
    if store.routines()?.iter().any(|r| !the_ceos(r.assignee)) {
        return Ok(None);
    }
    let Some(goal) = store
        .goals()?
        .into_iter()
        .find(|g| g.status == GoalStatus::Active && g.title.trim().eq_ignore_ascii_case(LAB_GOAL))
    else {
        return Ok(None);
    };
    let Some(lead) = store.members()?.into_iter().find(|m| m.role == Role::Lead) else {
        return Ok(None);
    };
    let made = hq.add_routine(
        endora_team::application::Author::Endora,
        NewRoutine {
            title: "Check the home lab".to_owned(),
            description: LAB_CHECK.to_owned(),
            goal: goal.id,
            project: None,
            assignee: lead.id,
            every_ms: 6 * 60 * 60 * 1000,
            on_lab_trouble: true,
        },
    )?;
    Ok(Some(made))
}

/// The owner's distance from UTC in minutes, for where their day starts: the zone the node is
/// given (`ENDORA_TIME_ZONE`, e.g. `America/New_York`), else the fixed
/// `ENDORA_UTC_OFFSET_MINUTES`, else UTC. Taken once, at `now`: after a clock change it is an
/// hour out until HQ next starts, which is still a day that begins at the owner's midnight
/// rather than at eight in the evening. A zone nobody knows is an error, never quietly UTC.
fn owners_offset(env: impl Fn(&str) -> Option<String>, now: i64) -> Result<i64, String> {
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
    let tz = time_zone::TimeZone::named_or_fixed(&zone, fixed)?;
    Ok(tz.offset_minutes_at(now))
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

/// What [`seed_ceo`] made: the CEO seated, its daily review, and its weekly planning.
pub(crate) type SeededCeo = (
    Option<endora_team::domain::MemberId>,
    Option<endora_team::domain::RoutineId>,
    Option<endora_team::domain::RoutineId>,
);

/// The CEO, its daily review (ADR 0111) and its weekly planning (ADR 0113), for HQ started with
/// the demo off: the CEO seated once (Mara, on Opus, said in the feed), and once the team has a
/// CEO and an active goal, a "Daily review" routine for it every 24 hours and a "Planning"
/// routine every 7 days, under the home lab's goal if it has one, each made once. Returns what
/// it made.
pub(crate) fn seed_ceo(hq: &Hq) -> Result<SeededCeo, endora_team::application::EngineError> {
    use endora_team::application::ports::{NewRoutine, TeamStore as _};
    use endora_team::domain::{GoalStatus, Role};
    let seated = hq.seat_ceo(CEO_NAME)?;
    let store = hq.store();
    let Some(ceo) = store.members()?.into_iter().find(|m| m.role == Role::Ceo) else {
        return Ok((seated, None, None));
    };
    let goals: Vec<_> = store
        .goals()?
        .into_iter()
        .filter(|g| g.status == GoalStatus::Active)
        .collect();
    let goal = goals
        .iter()
        .find(|g| g.title.trim().eq_ignore_ascii_case(LAB_GOAL))
        .or(goals.first());
    let Some(goal) = goal else {
        return Ok((seated, None, None));
    };
    let reviewed = store
        .routines()?
        .iter()
        .any(|r| r.assignee == ceo.id && r.title != "Planning");
    let review = if reviewed {
        None
    } else {
        Some(hq.add_routine(
            endora_team::application::Author::Endora,
            NewRoutine {
                title: "Daily review".to_owned(),
                description: DAILY_REVIEW.to_owned(),
                goal: goal.id,
                project: None,
                assignee: ceo.id,
                every_ms: 24 * 60 * 60 * 1000,
                on_lab_trouble: false,
            },
        )?)
    };
    let planning = hq.seed_planning(goal.id)?;
    Ok((seated, review, planning))
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
    let settings = Settings {
        offset_minutes: owners_offset(|name| std::env::var(name).ok(), now_ms())?,
        // Downtime (ADR 0119): a member with nothing to do tidies the office or its notebook,
        // free on the house's model. The scripted demo has none.
        downtime: !demo_mode,
        // Ping-pong and pool between free members (ADR 0121): played in code, free.
        games: !demo_mode,
        // The nightly quiz on the map (ADR 0126): a free member, in a downtime turn on the
        // house's model, scored by code. The scripted demo has none.
        quiz: !demo_mode,
        // The caps on a runaway (ADR 0133) are on for a real team from the start. The scripted
        // demo takes a turn as fast as its tick lets it, costs nothing, and is exempt.
        scripted: demo_mode,
        ..Settings::default()
    };
    let hq: Arc<Hq> = Arc::new(
        Engine::new(
            SqliteTeamStore::open(&db)?,
            runner,
            api::SystemClock,
            settings,
        )
        .with_docs(public_docs::PUBLIC_DOCS),
    );
    // Before seeding work, publishing HTTP or starting ticks: no v1 peer work
    // may execute in this process. Failure aborts startup; the next startup
    // repeats the bounded cleanup and settles only still-live legacy turns.
    hq.retire_legacy_peer_work()?;
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
        match seed_routine(&hq) {
            Ok(None) => {}
            Ok(Some(id)) => {
                println!("hq: made the home lab's routine ({id}): the lead checks it every 6 hours")
            }
            Err(e) => eprintln!("hq: could not make the home lab's routine: {e}"),
        }
        match seed_ceo(&hq) {
            Ok((seated, routine, planning)) => {
                if let Some(id) = seated {
                    println!("hq: seated the team's CEO, {CEO_NAME} ({id}), on Claude Opus");
                }
                if let Some(id) = routine {
                    println!("hq: made the CEO's daily review ({id})");
                }
                if let Some(id) = planning {
                    println!("hq: made the CEO's weekly planning ({id})");
                }
            }
            Err(e) => eprintln!("hq: could not seat the CEO: {e}"),
        }
        // Whom each member reports to, and the home lab's work as a project, for a store made
        // before either (ADR 0113): once, and nothing after.
        match hq.backfill_reports_to() {
            Ok(0) => {}
            Ok(n) => println!("hq: set whom {n} members report to"),
            Err(e) => eprintln!("hq: could not set whom members report to: {e}"),
        }
        match hq.migrate_projects() {
            Ok(None) => {}
            Ok(Some(id)) => {
                println!("hq: put the home lab's routines and open work in project {id}")
            }
            Err(e) => eprintln!("hq: could not make the home lab's project: {e}"),
        }
        // How this lab is built, in the house log, once and only when it is empty (ADR 0112).
        match hq.seed_house_log() {
            Ok(true) => println!("hq: put how this lab is built in the house log"),
            Ok(false) => {}
            Err(e) => eprintln!("hq: could not seed the house log: {e}"),
        }
        // The team's world (ADR 0122), once: the house log's lines into the house page's
        // timeline, the lessons into stories to retell. Nothing old is deleted.
        match hq.migrate_world() {
            Ok(true) => println!("hq: moved the house log and the lessons into the team's world"),
            Ok(false) => {}
            Err(e) => eprintln!("hq: could not move the house log into the team's world: {e}"),
        }
        // The team's map (ADR 0126): a page for every area, project, goal and role, made once
        // each from the team as it stands. Nothing is changed or deleted.
        match hq.migrate_map() {
            Ok(made) if made.total() == 0 => {}
            Ok(made) => println!(
                "hq: made the map's pages: {} areas, {} projects, {} goals, {} roles",
                made.areas, made.projects, made.goals, made.roles
            ),
            Err(e) => eprintln!("hq: could not make the map's pages: {e}"),
        }
        // What the move dated wrongly, mended once (ADR 0122).
        match hq.repair_world_dates() {
            Ok(0) => {}
            Ok(n) => println!("hq: mended the dates of {n} lines of the house page's timeline"),
            Err(e) => eprintln!("hq: could not mend the house page's dates: {e}"),
        }
        // The office's decorations as data (ADR 0119), once: the team moves them from there.
        match hq.seed_office() {
            Ok(true) => println!("hq: put the office's decorations on the floor"),
            Ok(false) => {}
            Err(e) => eprintln!("hq: could not seed the office: {e}"),
        }
        // The engineering wing (ADR 0115), once: its goal, project, lead, developers and first task.
        match hq.seed_wing() {
            Ok(Some(wing)) => println!(
                "hq: seeded the engineering wing: goal {}, project {}, lead {} with {} developers, task {}",
                wing.goal,
                wing.project,
                wing.lead,
                wing.developers.len(),
                wing.task
            ),
            Ok(None) => {}
            Err(e) => eprintln!("hq: could not seed the wing: {e}"),
        }
        // Each lead's "State of my area" (daily, at the owner's 18:00) and "Patterns" (weekly),
        // once per lead that leads a project (ADR 0120).
        match hq.seed_area_routines() {
            Ok(made) if made.is_empty() => {}
            Ok(made) => println!("hq: made {} of the leads' report routines", made.len()),
            Err(e) => eprintln!("hq: could not make the leads' report routines: {e}"),
        }
        // Routines from before their origin was kept, whose words are exactly HQ's own: their
        // tasks are HQ's words, not unknown ones (ADR 0138 A2b).
        match hq.vouch_template_routines(&[
            ("Check the home lab", LAB_CHECK),
            ("Daily review", DAILY_REVIEW),
        ]) {
            Ok(0) => {}
            Ok(n) => println!("hq: {n} routines are HQ's own words"),
            Err(e) => eprintln!("hq: could not vouch for HQ's routines: {e}"),
        }
        // The Claude Code pilot (ADR 0117), once: one wing developer on it, the other on the
        // worker, for the scorecard to compare.
        match hq.seed_claude_code_pilot() {
            Ok(Some(member)) => println!("hq: member {} runs on Claude Code (the pilot)", member.0),
            Ok(None) => {}
            Err(e) => eprintln!("hq: could not seed the Claude Code pilot: {e}"),
        }
    }

    // The tick: every `tick_every` seconds; a late one runs once. When each one finished is
    // kept for `/health` (ADR 0124).
    let ticking = hq.clone();
    let last_tick: Arc<std::sync::atomic::AtomicI64> = Arc::default();
    let ticked = last_tick.clone();
    let mut ticks: u64 = 0;
    tokio::spawn(async move {
        let mut ticker = tokio::time::interval(std::time::Duration::from_secs(tick_every));
        ticker.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            ticker.tick().await;
            let hq = ticking.clone();
            let ticked = ticked.clone();
            ticks += 1;
            let prune = ticks % 100 == 0;
            let _ = tokio::task::spawn_blocking(move || {
                // The scripted demo has no node to say whether the owner's Mac is free (ADR
                // 0131): its Mac is always free, so a tester's review never waits on one.
                if demo_mode {
                    hq.tell_mac(true, "the demo's Mac is always free");
                }
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
        last_tick,
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
    use super::{owners_offset, the_runner_token, the_token};

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
