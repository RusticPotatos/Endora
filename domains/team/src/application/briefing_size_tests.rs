//! The size of a work turn's and a review turn's briefing (ADR 0142, amended 2026-10-30), over
//! HQ's real SQLite store and the simulated runner, on a team whose world is full: house
//! sections on many services, map lines, other projects, a loop's earlier items, a long thread
//! and a branch with a diff. All data is fictional.
//!
//! Bytes are the briefing's, as the worker sends it as the first message; tokens are about four
//! bytes each.

use std::sync::atomic::{AtomicI64, Ordering};

use super::engine::{Author, Called, Engine, Settings, Submission};
use super::ports::{Clock, NewTask, RunContext, TeamStore};
use crate::domain::money::DOLLAR;
use crate::domain::{
    Criterion, GoalId, MemberId, Priority, ProjectId, Role, TaskId, TaskStatus, WakeReason,
};
use crate::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

const NOON: i64 = 1_792_166_400_000;
const MINUTE: i64 = 60_000;

fn hq(clock: &Hands) -> Hq<'_> {
    Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(8),
        clock,
        Settings {
            pool_per_month: 30 * DOLLAR,
            ..Settings::default()
        },
    )
}

struct Team {
    bo: MemberId,
    rae: MemberId,
    loop_: ProjectId,
    goal: GoalId,
}

fn team(hq: &Hq<'_>) -> Team {
    let goal = hq
        .set_goal("Keep the lab healthy", "you asked", Author::Owner)
        .unwrap();
    let seat = |name: &str, model: &str, role: Role| {
        let id = hq.hire(name, role, 10, 2, 0).unwrap();
        hq.set_model(id, model).unwrap();
        id
    };
    let mara = seat("Mara", "opus", Role::Ceo);
    seat("Kit", "sonnet", Role::Lead);
    seat("Ada", "haiku", Role::Lead);
    let rae = seat("Rae", "mac", Role::Developer);
    let bo = seat("Bo", "local", Role::Developer);
    let loop_ = hq
        .add_project(goal, "ansible-nas", "the fork's work", mara, None)
        .unwrap();
    // Other projects, each with a page of its own.
    for (title, about) in [
        ("Media server", "films and series for the house"),
        ("Backups", "every volume copied offsite nightly"),
        ("Home automation", "lights, heating and sensors"),
    ] {
        hq.add_project(goal, title, about, mara, None).unwrap();
    }
    Team {
        bo,
        rae,
        loop_,
        goal,
    }
}

fn item(hq: &Hq<'_>, t: &Team, title: &str, description: &str) -> TaskId {
    hq.add_task(
        Author::Owner,
        NewTask {
            goal: t.goal,
            project: Some(t.loop_),
            parent: None,
            title: title.to_owned(),
            description: description.to_owned(),
            assignee: None,
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap()
}

fn turn_of(hq: &Hq<'_>, member: MemberId) -> RunContext {
    hq.tick().unwrap();
    hq.runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member)
        .expect("a turn of the member's")
}

fn use_tool(hq: &Hq<'_>, context: &RunContext, tool: &str, args: &serde_json::Value) -> String {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    super::protocol::call(hq, &run, &member, tool, args).unwrap()
}

fn receipt(hq: &Hq<'_>, context: &RunContext, tool: &str, args: &str, excerpt: &str) -> i64 {
    hq.record_receipt(
        &context.token,
        &Called {
            tool: tool.to_owned(),
            args: args.to_owned(),
            excerpt: excerpt.to_owned(),
            ok: true,
            act: false,
            confidential: false,
        },
    )
    .unwrap()
}

/// What the house knows: a section on each of many services, the owner's.
const HOUSE: [(&str, &str, &str); 10] = [
    (
        "gluetun",
        "The VPN container",
        "gluetun is the VPN; qbittorrent and prowlarr run in its network and stop when it restarts.\nIts provider credentials live in the vault; a stale one shows AUTH_FAILED in its logs.",
    ),
    (
        "backups",
        "Backups",
        "restic copies every volume offsite at 02:00; a run takes about forty minutes.\nThe repository password is in the vault, never in a role's defaults.",
    ),
    (
        "media",
        "The media server",
        "jellyfin serves films and series on port 8096; its transcodes use the GPU.\nIts library is on the big pool, mounted read-only.",
    ),
    (
        "dns",
        "Names on the network",
        "pihole answers DNS for the house at 192.168.1.10; unbound resolves upstream.\nA blocklist update runs weekly on Sunday night.",
    ),
    (
        "proxy",
        "The reverse proxy",
        "traefik routes every web app by its host name and renews certificates itself.\nA new app needs its labels in the role's docker compose task.",
    ),
    (
        "monitoring",
        "Monitoring",
        "prometheus scrapes every exporter each minute; grafana shows the dashboards.\nAlerts go to the owner's phone through ntfy.",
    ),
    (
        "storage",
        "Disks and pools",
        "Two mirrored pools: fast for apps, big for media and backups.\nA scrub runs on the first Sunday of each month.",
    ),
    (
        "updates",
        "Updates",
        "watchtower is off: images are pinned in each role's defaults and moved on by hand.\nA bump is a branch like any other change.",
    ),
    (
        "automation",
        "Home automation",
        "home-assistant runs on the host network for discovery; mosquitto is its broker.\nZigbee devices pair through zigbee2mqtt.",
    ),
    (
        "photos",
        "Photos",
        "immich keeps the house's photos; its machine learning container is heavy and may be paused.\nIts database is postgres, backed up with the rest.",
    ),
];

/// Where things are, as members found them.
const MAP: [&str; 12] = [
    "gluetun's provider settings are in roles/gluetun/defaults/main.yml",
    "The backup schedule is in roles/restic/defaults/main.yml",
    "jellyfin's hardware transcoding is set in roles/jellyfin/templates/encoding.xml.j2",
    "pihole's blocklists are in roles/pihole/files/adlists.list",
    "traefik's labels for every app come from roles/traefik/templates/labels.j2",
    "prometheus's scrape targets are in roles/prometheus/templates/prometheus.yml.j2",
    "The pools' scrub timer is roles/storage/templates/scrub.timer.j2",
    "Pinned images are each role's <role>_image_tag in defaults/main.yml",
    "home-assistant's configuration is in roles/homeassistant/files/configuration.yaml",
    "immich's machine learning toggle is immich_ml_enabled in its defaults",
    "The lint config for the fork is .ansible-lint at the repository's root",
    "ntfy's topic for alerts is set in roles/ntfy/defaults/main.yml",
];

/// A diff of about 7 KB: the gluetun role's change and its README.
fn diff() -> String {
    let mut d = String::from(
        "diff --git a/roles/gluetun/defaults/main.yml b/roles/gluetun/defaults/main.yml\n\
         --- a/roles/gluetun/defaults/main.yml\n+++ b/roles/gluetun/defaults/main.yml\n\
         @@ -1,6 +1,8 @@\n gluetun_enabled: true\n-gluetun_provider: old\n+gluetun_provider: \
         example\n+gluetun_health_check: true\n",
    );
    for i in 0..60 {
        d.push_str(&format!(
            "+| `gluetun_option_{i}` | `value {i}` | what option {i} does for the tunnel |\n"
        ));
    }
    d
}

/// The world as a full house has it, and a loop with earlier items done.
fn full_world(hq: &Hq<'_>, t: &Team) {
    hq.migrate_map().unwrap();
    for (key, title, text) in HOUSE {
        hq.write_as_owner("house", key, title, text).unwrap();
    }
    hq.write_as_owner(
        &format!("project/{}", t.loop_.0),
        "branches",
        "How the fork takes changes",
        "Every change is a branch hq/task-N-<slug>, checked by ansible-lint and yamllint.\n\
         Nothing under group_vars, host_vars or the inventories is ever changed.",
    )
    .unwrap();
    hq.set_loop_items(t.loop_, 1).unwrap();
    for (n, title) in ["Fix the lint in roles/pihole", "Pin the jellyfin image"]
        .into_iter()
        .enumerate()
    {
        let earlier = item(hq, t, title, "");
        let bo = turn_of(hq, t.bo);
        use_tool(hq, &bo, "claim", &serde_json::json!({ "task": earlier.0 }));
        for line in MAP.iter().skip(n * 6).take(6) {
            use_tool(hq, &bo, "add_to_map", &serde_json::json!({ "line": line }));
        }
        for k in 0..4 {
            use_tool(
                hq,
                &bo,
                "post",
                &serde_json::json!({ "task": earlier.0, "text": format!(
                    "Step {k} on {title}: read the role, changed the task file, ran the lint again; \
                     it reports nothing new against develop and the container came back healthy."
                ) }),
            );
        }
        hq.runner().finish(bo.run, true);
        hq.store()
            .set_task_status(earlier, TaskStatus::Done)
            .unwrap();
    }
}

/// The hard item: what stops gluetun, with criteria, and owner's posts on it.
fn the_item(hq: &Hq<'_>, t: &Team) -> TaskId {
    let hard = item(
        hq,
        t,
        "Find what stops gluetun",
        "gluetun stopped twice this week and took qbittorrent with it. Find why, fix it in \
         roles/gluetun, and document the role's defaults in roles/gluetun/README.md.",
    );
    hq.set_criteria(
        hard,
        &[
            Criterion::new("gluetun is running and healthy", true),
            Criterion::new(
                "roles/gluetun/README.md lists every default with its value",
                false,
            ),
        ],
    )
    .unwrap();
    hq.post(
        Author::Owner,
        hard,
        "It stopped at night both times, around when the backups run.",
    )
    .unwrap();
    hard
}

/// Bo works the item: readings, a long thread, a branch, then asks for review.
fn bo_works(hq: &Hq<'_>, t: &Team, hard: TaskId, bo: &RunContext) {
    use_tool(hq, bo, "claim", &serde_json::json!({ "task": hard.0 }));
    let logs = receipt(
        hq,
        bo,
        "container_logs",
        "gluetun",
        "AUTH_FAILED from the provider; retrying in 10s",
    );
    for i in 0..14 {
        use_tool(
            hq,
            bo,
            "post",
            &serde_json::json!({ "task": hard.0, "text": format!(
                "Reading {i}: the provider's logs show the handshake failing after the restic \
                 run starts, then the health check gives up and the container exits; \
                 qbittorrent stops with it. Trying the next setting in the role's defaults."
            ) }),
        );
    }
    hq.record_branch(
        &bo.token,
        &Submission {
            branch: format!("hq/task-{}-find-what-stops-gluetun", hard.0),
            head: "a1b2c3d4e5f60718293a4b5c".to_owned(),
            base: "0123456789abcdef01234567".to_owned(),
            files: 2,
            plus: 62,
            minus: 1,
            stat: " roles/gluetun/README.md            | 60 ++++++\n \
                   roles/gluetun/defaults/main.yml    |  3 +-\n 2 files changed"
                .to_owned(),
            diff: diff(),
            can_push: false,
            changed: vec![
                "roles/gluetun/README.md".to_owned(),
                "roles/gluetun/defaults/main.yml".to_owned(),
            ],
            texts: Vec::new(),
        },
    )
    .unwrap();
    use_tool(
        hq,
        bo,
        "set_status",
        &serde_json::json!({ "task": hard.0, "status": "in_review",
            "reason": format!("Fixed: the provider is set and the health check is on. r{logs} shows `AUTH_FAILED from the provider` before."),
            "evidence": [[format!("r{logs}")], [format!("r{logs}")]] }),
    );
    hq.runner().finish(bo.run, true);
    let _ = t;
}

fn tokens(bytes: usize) -> usize {
    bytes.div_ceil(4)
}

/// Measured on this scenario before the cut (round2-local at 90e0cb7b): Bo's work turn on the
/// NAS's model and Rae's review on the Mac, in bytes.
const WORK_BEFORE: usize = 6_287;
const REVIEW_BEFORE: usize = 11_967;
/// The most each may be now.
const WORK_MOST: usize = 3_400;
const REVIEW_MOST: usize = 5_800;

/// The scenario: a full world, Bo's work turn on the item, then Rae's review of it.
fn work_then_review() -> (String, String, String) {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = hq(&clock);
    let t = team(&hq);
    full_world(&hq, &t);
    clock.0.fetch_add(10 * MINUTE, Ordering::SeqCst);
    let hard = the_item(&hq, &t);
    let bo = turn_of(&hq, t.bo);
    let work = bo.told.clone();
    bo_works(&hq, &t, hard, &bo);
    assert!(
        hq.wakes()
            .unwrap()
            .iter()
            .any(|w| w.member == t.rae && w.reason == WakeReason::ReviewRequested),
        "Rae is asked to review"
    );
    hq.tell_mac(true, "ready");
    let rae = turn_of(&hq, t.rae);
    let world_map = use_tool(
        &hq,
        &rae,
        "read_world",
        &serde_json::json!({ "address": "world:map" }),
    );
    (work, rae.told, world_map)
}

/// The owner, 2026-10-30: work turns read 110k to 213k prompt tokens, the briefing resent on
/// every call. A work turn's briefing carries only the world the item needs; a review turn's,
/// only what the review needs.
///
/// Given a full house, map, other projects, an earlier loop and an item with a long thread,
/// when Bo takes a turn on the item, and later Rae reviews it,
/// then each briefing is within its ceiling, well below what it was before.
#[test]
fn work_and_review_briefings_are_sized_to_what_the_turn_needs() {
    let (work, review, _) = work_then_review();
    println!(
        "work briefing: {} bytes (~{} tokens), before {WORK_BEFORE} (~{} tokens)\n{work}\n----",
        work.len(),
        tokens(work.len()),
        tokens(WORK_BEFORE)
    );
    println!(
        "review briefing: {} bytes (~{} tokens), before {REVIEW_BEFORE} (~{} tokens)\n{review}\n----",
        review.len(),
        tokens(review.len()),
        tokens(REVIEW_BEFORE)
    );
    assert!(
        work.len() <= WORK_MOST,
        "work briefing {} bytes",
        work.len()
    );
    assert!(
        review.len() <= REVIEW_MOST,
        "review briefing {} bytes",
        review.len()
    );
}

/// Cut 3: a task-relevant briefing. The world's lines that name what the item names stay; the
/// rest is a line saying which tool reads it.
#[test]
fn a_work_turn_is_told_the_world_its_item_names_and_where_the_rest_is() {
    let (work, _, world_map) = work_then_review();
    for kept in [
        // The house's section, the map's line and the lab's rule on gluetun.
        "gluetun is the VPN; qbittorrent and prowlarr run in its network",
        "gluetun's provider settings are in roles/gluetun/defaults/main.yml",
        "Containers that run in another's network (e.g. behind gluetun)",
        // The item's own project page and its section.
        "world:project/1#branches",
        // The item itself, whole.
        "roles/gluetun/README.md lists every default with its value",
        "It stopped at night both times",
    ] {
        assert!(work.contains(kept), "{kept} kept: {work}");
    }
    for dropped in [
        "jellyfin serves films",
        "pihole's blocklists",
        "Step 0 on Fix the lint",
        "world:project/2",
        "restic copies every volume",
        "The backup schedule is in",
    ] {
        assert!(!work.contains(dropped), "{dropped} left out: {work}");
    }
    for pointer in [
        "10 more lines on the map are about other things: read_world world:map.",
        "more sections, not about this item: read_world world:house.",
        "The loop's notes on its other items: ask_record with project 1.",
    ] {
        assert!(work.contains(pointer), "{pointer} pointed to: {work}");
    }
    // What the map left out is there to read, whole.
    for line in MAP {
        assert!(world_map.contains(line), "{line} in: {world_map}");
    }
}

/// Cut 4: a slimmer review. The reviewer gets the item's criteria, the branch (its stat, its
/// files and its diff, capped), HQ's mechanical checks and the worker's handover, and not the
/// thread's history or the world.
#[test]
fn a_review_turn_is_told_the_criteria_the_branch_the_checks_and_the_handover() {
    let (_, review, _) = work_then_review();
    for kept in [
        "1. gluetun is running and healthy",
        "2. roles/gluetun/README.md lists every default with its value",
        "Bo asks for your review.",
        "You review it one rank up",
        // The worker's handover: what it says it did, on what.
        "Fixed: the provider is set and the health check is on.",
        "r1 container_logs gluetun (ok): AUTH_FAILED from the provider",
        // HQ's checks, in full.
        crate::domain::mechanical::HEADER,
        "1 of 1 quoted lines are in their receipts",
        // The branch: its stat, its files, its diff's start, and where the rest is.
        "roles/gluetun/defaults/main.yml    |  3 +-",
        "Files it changes: roles/gluetun/README.md, roles/gluetun/defaults/main.yml",
        "+gluetun_provider: example",
        "read_document d1",
        "Check c1",
        // The owner's word on the item.
        "It stopped at night both times",
    ] {
        assert!(review.contains(kept), "{kept} kept: {review}");
    }
    assert!(
        !review.contains("gluetun_option_59"),
        "the diff is capped: {review}"
    );
    for dropped in [
        "Reading 3:",
        "Your map of the team's world",
        "The house page",
        "How the lab is built (world:lab):",
        "What this loop did lately",
        "The map, where things are",
    ] {
        assert!(!review.contains(dropped), "{dropped} left out: {review}");
    }
    for pointer in ["my_turn", "read_world"] {
        assert!(review.contains(pointer), "{pointer} pointed to: {review}");
    }
}
