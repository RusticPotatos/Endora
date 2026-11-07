//! The HQ agents map (the owner's decision M5a): the Core team, the open items each member holds
//! grouped by project, what runs now and what waits on the owner, read from HQ's own records.
//! Read-only. A line moves only for a turn that is running and was heard from recently; a
//! downtime turn is not work; nothing is drawn as activity that the record does not show.

use serde_json::{Value, json};

use super::work::Millis;

/// A running turn not heard from for this long stands still and says since when.
pub const FRESH_FOR: Millis = super::live_work::FRESH_FOR;

/// One member as the record shows it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MemberSeen {
    /// Its name.
    pub name: String,
    /// What it thinks with, as the owner reads it.
    pub brain: String,
    /// Paused by the owner.
    pub paused: bool,
    /// On furlough.
    pub furloughed: bool,
    /// Today's share spent.
    pub out_of_budget: bool,
    /// Its live turns and its latest ended ones.
    pub turns: Vec<TurnSeen>,
}

/// One turn.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TurnSeen {
    /// Its id.
    pub run: i64,
    /// The item it is on, if any.
    pub task: Option<i64>,
    /// `queued`, `running`, `retry_waiting`, or how it ended.
    pub status: String,
    /// When it started.
    pub started: Option<Millis>,
    /// When it ended.
    pub ended: Option<Millis>,
    /// A downtime turn: not work.
    pub downtime: bool,
    /// The latest sign of life while it runs: a call or a post.
    pub last_signal: Option<Millis>,
}

/// One open item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ItemSeen {
    /// Its id.
    pub task: i64,
    /// Its title.
    pub title: String,
    /// Its status name.
    pub status: String,
    /// Who holds it.
    pub holder: Option<String>,
    /// Its project's title.
    pub project: Option<String>,
}

/// One desk card waiting on the owner.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CardSeen {
    /// Its id.
    pub id: i64,
    /// Its kind.
    pub kind: String,
    /// The item it is about.
    pub task: Option<i64>,
    /// Its project's title.
    pub project: Option<String>,
    /// When it was raised.
    pub at: Millis,
}

/// What the map reads.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Seen {
    /// The time now.
    pub now: Millis,
    /// The members.
    pub members: Vec<MemberSeen>,
    /// The open items.
    pub items: Vec<ItemSeen>,
    /// The desk cards waiting on the owner.
    pub desk: Vec<CardSeen>,
}

/// A member's state now.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum MemberState {
    /// A turn runs and was heard from recently.
    Working,
    /// A turn runs, silent past [`FRESH_FOR`].
    Unknown,
    /// A turn waits to start or to retry.
    Queued,
    /// Nothing runs.
    Idle,
    /// Today's share is spent.
    OutForToday,
    /// Paused by the owner.
    Paused,
    /// On furlough.
    Furloughed,
}

impl MemberState {
    /// Stable lowercase name.
    #[must_use]
    pub const fn name(self) -> &'static str {
        match self {
            Self::Working => "working",
            Self::Unknown => "unknown",
            Self::Queued => "queued",
            Self::Idle => "idle",
            Self::OutForToday => "out_for_today",
            Self::Paused => "paused",
            Self::Furloughed => "furloughed",
        }
    }
}

/// A running turn's line from a member to its item.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Live {
    /// The turn.
    pub run: i64,
    /// The item.
    pub task: Option<i64>,
    /// When it was last heard from.
    pub heard: Millis,
    /// Whether the line moves: running and heard from within [`FRESH_FOR`].
    pub moving: bool,
}

/// An item on the map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Item {
    /// Its id.
    pub task: i64,
    /// Its title.
    pub title: String,
    /// Its status name.
    pub status: String,
    /// Whether a desk card about it waits on the owner.
    pub asks_owner: bool,
}

/// Items under one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Group {
    /// The project's title, or `no project`.
    pub project: String,
    /// Its items.
    pub items: Vec<Item>,
}

/// A member on the map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Member {
    /// Its name.
    pub name: String,
    /// What it thinks with.
    pub brain: String,
    /// Its state.
    pub state: MemberState,
    /// When any turn of its last ended.
    pub last_seen: Option<Millis>,
    /// When its last turn that was work ended, and on which item.
    pub last_work: Option<(Millis, Option<i64>)>,
    /// Its running turns.
    pub live: Vec<Live>,
    /// The items it holds, by project.
    pub groups: Vec<Group>,
}

/// Desk cards under one project.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct DeskGroup {
    /// The project's title, or `other`.
    pub project: String,
    /// How many cards.
    pub count: usize,
    /// How many of each kind.
    pub kinds: Vec<(String, usize)>,
}

/// The map.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TeamMap {
    /// Turns running now.
    pub running: usize,
    /// Turns waiting to start or retry.
    pub queued: usize,
    /// Open items blocked.
    pub blocked: usize,
    /// The headline in plain words.
    pub said: String,
    /// Cards waiting on the owner.
    pub desk_total: usize,
    /// The oldest of them.
    pub desk_oldest: Option<Millis>,
    /// Those cards by project.
    pub desk: Vec<DeskGroup>,
    /// The members, working first.
    pub members: Vec<Member>,
    /// Open items nobody holds.
    pub unheld: Vec<Item>,
}

/// The map from the record.
#[must_use]
pub fn team_map(seen: &Seen) -> TeamMap {
    let asks = |task: i64| seen.desk.iter().any(|d| d.task == Some(task));
    let shown = |i: &ItemSeen| Item {
        task: i.task,
        title: i.title.clone(),
        status: i.status.clone(),
        asks_owner: asks(i.task),
    };
    let mut members: Vec<Member> = seen
        .members
        .iter()
        .map(|m| member(m, seen, &shown))
        .collect();
    members.sort_by_key(rank);
    let running = members.iter().map(|m| m.live.len()).sum();
    let queued = seen
        .members
        .iter()
        .flat_map(|m| &m.turns)
        .filter(|t| matches!(t.status.as_str(), "queued" | "retry_waiting"))
        .count();
    let desk_total = seen.desk.len();
    let busy = match running {
        0 => "Nothing is running.".to_owned(),
        1 => "1 turn running.".to_owned(),
        n => format!("{n} turns running."),
    };
    let waits = match desk_total {
        0 => "Nothing waits on you.".to_owned(),
        1 => "1 card waits on you.".to_owned(),
        n => format!("{n} cards wait on you."),
    };
    TeamMap {
        running,
        queued,
        blocked: seen.items.iter().filter(|i| i.status == "blocked").count(),
        said: format!("{busy} {waits}"),
        desk_total,
        desk_oldest: seen.desk.iter().map(|d| d.at).min(),
        desk: desk_groups(&seen.desk),
        members,
        unheld: seen
            .items
            .iter()
            .filter(|i| i.holder.is_none())
            .map(shown)
            .collect(),
    }
}

fn member(m: &MemberSeen, seen: &Seen, shown: &dyn Fn(&ItemSeen) -> Item) -> Member {
    let live: Vec<Live> = m
        .turns
        .iter()
        .filter(|t| t.status == "running")
        .map(|t| {
            let heard = t
                .last_signal
                .into_iter()
                .chain(t.started)
                .max()
                .unwrap_or(seen.now);
            Live {
                run: t.run,
                task: t.task,
                heard,
                moving: seen.now - heard <= FRESH_FOR,
            }
        })
        .collect();
    let queued = m
        .turns
        .iter()
        .any(|t| matches!(t.status.as_str(), "queued" | "retry_waiting"));
    let state = if m.furloughed {
        MemberState::Furloughed
    } else if m.paused {
        MemberState::Paused
    } else if live.iter().any(|l| l.moving) {
        MemberState::Working
    } else if !live.is_empty() {
        MemberState::Unknown
    } else if queued {
        MemberState::Queued
    } else if m.out_of_budget {
        MemberState::OutForToday
    } else {
        MemberState::Idle
    };
    let mut groups: Vec<Group> = Vec::new();
    for i in seen
        .items
        .iter()
        .filter(|i| i.holder.as_deref() == Some(m.name.as_str()))
    {
        let project = i.project.clone().unwrap_or_else(|| "no project".into());
        match groups.iter_mut().find(|g| g.project == project) {
            Some(g) => g.items.push(shown(i)),
            None => groups.push(Group {
                project,
                items: vec![shown(i)],
            }),
        }
    }
    Member {
        name: m.name.clone(),
        brain: m.brain.clone(),
        state,
        last_seen: m.turns.iter().filter_map(|t| t.ended).max(),
        last_work: m
            .turns
            .iter()
            .filter(|t| !t.downtime)
            .filter_map(|t| t.ended.map(|e| (e, t.task)))
            .max_by_key(|(e, _)| *e),
        live,
        groups,
    }
}

/// Working first, then those holding items, then the idle, then the away.
fn rank(m: &Member) -> u8 {
    match m.state {
        MemberState::Working | MemberState::Unknown | MemberState::Queued => 0,
        MemberState::Idle if !m.groups.is_empty() => 1,
        MemberState::Idle => 2,
        MemberState::OutForToday | MemberState::Paused => 3,
        MemberState::Furloughed => 4,
    }
}

fn desk_groups(desk: &[CardSeen]) -> Vec<DeskGroup> {
    let mut groups: Vec<DeskGroup> = Vec::new();
    for d in desk {
        let project = d.project.clone().unwrap_or_else(|| "other".into());
        let at = groups
            .iter()
            .position(|g| g.project == project)
            .unwrap_or_else(|| {
                groups.push(DeskGroup {
                    project,
                    count: 0,
                    kinds: Vec::new(),
                });
                groups.len() - 1
            });
        let g = &mut groups[at];
        g.count += 1;
        match g.kinds.iter_mut().find(|(k, _)| *k == d.kind) {
            Some((_, n)) => *n += 1,
            None => g.kinds.push((d.kind.clone(), 1)),
        }
    }
    // The biggest first; "other" last.
    groups.sort_by_key(|g| (g.project == "other", std::cmp::Reverse(g.count)));
    groups
}

impl TeamMap {
    /// The map as the console reads it.
    #[must_use]
    pub fn shown(&self) -> Value {
        let item = |i: &Item| json!({ "task": i.task, "title": i.title, "status": i.status, "asks_owner": i.asks_owner });
        json!({
            "running": self.running, "queued": self.queued, "blocked": self.blocked, "said": self.said,
            "desk_total": self.desk_total, "desk_oldest": self.desk_oldest,
            "desk": self.desk.iter().map(|g| json!({ "project": g.project, "count": g.count,
                "kinds": g.kinds.iter().map(|(k, n)| json!({ "kind": k, "count": n })).collect::<Vec<_>>() }))
                .collect::<Vec<_>>(),
            "members": self.members.iter().map(|m| json!({
                "name": m.name, "brain": m.brain, "state": m.state.name(), "last_seen": m.last_seen,
                "last_work": m.last_work.map(|(at, task)| json!({ "at": at, "task": task })),
                "live": m.live.iter().map(|l| json!({ "run": l.run, "task": l.task, "heard": l.heard,
                    "moving": l.moving })).collect::<Vec<_>>(),
                "groups": m.groups.iter().map(|g| json!({ "project": g.project,
                    "items": g.items.iter().map(item).collect::<Vec<_>>() })).collect::<Vec<_>>(),
            })).collect::<Vec<_>>(),
            "unheld": self.unheld.iter().map(item).collect::<Vec<_>>(),
        })
    }
}

#[cfg(test)]
#[path = "team_map_tests.rs"]
mod tests;
