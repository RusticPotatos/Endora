//! A stable prompt opening (ADR 0142, amended 2026-10-31, after Ralph: the same PROMPT.md every
//! pass, what changes at its end). Two work turns of different members, on different items, on
//! different days, built from the engine's real briefings and HQ's and the node's real tools:
//! the bytes a provider's prompt cache reads before the first part that varies are the same in
//! both, and Claude's cache breakpoints sit at the end of that fixed opening.
//!
//! All data is fictional.

use std::sync::atomic::{AtomicI64, Ordering};

use serde_json::{Value, json};

use super::*;

use endora_team::application::{Author, Clock, Engine, NewTask, RunContext, Settings};
use endora_team::domain::money::DOLLAR;
use endora_team::domain::{MemberId, Priority, ProjectId, Role};
use endora_team::infrastructure::{SimulatedRunner, SqliteTeamStore};

struct Hands(AtomicI64);
impl Clock for &Hands {
    fn now(&self) -> i64 {
        self.0.load(Ordering::SeqCst)
    }
}

type Hq<'a> = Engine<SqliteTeamStore, SimulatedRunner, &'a Hands>;

const NOON: i64 = 1_792_166_400_000;
const HOUR: i64 = 3_600_000;

/// A tool bridged from one of the lab's services, as the node lists one (ADR 0108).
fn bridged(i: usize) -> Value {
    json!({ "name": format!("service_{i}_status"), "kind": "read",
            "description": format!("Read service {i}'s state, and what failed in it with why."),
            "inputSchema": { "type": "object", "properties": {
                "why": { "type": "string", "description": "Why, in a few words." } } } })
}

/// The first call of a member's running turn as the worker builds it for `api`: HQ's tools for
/// the turn and the node's contracts with ten bridged tools, and the briefing with the claim
/// the worker makes for it.
fn first_call(hq: &Hq<'_>, context: &RunContext, api: Api) -> (Talk, String) {
    let (run, member) = hq.turn_of(&context.token).unwrap().expect("a running turn");
    let mut offered: Vec<Value> = endora_team::application::protocol::tools_in(hq, &run, &member)
        .as_array()
        .cloned()
        .unwrap();
    let role = member.role.name();
    let contracts: Vec<Value> = endora_capabilities::contracts::all()
        .iter()
        .filter(|c| c.offered_to(role, false) && !is_repo_tool(c.name))
        .map(endora_capabilities::contracts::Contract::as_tool)
        .chain((0..10).map(bridged))
        .collect();
    let on_the_node: Vec<String> = contracts
        .iter()
        .filter_map(|c| c["name"].as_str().map(str::to_owned))
        .collect();
    offered.extend(contracts);
    let lessons = offered.iter().any(|t| t["name"] == "tell_story");
    let call = |tool: &str, args: &Value| {
        endora_team::application::protocol::call(hq, &run, &member, tool, args).unwrap()
    };
    let briefing = call("my_turn", &json!({}));
    let task = task_of(&briefing).expect("a work turn");
    let said = call("claim", &json!({ "task": task }));
    let briefing = format!(
        "{}\n\nBefore your turn began, your task was claimed for you: {said}",
        briefing.trim_end()
    );
    let Opening { talk, .. } = opening(
        api,
        offered,
        briefing.clone(),
        &on_the_node,
        true,
        lessons,
        false,
    );
    (talk, briefing)
}

/// The text of a part of a call: a string as it is, a list of blocks as their texts in order.
fn text_of(part: &Value) -> String {
    if let Some(text) = part.as_str() {
        return text.to_owned();
    }
    part.as_array()
        .into_iter()
        .flatten()
        .map(|block| match block["text"].as_str() {
            Some(text) => text.to_owned(),
            None => block.to_string(),
        })
        .collect()
}

/// The prompt as a provider's cache reads it, in the order it renders it. Claude: its tools,
/// its system, then its messages (Anthropic's render order). A house model: its system text, its
/// tools, then its messages (the chat templates of llama.cpp's models, and DeepSeek's, put the
/// tools in the system turn after its text).
fn as_cached(talk: &Talk) -> String {
    let body = talk.body("model");
    let tools: String = body["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(Value::to_string)
        .collect();
    let messages = body["messages"].as_array().unwrap();
    match talk.api {
        Api::Anthropic => {
            let rest: String = messages.iter().map(|m| text_of(&m["content"])).collect();
            format!("{tools}{}{rest}", text_of(&body["system"]))
        }
        Api::OpenAi => {
            let rest: String = messages[1..]
                .iter()
                .map(|m| text_of(&m["content"]))
                .collect();
            format!("{}{tools}{rest}", text_of(&messages[0]["content"]))
        }
    }
}

/// How many bytes two texts share from their start.
fn shared(a: &str, b: &str) -> usize {
    a.bytes().zip(b.bytes()).take_while(|(x, y)| x == y).count()
}

/// A loop at `rank` with one item, and the member at that rank's turn on it.
fn turn_on_an_item(
    hq: &Hq<'_>,
    goal: endora_team::domain::GoalId,
    lead: MemberId,
    (title, rank): (&str, u8),
    item: (&str, &str),
    member: MemberId,
) -> (ProjectId, RunContext) {
    let project = hq.add_project(goal, title, "its work", lead, None).unwrap();
    hq.set_loop_rank(project, rank).unwrap();
    hq.add_task(
        Author::Owner,
        NewTask {
            goal,
            project: Some(project),
            parent: None,
            title: item.0.to_owned(),
            description: item.1.to_owned(),
            assignee: None,
            priority: Priority::Normal,
            waits_on: Vec::new(),
        },
    )
    .unwrap();
    hq.tick().unwrap();
    let context = hq
        .runner()
        .started()
        .into_iter()
        .rfind(|c| c.member == member && c.task.is_some())
        .expect("a turn on the item");
    (project, context)
}

/// Given Bo's turn on one loop's item, and a day and two hours later Rae's on another loop's,
/// when the worker builds each turn's first call for the house's models and for Claude,
/// then everything before the briefing's first line about the member is the same bytes in both:
/// the instructions, the tools and the briefing's fixed opening; and Claude's cache breakpoints
/// end the system and that opening, so a turn reads them from the cache another turn wrote.
#[test]
fn two_turns_share_their_prompts_opening() {
    let clock = Hands(AtomicI64::new(NOON));
    let hq = Engine::new(
        SqliteTeamStore::in_memory().unwrap(),
        SimulatedRunner::new(8),
        &clock,
        Settings {
            pool_per_month: 30 * DOLLAR,
            ..Settings::default()
        },
    );
    let goal = hq
        .set_goal("Keep the lab healthy", "you asked", Author::Owner)
        .unwrap();
    let seat = |name: &str, model: &str, role: Role| {
        let id = hq.hire(name, role, 10, 2, 0).unwrap();
        hq.set_model(id, model).unwrap();
        id
    };
    let mara = seat("Mara", "opus", Role::Ceo);
    let rae = seat("Rae", "mac", Role::Developer);
    let bo = seat("Bo", "local", Role::Developer);
    let (_, bos) = turn_on_an_item(
        &hq,
        goal,
        mara,
        ("ansible-nas", 6),
        (
            "Find what stops gluetun",
            "gluetun stopped twice this week: find why and fix it in roles/gluetun.",
        ),
        bo,
    );
    let (bo_house, bo_briefing) = first_call(&hq, &bos, Api::OpenAi);
    let (bo_claude, _) = first_call(&hq, &bos, Api::Anthropic);
    clock.0.fetch_add(26 * HOUR, Ordering::SeqCst);
    hq.tell_mac(true, "ready");
    let (_, raes) = turn_on_an_item(
        &hq,
        goal,
        mara,
        ("Backups", 5),
        (
            "Pin the restic image",
            "Pin restic's image tag in its role's defaults.",
        ),
        rae,
    );
    let (rae_house, rae_briefing) = first_call(&hq, &raes, Api::OpenAi);
    let (rae_claude, _) = first_call(&hq, &raes, Api::Anthropic);
    assert_ne!(bo_briefing, rae_briefing);

    // The briefing opens with what every work turn is told alike, before who the member is.
    let identity = |briefing: &str| {
        briefing
            .find("You are ")
            .expect("the briefing says who the member is")
    };
    let opening_bytes = identity(&bo_briefing);
    assert_eq!(
        bo_briefing[..opening_bytes],
        rae_briefing[..identity(&rae_briefing)],
        "the briefing's opening is the same for every work turn"
    );

    // Measured before this change (round3-local at 79a24b06), the same way: 9,760 bytes shared
    // on a house model and 18,898 on Claude, the briefing differing from its first line, and no
    // breakpoint of Claude's where they ended, so no turn read another's from the cache. After:
    // 10,466 and 19,604, with breakpoints at the system's end and the opening's. Since D30
    // (round 4) a turn on its own item is not offered `my_turn` or `claim`: 424 bytes fewer of
    // tools, 9,984 and 19,180, still shared up to who the member is.
    for (kind, a, b, floor) in [
        ("house", &bo_house, &rae_house, 9_900),
        ("Claude", &bo_claude, &rae_claude, 19_100),
    ] {
        let (a, b) = (as_cached(a), as_cached(b));
        let same = shared(&a, &b);
        let first_varying = identity(&a);
        let varies: String = a[same..].chars().take(60).collect();
        eprintln!(
            "{kind}: {same} bytes shared before the first part that varies, of {} and {} \
             (~{} tokens); it varies at {varies:?}",
            a.len(),
            b.len(),
            same / 4
        );
        assert!(
            same >= first_varying,
            "{kind}: everything before who the member is is shared ({same} of {first_varying})"
        );
        assert!(same >= floor, "{kind}: {same} bytes shared");
    }
    eprintln!("the briefing's fixed opening: {opening_bytes} bytes");
    assert!(
        opening_bytes >= 300,
        "the briefing opens with its fixed part: {opening_bytes} bytes"
    );

    // Claude: a breakpoint ends the system (the tools and instructions) and one the briefing's
    // fixed opening; the top-level mark still caches the conversation as it grows.
    for talk in [&bo_claude, &rae_claude] {
        let body = talk.body("model");
        let ephemeral = json!({ "type": "ephemeral" });
        let system = body["system"].as_array().expect("the system as blocks");
        assert_eq!(system.last().unwrap()["cache_control"], ephemeral);
        let first = body["messages"][0]["content"]
            .as_array()
            .expect("the briefing as blocks");
        assert_eq!(first.len(), 2, "its fixed opening, then the rest");
        assert_eq!(first[0]["cache_control"], ephemeral);
        assert!(first[1]["text"].as_str().unwrap().starts_with("You are "));
        assert!(first[1].get("cache_control").is_none());
        assert_eq!(body["cache_control"], ephemeral);
    }
    let up_to_breakpoints = |talk: &Talk| {
        let body = talk.body("model");
        (
            format!("{}{}", body["tools"], body["system"]),
            body["messages"][0]["content"][0].to_string(),
        )
    };
    assert_eq!(
        up_to_breakpoints(&bo_claude),
        up_to_breakpoints(&rae_claude),
        "what each breakpoint caches is the same bytes in both turns"
    );
}
