use super::*;

const MIN: Millis = 60_000;
const HOUR: Millis = 60 * MIN;
const DAY: Millis = 24 * HOUR;

fn run(id: i64, who: &str, at: Millis, status: &str) -> RunSeen {
    RunSeen {
        id,
        who: who.into(),
        why: "assigned".into(),
        status: status.into(),
        started: Some(at),
        ended: Some(at + MIN),
        end_reason: "ended its turn".into(),
        receipts: 0,
        moved_forward: true,
    }
}

/// Task 672's shape, as HQ recorded it: a failed check, a decision, a stall the budget caused,
/// a retry, a pass, a review and an accepted close.
fn worked() -> Seen {
    let t0 = 1_000 * DAY;
    Seen {
        now: t0 + 9 * DAY,
        status: "done".into(),
        assignee: Some("Zed".into()),
        closure: Some(ClosureSeen {
            why: "met_criteria".into(),
            said: "it met its criteria".into(),
            at: t0 + 8 * DAY,
        }),
        runs: vec![
            run(1, "Zed", t0, "succeeded"),
            run(2, "Dee", t0 + 10 * MIN, "succeeded"),
            RunSeen {
                end_reason: "its container is gone".into(),
                moved_forward: false,
                ..run(3, "Dee", t0 + 20 * MIN, "failed")
            },
            RunSeen {
                moved_forward: false,
                ..run(4, "Dee", t0 + 70 * MIN, "succeeded")
            },
            run(5, "Zed", t0 + 7 * DAY + 60 * MIN, "succeeded"),
            run(6, "Kit", t0 + 8 * DAY - 2 * MIN, "succeeded"),
        ],
        refused: vec![
            RefusedSeen {
                at: t0 + 12 * MIN,
                who: "Zed".into(),
                reason: "check_back".into(),
                refused: "in_review".into(),
            },
            RefusedSeen {
                at: t0 + 40 * MIN,
                who: "Mara".into(),
                reason: "decide".into(),
                refused: "out_of_budget".into(),
            },
            RefusedSeen {
                at: t0 + 90 * MIN,
                who: "Kit".into(),
                reason: "decide".into(),
                refused: "out_of_budget".into(),
            },
        ],
        branches: vec![BranchSeen {
            name: "hq/task-672-role-readmes".into(),
            files: 7,
            plus: 88,
            minus: 0,
            at: t0 + 8 * MIN,
            run: 1,
            who: "Zed".into(),
        }],
        checks: vec![
            CheckSeen {
                id: 2,
                ok: Some(false),
                summary: "syntax failed".into(),
                at: t0 + 9 * MIN,
                run: 1,
            },
            CheckSeen {
                id: 13,
                ok: Some(true),
                summary: "no worse than develop".into(),
                at: t0 + 7 * DAY + 61 * MIN,
                run: 5,
            },
        ],
        desk: vec![
            DeskSeen {
                id: 257,
                kind: "decision".into(),
                text: "Will you install the tools?".into(),
                status: "approve".into(),
                at: t0 + 30 * MIN,
                from: Some("Mara".into()),
            },
            DeskSeen {
                id: 288,
                kind: "stuck".into(),
                text: "Stuck: nobody came back".into(),
                status: "retry".into(),
                at: t0 + 7 * DAY,
                from: None,
            },
        ],
        handovers: vec![HandoverSeen {
            from: Some("Zed".into()),
            to: Some("Dee".into()),
            at: t0 + 8 * DAY - 5 * MIN,
        }],
        last_signal: None,
    }
}

#[test]
fn a_closed_item_that_met_its_criteria_is_completed_and_verified() {
    let map = live_map(&worked());
    assert_eq!(map.state, State::Completed);
    assert_eq!(map.said, "Done, verified against its criteria");
}

#[test]
fn a_closed_item_without_verification_says_done_unverified_never_completed() {
    let mut seen = worked();
    seen.closure = Some(ClosureSeen {
        why: "completed_unverified".into(),
        said: String::new(),
        at: 0,
    });
    let map = live_map(&seen);
    assert_eq!(map.state, State::DoneUnverified);
    assert!(map.said.contains("not verified"), "{}", map.said);
}

#[test]
fn a_dropped_item_is_closed_and_says_why() {
    let mut seen = worked();
    seen.status = "dropped".into();
    seen.closure = Some(ClosureSeen {
        why: "duplicate".into(),
        said: "it duplicated #3".into(),
        at: 0,
    });
    let map = live_map(&seen);
    assert_eq!(map.state, State::Closed);
    assert!(map.said.contains("it duplicated #3"), "{}", map.said);
}

#[test]
fn a_decision_waiting_on_the_owner_comes_first_and_makes_the_item_wait() {
    let mut seen = worked();
    seen.status = "doing".into();
    seen.closure = None;
    seen.desk[0].status = "waiting".into();
    let map = live_map(&seen);
    assert_eq!(map.state, State::Waiting);
    assert_eq!(map.attention.len(), 1);
    assert_eq!(map.attention[0].id, 257);
}

#[test]
fn nothing_waits_on_the_owner_when_every_card_is_decided() {
    assert!(live_map(&worked()).attention.is_empty());
}

#[test]
fn a_running_turn_with_a_fresh_event_is_working() {
    let mut seen = worked();
    seen.status = "doing".into();
    seen.closure = None;
    seen.runs.push(RunSeen {
        ended: None,
        ..run(9, "Zed", seen.now - 2 * MIN, "running")
    });
    assert_eq!(live_map(&seen).state, State::Working);
}

#[test]
fn a_running_turn_silent_past_the_window_is_unknown_and_says_since_when() {
    let mut seen = worked();
    seen.status = "doing".into();
    seen.closure = None;
    let since = seen.now - 3 * HOUR;
    seen.runs.push(RunSeen {
        ended: None,
        ..run(9, "Zed", since, "running")
    });
    let map = live_map(&seen);
    assert_eq!(map.state, State::Unknown);
    assert_eq!(map.silent_since, Some(since));
}

#[test]
fn an_open_item_whose_last_check_failed_is_blocked() {
    let mut seen = worked();
    seen.status = "doing".into();
    seen.closure = None;
    seen.checks.pop();
    seen.desk.clear();
    assert_eq!(live_map(&seen).state, State::Blocked);
}

#[test]
fn a_queued_turn_makes_an_open_item_wait() {
    let mut seen = worked();
    seen.status = "doing".into();
    seen.closure = None;
    seen.runs.push(RunSeen {
        started: None,
        ended: None,
        ..run(9, "Zed", seen.now, "queued")
    });
    assert_eq!(live_map(&seen).state, State::Waiting);
}

#[test]
fn the_steps_follow_the_record_in_time_order() {
    let map = live_map(&worked());
    let kinds: Vec<&str> = map.steps.iter().map(|s| s.kind).collect();
    assert_eq!(
        kinds,
        [
            "assigned",
            "submitted",
            "check",
            "desk",
            "stall",
            "desk",
            "check",
            "handover",
            "closed"
        ]
    );
    assert!(map.steps.windows(2).all(|w| w[0].at <= w[1].at));
}

#[test]
fn a_failed_check_is_a_red_step_with_its_evidence() {
    let map = live_map(&worked());
    let failed = map
        .steps
        .iter()
        .find(|s| s.kind == "check" && s.tone == Tone::Bad)
        .unwrap();
    assert_eq!(failed.label, "Check c2 failed");
    assert!(failed.ids.contains(&"c2".to_owned()));
}

#[test]
fn a_long_gap_with_refused_wakes_is_one_stall_that_counts_each_reason() {
    let map = live_map(&worked());
    let stall = map.steps.iter().find(|s| s.kind == "stall").unwrap();
    assert_eq!(stall.tone, Tone::Warn);
    assert!(stall.label.starts_with("7 days stalled"), "{}", stall.label);
    assert!(stall.label.contains("out of budget"), "{}", stall.label);
    assert!(stall.sub.contains("2 out of budget"), "{}", stall.sub);
    // The refusal before the gap is said too, never folded into the budget's count.
    assert!(stall.sub.contains("1 in review"), "{}", stall.sub);
}

#[test]
fn a_long_gap_with_no_refusals_is_no_stall() {
    let mut seen = worked();
    seen.refused.clear();
    assert!(live_map(&seen).steps.iter().all(|s| s.kind != "stall"));
}

#[test]
fn a_decided_card_is_an_owner_step_that_says_how_it_was_decided() {
    let map = live_map(&worked());
    let card = map.steps.iter().find(|s| s.kind == "desk").unwrap();
    assert_eq!(card.tone, Tone::Owner);
    assert_eq!(card.owner.as_deref(), Some("Approved"));
    assert!(card.ids.contains(&"#257".to_owned()));
    // Who put it to the owner is the handoff.
    assert_eq!(card.label, "Decision from Mara");
}

#[test]
fn a_card_the_engine_raised_on_its_own_is_not_drawn_as_an_owner_decision() {
    let map = live_map(&worked());
    let stuck = map
        .steps
        .iter()
        .filter(|s| s.kind == "desk")
        .nth(1)
        .unwrap();
    assert_eq!(stuck.label, "Flagged stuck");
    assert_eq!(stuck.owner.as_deref(), Some("Chose retry"));
}

#[test]
fn the_owners_word_that_it_is_deployed_is_never_drawn_as_verified() {
    let mut seen = worked();
    seen.desk.push(DeskSeen {
        id: 339,
        kind: "deploy".into(),
        text: "say Deployed".into(),
        status: "deployed".into(),
        at: seen.now - DAY,
        from: None,
    });
    let map = live_map(&seen);
    let deployed = map
        .steps
        .iter()
        .find(|s| s.ids.contains(&"#339".to_owned()))
        .unwrap();
    assert_ne!(deployed.tone, Tone::Ok);
    assert!(
        deployed.label.contains("owner's word"),
        "{}",
        deployed.label
    );
}

#[test]
fn an_inconclusive_check_after_a_deploy_stays_open() {
    let mut seen = worked();
    seen.desk.push(DeskSeen {
        id: 4808,
        kind: "deploy_check".into(),
        text: "Post-deploy infrastructure check: Inconclusive. It ended without a verdict.".into(),
        status: "seen".into(),
        at: seen.now,
        from: None,
    });
    let map = live_map(&seen);
    let last = map.steps.last().unwrap();
    assert_eq!(last.tone, Tone::Open);
    assert!(last.label.contains("inconclusive"), "{}", last.label);
    assert_eq!(map.open.len(), 1);
}

#[test]
fn people_are_only_those_who_took_a_turn_with_their_count() {
    let map = live_map(&worked());
    let names: Vec<(&str, usize, usize)> = map
        .people
        .iter()
        .map(|p| (p.name.as_str(), p.turns, p.failed))
        .collect();
    assert_eq!(names, [("Zed", 2, 0), ("Dee", 3, 1), ("Kit", 1, 0)]);
}

#[test]
fn the_gap_before_a_step_reads_as_time() {
    assert_eq!(gap(8 * MIN), "+8 min");
    assert_eq!(gap(30_000), "<1 min");
    assert_eq!(gap(3 * HOUR + 9 * MIN), "+3 h");
    assert_eq!(gap(7 * DAY + 13 * HOUR), "+7 d");
}
