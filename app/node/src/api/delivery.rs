//! Accepted work reaches the owner's fork without a card (ADR 0115, amended 2026-10-31). Every
//! minute the node asks HQ what is due: branches accepted and passed, to push, and pushed
//! branches to merge into the fork's develop while the owner's setting has merging on. Before a
//! push, the branch's whole diff and its commit messages go through the privacy scan
//! (`endora_capabilities::outgoing`), which is code: the repository check's shapes, the kernel's
//! secret tripwire and the owner's own values from `ENDORA_PERSONAL_VALUES`. A scan that finds
//! anything pushes nothing, and HQ puts one card on the desk; a branch that cannot be read is not
//! pushed either, since the scan could not tell. Pushes and merges go through the same mirror,
//! push path and deploy key as before, never with force.

use serde_json::json;

use super::*;
use endora_capabilities::outgoing::{self, Found};

/// How often the node asks HQ what is due.
const EVERY_S: u64 = 60;

/// Asks HQ what is due, every minute, and does it. The owner's own values are read from the
/// node's environment once, at start.
pub(crate) fn spawn(state: AppState) {
    let own = outgoing::own_values(std::env::var("ENDORA_PERSONAL_VALUES").ok().as_deref());
    if own.is_empty() {
        eprintln!(
            "delivery: no ENDORA_PERSONAL_VALUES on the node: the privacy scan reads shapes and \
             the secret tripwire only"
        );
    }
    let own = Arc::new(own);
    tokio::spawn(async move {
        let mut interval = tokio::time::interval(std::time::Duration::from_secs(EVERY_S));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let (state, own) = (state.clone(), own.clone());
            let _ = tokio::task::spawn_blocking(move || deliver(&state, &own)).await;
        }
    });
}

fn found_json(found: &[Found]) -> serde_json::Value {
    found
        .iter()
        .map(|f| json!({ "what": f.what, "at": f.at }))
        .collect()
}

/// What the scan read from, said with each push so the record shows what was and was not
/// checked.
fn scanned_with(own: &[String]) -> String {
    if own.is_empty() {
        return "its privacy scan clean (shapes and the secret tripwire; no personal values \
                are set on the node)"
            .to_owned();
    }
    format!(
        "its privacy scan clean (shapes, the secret tripwire and {} of your own values)",
        own.len()
    )
}

/// One pass: what HQ says is due, done, each outcome told to HQ. Returns how many steps were
/// told.
pub(crate) fn deliver(state: &AppState, own: &[String]) -> usize {
    let Some(hq) = state.hq.as_ref() else {
        return 0;
    };
    let due = match hq.get("/v1/team/deliveries") {
        Ok(due) => due,
        Err(e) => {
            eprintln!("delivery: HQ was not asked what is due: {e}");
            return 0;
        }
    };
    let push = due["push"].as_array().cloned().unwrap_or_default();
    let merge = due["merge"].as_array().cloned().unwrap_or_default();
    if push.is_empty() && merge.is_empty() {
        return 0;
    }
    let now = state.clock.now().unix_millis();
    let Some(mirror) = state.lab.mirror(now) else {
        eprintln!("delivery: no fork is configured on the node");
        return 0;
    };
    let fresh = mirror.refresh(now);
    let Some(remote) = state.lab.push_remote() else {
        return 0;
    };
    let key = state.lab.push_key.as_deref();
    let mut told = 0;
    let tell = |branch: i64, body: serde_json::Value| -> bool {
        match hq.post(&format!("/v1/team/deliveries/{branch}"), &body) {
            Ok(_) => true,
            Err(e) => {
                eprintln!("delivery: HQ was not told what came of branch {branch}: {e}");
                false
            }
        }
    };
    for row in &push {
        let (Some(branch), Some(name), Some(head)) = (
            row["branch"].as_i64(),
            row["name"].as_str(),
            row["head"].as_str(),
        ) else {
            continue;
        };
        let scan = row["scan"].as_bool().unwrap_or(true);
        let out = match mirror.outgoing(name, head) {
            Ok(out) => out,
            Err(why) => {
                let text = format!(
                    "Not pushed: {name} could not be read for its privacy scan ({why}), so the \
                     scan cannot tell"
                );
                told += usize::from(tell(
                    branch,
                    json!({ "step": "push", "ok": false, "text": text }),
                ));
                continue;
            }
        };
        let needs = outgoing::owner_needs(&out.diff);
        if scan {
            let found = outgoing::scan(&out.diff, &out.messages, own);
            if !found.is_empty() {
                hq::record(
                    state,
                    &format!(
                        "the privacy scan held {name}: {} found, nothing pushed",
                        found.len()
                    ),
                );
                told += usize::from(tell(
                    branch,
                    json!({ "step": "held", "found": found_json(&found) }),
                ));
                continue;
            }
        }
        let checked = if scan {
            scanned_with(own)
        } else {
            "without its privacy scan, as the owner said".to_owned()
        };
        let (ok, text) = match mirror.push(name, head, &remote, key) {
            Ok(said) => (true, format!("{said}, {checked}")),
            Err(why) => (
                false,
                format!("Pushing {name} failed: {why}. It stays stored on the node."),
            ),
        };
        hq::record(
            state,
            &format!(
                "HQ pushed {name} ({}) to the fork: {}",
                &head[..12.min(head.len())],
                if ok { "pushed" } else { "it failed" }
            ),
        );
        told += usize::from(tell(
            branch,
            json!({ "step": "push", "ok": ok, "text": text, "needs": found_json(&needs) }),
        ));
    }
    // A merge reads develop as the fork has it now: not on a mirror that could not be refreshed.
    if let Err(why) = &fresh {
        if !merge.is_empty() {
            eprintln!("delivery: the mirror was not refreshed, so nothing is merged: {why}");
        }
        return told;
    }
    for row in &merge {
        let (Some(branch), Some(name), Some(head)) = (
            row["branch"].as_i64(),
            row["name"].as_str(),
            row["head"].as_str(),
        ) else {
            continue;
        };
        let (ok, text) = match mirror.merge_into_base(name, head, &remote, key) {
            Ok(said) => (true, said),
            Err(why) => (false, format!("Merging {name} failed: {why}")),
        };
        hq::record(
            state,
            &format!(
                "HQ merged {name} into the fork's {}: {}",
                mirror.base(),
                if ok { "merged" } else { "it failed" }
            ),
        );
        told += usize::from(tell(
            branch,
            json!({ "step": "merge", "ok": ok, "text": text }),
        ));
    }
    told
}

#[cfg(test)]
mod tests {
    use super::super::contracts::LabKnowledge;
    use super::super::relay::tests::{fake, node};
    use super::*;
    use endora_capabilities::ansible_nas::{self, Docs};
    use std::sync::Mutex;

    fn git_in(dir: &std::path::Path, args: &[&str]) -> String {
        let out = std::process::Command::new("git")
            .args(args)
            .current_dir(dir)
            .env("GIT_CONFIG_GLOBAL", "/dev/null")
            .env("GIT_CONFIG_NOSYSTEM", "1")
            .output()
            .unwrap();
        assert!(
            out.status.success(),
            "git {args:?}: {}",
            String::from_utf8_lossy(&out.stderr)
        );
        String::from_utf8_lossy(&out.stdout).trim().to_owned()
    }

    /// One branch of the team's, as the sandbox makes it and the node stores it: one commit by
    /// HQ with its trailer, adding `file` with `text`, cut from develop.
    fn stored(
        state: &AppState,
        root: &std::path::Path,
        name: &str,
        task: i64,
        file: &str,
        text: &str,
    ) -> String {
        let mirror = state.lab.mirror(0).unwrap();
        let work = root.join(format!("work-{task}"));
        let bundle = mirror.bundle(None, 0).unwrap();
        let checkout = root.join(format!("checkout-{task}.bundle"));
        std::fs::write(&checkout, &bundle.bytes).unwrap();
        git_in(
            root,
            &[
                "clone",
                "-q",
                "-b",
                "develop",
                checkout.to_str().unwrap(),
                work.to_str().unwrap(),
            ],
        );
        git_in(&work, &["checkout", "-q", "-b", name]);
        let path = work.join(file);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        git_in(&work, &["add", "."]);
        git_in(
            &work,
            &[
                "-c",
                "user.name=Endora HQ",
                "-c",
                "user.email=hq@endora.invalid",
                "commit",
                "-q",
                "-m",
                &format!("{name}\n\nMade-by: Endora HQ (Bo) for task #{task}"),
            ],
        );
        let delta = root.join(format!("delta-{task}.bundle"));
        git_in(
            &work,
            &[
                "bundle",
                "create",
                delta.to_str().unwrap(),
                &format!("develop..{name}"),
            ],
        );
        mirror
            .submit(&std::fs::read(&delta).unwrap(), name, task, 0)
            .unwrap()
            .head
    }

    /// From what HQ says is due to the fork, with real git and a local bare fork: a clean branch
    /// is pushed and what it needs of the owner told; one with a real-looking address is held,
    /// nothing pushed, and the address never said; then, merging on, the pushed one is merged
    /// into develop.
    #[tokio::test]
    async fn a_clean_branch_is_pushed_one_that_leaks_is_held_and_merging_merges() {
        let root = std::env::temp_dir().join(format!(
            "endora-node-delivery-{}-{}",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&root).unwrap();
        git_in(
            &root,
            &["init", "-q", "--bare", "-b", "develop", "fork.git"],
        );
        git_in(&root, &["clone", "-q", "fork.git", "seed"]);
        let seed = root.join("seed");
        std::fs::write(seed.join("nas.yml"), "---\n- hosts: all\n").unwrap();
        git_in(&seed, &["add", "."]);
        git_in(
            &seed,
            &[
                "-c",
                "user.name=John Doe",
                "-c",
                "user.email=john@example.com",
                "commit",
                "-q",
                "-m",
                "the lab",
            ],
        );
        git_in(&seed, &["push", "-q", "origin", "HEAD:develop"]);
        let fork = root.join("fork.git");
        let fork_url = fork.to_str().unwrap().to_owned();

        // What HQ says is due: the pass the test is at, and what it was told.
        let stage: Arc<Mutex<serde_json::Value>> = Arc::new(Mutex::new(json!({})));
        let due = stage.clone();
        let (hq_url, seen) = fake(Arc::new(move |path, _body| {
            if path == "/v1/team/deliveries" {
                return (200, "application/json", due.lock().unwrap().to_string());
            }
            (200, "application/json", json!({ "told": true }).to_string())
        }));
        let mut state = node(&hq_url, "http://127.0.0.1:9");
        state.lab =
            Arc::new(
                LabKnowledge::new(Docs::new(None, Box::new(ansible_nas::Web)), Vec::new())
                    .with_repo(root.join("data"), &fork_url, "develop", &fork_url),
            );
        let clean = stored(
            &state,
            &root,
            "hq/task-3-drift",
            3,
            "roles/drift/defaults/main.yml",
            "---\ndrift_token: \"{{ vault_drift_token }}\"\n",
        );
        let address = format!("{}.{}.7.23", 192, 168);
        let leak = stored(
            &state,
            &root,
            "hq/task-4-leak",
            4,
            "leak.yml",
            &format!("host: {address}\n"),
        );
        *stage.lock().unwrap() = json!({ "push": [
            { "branch": 1, "name": "hq/task-3-drift", "head": clean, "scan": true },
            { "branch": 2, "name": "hq/task-4-leak", "head": leak, "scan": true },
        ], "merge": [] });

        let own = vec!["New York".to_owned()];
        let told = tokio::task::spawn_blocking({
            let state = state.clone();
            move || deliver(&state, &own)
        })
        .await
        .unwrap();
        assert_eq!(told, 2);
        let posts: Vec<(String, serde_json::Value)> = seen
            .lock()
            .unwrap()
            .iter()
            .filter(|(path, _, _)| path.starts_with("/v1/team/deliveries/"))
            .map(|(path, _, body)| (path.clone(), serde_json::from_str(body).unwrap()))
            .collect();
        let (_, pushed) = posts
            .iter()
            .find(|(p, _)| p == "/v1/team/deliveries/1")
            .expect("told of the clean branch");
        assert_eq!(pushed["step"], "push");
        assert_eq!(pushed["ok"], true, "{pushed}");
        assert!(
            pushed["text"]
                .as_str()
                .unwrap()
                .contains("privacy scan clean (shapes, the secret tripwire and 1 of your own"),
            "{pushed}"
        );
        assert_eq!(
            pushed["needs"],
            json!([{ "what": "vault_drift_token", "at": "roles/drift/defaults/main.yml" }])
        );
        let (_, held) = posts
            .iter()
            .find(|(p, _)| p == "/v1/team/deliveries/2")
            .expect("told of the leaking branch");
        assert_eq!(held["step"], "held");
        assert_eq!(
            held["found"],
            json!([{ "what": "a real network address", "at": "leak.yml line 1" }])
        );
        assert!(!held.to_string().contains(&address), "the value was said");
        assert_eq!(
            git_in(&fork, &["rev-parse", "refs/heads/hq/task-3-drift"]),
            clean
        );
        assert!(
            git_in(&fork, &["branch", "--list", "hq/task-4-leak"]).is_empty(),
            "nothing of the held branch left"
        );

        // Merging on: the pushed branch is merged into develop.
        *stage.lock().unwrap() = json!({ "push": [], "merge": [
            { "branch": 1, "name": "hq/task-3-drift", "head": clean },
        ] });
        let told = tokio::task::spawn_blocking({
            let state = state.clone();
            move || deliver(&state, &[])
        })
        .await
        .unwrap();
        assert_eq!(told, 1);
        assert_eq!(git_in(&fork, &["rev-parse", "refs/heads/develop"]), clean);
        let merged: serde_json::Value =
            serde_json::from_str(&seen.lock().unwrap().last().unwrap().2).unwrap();
        assert_eq!(merged["step"], "merge", "{merged}");
        assert_eq!(merged["ok"], true, "{merged}");
        assert!(
            merged["text"].as_str().unwrap().contains("fast-forward"),
            "{merged}"
        );
        let _ = std::fs::remove_dir_all(&root);
    }
}
