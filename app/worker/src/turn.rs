//! One turn of a team member (ADR 0102, phase 4), which is the worker's whole job. It opens
//! the team protocol on HQ with the turn's token, reads what the turn is for, and lets the
//! model work through the team's tools a round at a time, until it answers without calling
//! one or the rounds run out. Its last word goes in the task's thread.
//!
//! The token is the only credential here, for HQ and the node alike. A 401 or a 402 from
//! either means the turn is over (stopped, ended, or out of money), and the worker does
//! nothing more. Beside the team's tools the model is offered the node's contracts (ADR 0106),
//! narrow abilities on the home lab. Each tool call goes where its tool came from, HQ or the
//! node, which decides it by its own rules: the worker itself acts on nothing. The one
//! exception is the workbench's shell (ring 1), which runs in the sandbox the worker is in:
//! nothing outside it, since the sandbox is offline but for the team gateway.

use std::path::Path;
use std::time::Duration;

use serde_json::{Value, json};

use crate::repo;
use crate::shell::Shell;

/// The MCP version the worker speaks.
pub(crate) const PROTOCOL: &str = "2025-06-18";

/// How long HQ has to answer a protocol call. It is on the same network, so anything slower
/// is a fault.
const HQ_ANSWERS_WITHIN: Duration = Duration::from_secs(60);

/// How long one model call may take: a local model on a small machine is slow.
const LONGEST_CALL: Duration = Duration::from_secs(10 * 60);

/// The most of a reason the worker says.
const SAYS_AT_MOST: usize = 300;

/// The most HQ keeps of a post in a thread (the protocol's own limit).
pub(crate) const POST_AT_MOST: usize = 4000;

/// The most HQ keeps of a question to the owner.
const ASK_AT_MOST: usize = 1200;

/// The most calls the line saying what a turn did names; the rest are counted.
const NAMES_AT_MOST: usize = 12;

/// The most a cloud model may say in one round. The relay lowers it to what the turn can
/// still pay for; a round's answer is a few sentences and tool calls, far less than this.
const MOST_A_REPLY: u32 = 4096;

/// The workbench's shell, as the model names it.
pub(crate) const SHELL: &str = "shell";

/// The wing's git (ADR 0115), as the node offers it: the worker unpacks and makes the bundles,
/// in the workbench alone.
pub(crate) const REPO_CHECKOUT: &str = "repo_checkout";
pub(crate) const REPO_SUBMIT: &str = "repo_submit";

/// The most of a command the line saying what a turn did shows.
const COMMAND_AT_MOST: usize = 60;

/// The version of Anthropic's Messages API the worker speaks.
const ANTHROPIC_VERSION: &str = "2023-06-01";

/// Which API the model relay speaks to the worker.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Api {
    /// OpenAI's chat completions: the local relay, to the house's own model.
    OpenAi,
    /// Anthropic's Messages: the Anthropic relay, to Claude (ADR 0109).
    Anthropic,
}

/// What a turn is taken with.
pub struct Setup {
    /// The turn's own token: the only credential the worker has.
    pub token: String,
    /// HQ's team protocol.
    pub mcp: String,
    /// The node's model relay, up to its `/v1`.
    pub model_url: String,
    /// The node's contracts.
    pub contracts: String,
    /// The model the worker names.
    pub model: String,
    /// Which API the relay speaks.
    pub api: Api,
    /// The most rounds of the model a turn may take.
    pub most_rounds: u32,
    /// Where the workbench's shell runs, when the worker is in the workbench; `None` anywhere
    /// else, where it has no shell.
    pub workbench: Option<std::path::PathBuf>,
    /// How hard Claude tries this turn, as HQ decided it (one of [`EFFORTS`]); `None` leaves
    /// it to the model. The local relay is never told.
    pub effort: Option<String>,
    /// Whether Claude Code takes the turn (ADR 0117), started by the worker in the workbench.
    pub claude_code: bool,
}

/// The efforts Claude may be asked for, least first.
pub const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// How a turn was taken.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Taken {
    /// The model answered without calling a tool.
    Answered,
    /// The rounds ran out with the model still calling tools. What it said on the way is posted
    /// as its last word, said to be unfinished.
    OutOfRounds,
    /// The model asked the owner, which ends the turn: HQ wakes the member again when the
    /// owner has seen it.
    AskedTheOwner,
    /// The model moved its task on with `set_status`, the turn's last step.
    MovedOn,
}

/// Why a turn stopped short. The reason never carries the token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Stopped {
    /// HQ or the relay answered 401 or 402: the turn's token opens nothing now, or its money
    /// is spent.
    Over(String),
    /// Anything else, said shortly.
    Failed(String),
}

/// Takes one turn.
///
/// # Errors
/// [`Stopped::Over`] when HQ or the relay ends the turn; [`Stopped::Failed`] when either cannot
/// be reached or answers in a way the worker cannot use.
pub fn take(setup: &Setup) -> Result<Taken, Stopped> {
    turn(setup).map_err(|stopped| match stopped {
        Stopped::Over(why) => Stopped::Over(quiet(&why, &setup.token)),
        Stopped::Failed(why) => Stopped::Failed(quiet(&why, &setup.token)),
    })
}

fn turn(setup: &Setup) -> Result<Taken, Stopped> {
    let mut hq = Hq::new(&setup.mcp, &setup.token);
    hq.open()?;
    let listed = hq.request("tools/list", json!({}))?;
    let mut offered: Vec<Value> = listed["tools"].as_array().cloned().unwrap_or_default();
    let node = Node::new(&setup.contracts, &setup.token);
    // A contract named like a team tool is left out: the name goes to HQ, as it always did.
    // The wing's git (ADR 0115) is offered only in the workbench, where there is a git and a
    // /work to unpack the fork into: the bundles are the worker's to carry, never the model's.
    let contracts: Vec<Value> = node
        .list()?
        .into_iter()
        .filter(|c| !offered.iter().any(|t| t["name"] == c["name"]))
        .filter(|c| setup.workbench.is_some() || !is_repo_tool(c["name"].as_str().unwrap_or("")))
        .collect();
    let on_the_node: Vec<String> = contracts
        .iter()
        .filter_map(|c| c["name"].as_str().map(str::to_owned))
        .collect();
    // What only looks, as the node marks it: the line saying what the turn did tells a turn
    // that only read from one that acted.
    let mut did = Ledger {
        reads: contracts
            .iter()
            .filter(|c| c["kind"] == "read")
            .filter_map(|c| c["name"].as_str().map(str::to_owned))
            .chain(std::iter::once("my_turn".to_owned()))
            .collect(),
        calls: Vec::new(),
    };
    offered.extend(contracts);
    // In the workbench, the sandbox's own shell: run here, never sent to HQ or the node, so a
    // tool of theirs named the same is left out.
    let shell = setup
        .workbench
        .as_ref()
        .map(|dir| Shell::new(dir.clone(), &setup.token));
    if shell.is_some() {
        offered.retain(|t| t["name"] != SHELL);
        offered.push(shell_tool());
    }
    // A lesson is a story since ADR 0122: told with tell_story.
    let lessons = offered.iter().any(|t| t["name"] == "tell_story");
    let (briefing, refused) = hq.call("my_turn", &json!({}))?;
    if refused {
        return Err(Stopped::Failed(format!("HQ refused my_turn: {briefing}")));
    }
    let task = task_of(&briefing);
    let who = member_of(&briefing).unwrap_or_else(|| "a member".to_owned());
    // Claiming is the first step of every turn with a task, and a small model skips it: the
    // worker takes that step itself and tells the model what HQ said. A refusal (another
    // member has it) is news for the model, not a failure of the turn.
    let mut briefing = briefing;
    let mut answered = Answered::default();
    if let Some(task) = task {
        let claim = json!({ "task": task });
        let (said, _) = hq.call("claim", &claim)?;
        // A model that claims it again is answered from this, not sent to HQ twice.
        answered
            .given
            .insert(format!("claim {claim}"), said.clone());
        briefing = format!(
            "{}\n\nBefore your turn began, your task was claimed for you: {said}",
            briefing.trim_end()
        );
    }
    let model = Model::new(setup);
    let told = instructions(task.is_some(), &on_the_node, lessons, shell.is_some());
    let mut talk = Talk::new(setup.api, told, briefing, &offered);
    talk.effort.clone_from(&setup.effort);
    // One loop whichever API the relay speaks: the rounds limit, the repeat guard, the line
    // saying what the turn did, and the ends of a turn are the same for every brain.
    let taken = (|| {
        // What the model said beside its calls: its last word should the rounds run out.
        let mut on_the_way: Vec<String> = Vec::new();
        for _ in 0..setup.most_rounds {
            let said = model.ask(&talk)?;
            // Its words are part of what the turn said, its last ones too.
            talk.said(&said);
            if said.calls.is_empty() {
                last_word(&mut hq, task, said.text.trim(), &did.line())?;
                return Ok(Taken::Answered);
            }
            if !said.text.trim().is_empty() {
                on_the_way.push(said.text.trim().to_owned());
            }
            let mut replies = Vec::new();
            for call in &said.calls {
                let reply = match (&shell, setup.workbench.as_deref()) {
                    (Some(shell), _) if call.name == SHELL => in_the_shell(shell, &mut did, call),
                    (_, Some(dir)) if is_repo_tool(&call.name) => in_the_repo(
                        &node,
                        dir,
                        &who,
                        task,
                        on_the_node.iter().any(|n| n == REPO_SUBMIT),
                        &mut did,
                        call,
                    )?,
                    _ => run(
                        &mut hq,
                        &node,
                        &on_the_node,
                        task,
                        &mut answered,
                        &mut did,
                        call,
                    )?,
                };
                let ended = if reply.asked {
                    Some(Taken::AskedTheOwner)
                } else if reply.moved_on {
                    Some(Taken::MovedOn)
                } else {
                    None
                };
                replies.push((call.id.clone(), reply));
                // The call that ends the turn is answered in its transcript like any other.
                if let Some(taken) = ended {
                    talk.answered(replies);
                    return Ok(taken);
                }
            }
            talk.answered(replies);
        }
        // Out of rounds, what it found so far goes in the thread, said to be unfinished: else
        // the next turn starts again from the thread alone.
        let line = format!(
            "The turn's {} rounds ran out before it finished: the words above are what it said \
             on the way, not an answer. {}",
            setup.most_rounds,
            did.line()
        );
        let room = POST_AT_MOST.saturating_sub(line.chars().count() + 2);
        last_word(&mut hq, task, &newest(&on_the_way, room), &line)?;
        Ok(Taken::OutOfRounds)
    })();
    // What the turn said is kept by HQ for a while (ADR 0129), however the turn ended: the
    // node scrubs it of secrets on the way.
    node.tell_conversation("worker", &talk.as_text());
    taken
}

/// The node's container tools, in the order the instructions name them.
const LAB_TOOLS: [&str; 3] = ["homelab_status", "container_logs", "restart_container"];

/// How to take a good turn: the worker's own instructions, written for Endora. `posted` says
/// whether its last word goes in the task's thread for it; `on_the_node`, the tools the node
/// offers it (the home lab's container tools among them are named as the turn's own: live, a
/// lead on Claude offered `homelab_status` said three times that its tools could not read the
/// home lab, reading HQ's briefing, which lists only HQ's tools, as all it had); `lessons`,
/// whether it tells the team's lessons as stories (`tell_story`); `shell`, whether it is in the
/// workbench, with a shell.
pub(crate) fn instructions(
    posted: bool,
    on_the_node: &[String],
    lessons: bool,
    shell: bool,
) -> String {
    let mut told = String::from(
        "You are one member of a small team at Endora HQ, taking one turn of work. The first \
         message is what my_turn says: who you are, your task, why it exists, and what is new \
         in its thread. Read the thread: the owner may have told you something there.\n\
         \n\
         Take your turn in these steps:\n",
    );
    if shell {
        told.push_str("1. Look: use the tools to find out what you need, the shell among them.\n");
    } else {
        told.push_str(
            "1. Look: use the tools to find out what you need. You have no other way to see or \
             do anything. You cannot run commands, ping, open files or browse: never offer to.\n",
        );
    }
    told.push_str(
        "2. Post in the task's thread what you found, briefly, so whoever comes next can go on \
         from there.\n\
         3. End the turn with exactly one of these: move the task on with set_status and say \
         why; create sub-tasks for members below you when the work needs them; or put a \
         concrete plan or question with ask_owner, which goes to whoever your briefing says \
         is above you (asking ends your turn, and you are woken when they answer).",
    );
    let lab: Vec<&str> = LAB_TOOLS
        .into_iter()
        .filter(|t| on_the_node.iter().any(|n| n == t))
        .collect();
    if !on_the_node.is_empty() {
        if lab.is_empty() {
            told.push_str("\n\nThe home lab tools are Endora's contracts:\n");
        } else {
            told.push_str(&format!(
                "\n\nThe home lab tools are Endora's contracts, and this turn they are yours: \
                 {}. Whatever your role, look with them yourself before you hand anything on, \
                 and never say you cannot read the home lab.\n",
                lab.join(", ")
            ));
        }
        told.push_str(
            "\
             - homelab_status and container_logs only look: use them freely to see what is \
             wrong and why.\n\
             - An alarm is a moment, not the state now: check homelab_status before you act, \
             since what alarmed may be back up (Endora says so in the thread). If everything \
             has recovered, say so and close the task: a routine's check as done, other work \
             moved on with set_status.\n\
             - restart_container is a safe fix you may make on your own, within its limits (one \
             restart of a container an hour, three in all), and the owner is told: look and \
             diagnose first, and say why.\n\
             - After a fix, check back to confirm it held: call check_back with the minutes to \
             wait and what you will look at.\n\
             - Endora's other tools say whether they read or act; an action that needs the \
             owner's approval says which of your task's criteria it serves (serves), then goes \
             on their desk and ends your turn, and you are woken when they decide. One that \
             serves none of them is refused, and never reaches the desk.\n\
             - Anything else on the home lab is the owner's to decide: ask with ask_owner, and \
             never try to work around a refusal.\n\
             - A question about the home lab goes to the team's reviewer first, who checks every \
             claim and container name against these tools: say only what you saw.\n\
             - Report something as a problem only when a tool shows it causing harm: an item \
             stuck in a queue, a container down or unhealthy, a failure that repeats. Errors or \
             odd lines in a log are evidence to explain, not problems in themselves. Normal \
             operation (e.g. an RSS sync that finds releases and grabs none) is not a problem.",
        );
        // How the lab is built (ADR 0112): one text, shared with HQ's house log.
        told.push_str("\n\n");
        told.push_str(&endora_kernel::lab::lab_built_paragraph());
    }
    if shell {
        told.push_str(
            "\n\nYou have a real shell in a disposable sandbox: run commands, git and \
             python there with the shell tool, and say why. The sandbox is offline: neither the \
             internet nor the house's services are reachable from the shell, so use the house \
             through the team's tools. Anything you leave in /work is gone when the \
             turn ends: write durable findings in the task's thread",
        );
        told.push_str(if lessons {
            ", or as a story with tell_story."
        } else {
            "."
        });
    }
    // A checkout without a submission is the tester's: a branch is read, not worked on.
    if on_the_node.iter().any(|n| n == REPO_CHECKOUT)
        && !on_the_node.iter().any(|n| n == REPO_SUBMIT)
    {
        told.push_str(
            "\n\nThe wing's git, for your reading: repo_checkout with a branch's name \
             (hq/task-N-<slug>, as the task's thread gives it) puts the team's submitted branch \
             in /work/repo, on top of its base, and you read it in the shell: git log, git diff \
             against the base, the files themselves. Cite that checkout's receipt (receipt r12) \
             as a reading of your own, with the check's, when you pass the work or send it back. \
             You read; you submit and push nothing, and what you leave in /work is gone when \
             the turn ends. Never cite or copy from group_vars/*.yml, host_vars or any \
             inventory: they hold the owner's own values.",
        );
    } else if on_the_node.iter().any(|n| n == REPO_CHECKOUT) {
        told.push_str(
            "\n\nThe wing's git: repo_checkout brings the lab's ansible-nas fork into /work/repo \
             with git set up (your commits are authored Endora HQ and carry the trailer for \
             you). Work on a branch named hq/task-N-<slug> for your task (N is your task's \
             number), commit in small steps with the shell, then repo_submit that branch: \
             Endora keeps it, writes its diff as a document on the task and runs the lab's \
             checks; the result comes back as a receipt. A branch of your work that is \
             submitted stays kept on the node, and repo_checkout with its name (branch: \
             hq/task-N-<slug>) brings it back whole in a later turn, on top of its base: go on \
             from it, never recreate it. If a check failed for the check box's own reasons (its \
             line says a tool could not run, or that develop could not be checked), ask for the \
             same branch to be checked again with recheck rather than submitting it again. \
             Then submit the task for review, citing that receipt. You never push: \
             the owner pushes an accepted branch to the fork and merges it there. Never edit, \
             cite or copy from group_vars/*.yml, host_vars or any inventory: they hold the \
             owner's own values, and a branch touching them is refused.",
        );
    }
    if lessons {
        told.push_str(
            "\n\nWhen the owner corrects you, or you learn something about this house that the \
             team will need again, tell it as a story with tell_story (when it matters, the \
             tempting move, what to do instead, and why) so nobody asks twice; cite a story by \
             its handle rather than retelling it.",
        );
    }
    // How done is shown (ADR 0114): from the record, never from what a model says.
    told.push_str(
        "\n\nDone is shown, never said: your task's criteria (in my_turn) say what done means. \
         Each result of Endora's tools names its receipt (receipt r12): cite those ids in \
         `evidence`, one list per criterion, when you submit with set_status in_review or close \
         a routine's check as done; anything about the lab now needs a reading taken this turn, \
         never memory or another member's word. The tester then re-checks it with her own \
         readings, and the lead accepts it. A post that states a finding cites what it rests \
         on. Keep it short, and never say you did what you did not do. When your part is done, \
         stop calling tools and reply in one or two sentences.",
    );
    if posted {
        told.push_str(" That reply goes in your task's thread.");
    }
    told
}

/// Whether a tool is the wing's git, which the worker carries out in the sandbox (ADR 0115).
pub(crate) fn is_repo_tool(name: &str) -> bool {
    name == REPO_CHECKOUT || name == REPO_SUBMIT
}

/// The member a turn is of, from HQ's briefing: its first line, "You are Kit, the team's lead."
pub(crate) fn member_of(briefing: &str) -> Option<String> {
    let first = briefing.lines().next()?.strip_prefix("You are ")?;
    let (name, _) = first.split_once(',')?;
    let name = name.trim();
    (!name.is_empty() && name.chars().count() <= 40).then(|| name.to_owned())
}

/// What the model reads after a checkout: where it is, and what to do with it. A turn that
/// may submit (`can_submit`: a lead or a developer) is told to branch from the base, or, for the
/// team's own kept branch, to go on from it and submit it again; a turn that may not (the
/// tester's) is told to read it and to cite this checkout's receipt as a reading of its own.
pub(crate) fn said_after_checkout(
    out: &repo::CheckedOut,
    base: &str,
    kept: bool,
    can_submit: bool,
    task: Option<i64>,
    receipt: &str,
) -> String {
    let at = &out.head[..12.min(out.head.len())];
    let (branch, path, files) = (&out.branch, out.path.display(), out.files);
    if !can_submit {
        return format!(
            "Checked out {branch} at {at} into {path} ({files} files){receipt}. It sits on top \
             of {base}. Read it with the shell: `cd {path} && git log --oneline \
             {base}..{branch}`, `git diff {base}..{branch}`, the files themselves. You read; \
             you submit and push nothing. Cite this checkout's receipt as a reading of your \
             own, with the check's, when you pass the work or send it back. Never cite or copy \
             from group_vars/*.yml, host_vars or any inventory."
        );
    }
    if kept {
        return format!(
            "Checked out the team's kept branch {branch} at {at} into {path} ({files} \
             files){receipt}. It sits on top of {base}, which is a branch here too. Go on from \
             it: edit, `git add -A && git commit -m \"what and why\"` in {path}, then \
             repo_submit this branch again; if {base} has moved on, `git rebase {base}` \
             first. Git is set up: your commits are authored Endora HQ and the trailer is \
             added for you. Never edit, cite or copy from group_vars/*.yml, host_vars or any \
             inventory: they hold the owner's own values, and a branch touching them is \
             refused."
        );
    }
    let slug = task.map_or_else(|| "N-<slug>".to_owned(), |t| format!("{t}-<slug>"));
    format!(
        "Checked out {branch} at {at} into {path} ({files} files){receipt}. Your base is \
         {base}: branch from it and your submission must sit on top of it. Git is set up: \
         your commits are authored Endora HQ and the trailer is added for you. Work on a \
         branch of your own: `cd {path} && git checkout -b hq/task-{slug} {base}`, edit, \
         `git add -A && git commit -m \"what and why\"`, then repo_submit with that branch. \
         Never edit, cite or copy from group_vars/*.yml, host_vars or any inventory: they \
         hold the owner's own values, and a branch touching them is refused."
    )
}

/// What the node handed over for `repo_checkout`, read from its text.
pub(crate) struct Handed {
    /// The branch to be on: the fork's, or the team's kept one.
    pub(crate) branch: String,
    /// The branch the wing works from; the branch itself when the node names none.
    pub(crate) base: String,
    /// Whether it is the team's own kept submission (ADR 0115, amended), bundled with its base
    /// and unpacked as a head of its own on top of it.
    pub(crate) kept: bool,
    /// The bundle's bytes.
    pub(crate) bundle: Vec<u8>,
    /// The node's receipt after the text, as the model reads it (` (receipt r12)`), or nothing.
    pub(crate) receipt: String,
}

/// The node's answer to `repo_checkout`: its text is the bundle as JSON, with the receipt
/// after it. `None` when it is not that, or holds no bundle.
pub(crate) fn handed_over(said: &str) -> Option<Handed> {
    use base64::Engine as _;
    let (payload, receipt) = said
        .rsplit_once(" (receipt ")
        .map_or((said, String::new()), |(p, r)| {
            (p, format!(" (receipt {r}"))
        });
    let handed: Value = serde_json::from_str(payload).ok()?;
    let bundle = base64::engine::general_purpose::STANDARD
        .decode(handed["bundle"].as_str()?)
        .ok()?;
    let branch = handed["branch"].as_str().unwrap_or("main").to_owned();
    let base = handed["base"]
        .as_str()
        .map_or_else(|| branch.clone(), str::to_owned);
    Some(Handed {
        branch,
        base,
        kept: handed["kept"] == true,
        bundle,
        receipt,
    })
}

/// Runs one call of the wing's git (ADR 0115) in the sandbox: a checkout asks the node for the
/// fork's bundle and unpacks it into /work/repo; a submission bundles the member's branch and
/// sends it to the node, which decides it. The model reads where the checkout is, or what the
/// node said; it never sees a bundle. `can_submit` is whether the node offers this turn
/// `repo_submit`: a tester's checkout is told to be read, not worked on.
pub(crate) fn in_the_repo(
    node: &Node<'_>,
    dir: &Path,
    member: &str,
    task: Option<i64>,
    can_submit: bool,
    did: &mut Ledger,
    call: &Call,
) -> Result<Reply, Stopped> {
    let refused = |text: String| Reply {
        text: format!("Refused: {text}"),
        refused: true,
        asked: false,
        moved_on: false,
    };
    let args = match arguments(&call.arguments) {
        Ok(args) => args,
        Err(why) => {
            did.unreadable(&call.name);
            return Ok(refused(why));
        }
    };
    if args["why"].as_str().is_none_or(|why| why.trim().is_empty()) {
        did.note(&call.name, &args, Outcome::Refused);
        return Ok(refused("say why, in `why`".to_owned()));
    }
    if call.name == REPO_SUBMIT {
        let branch = args["branch"].as_str().unwrap_or("").trim().to_owned();
        let (bytes, head) = match repo::bundle(dir, &branch) {
            Ok(bundled) => bundled,
            Err(why) => {
                did.note(&call.name, &args, Outcome::Refused);
                return Ok(refused(why));
            }
        };
        use base64::Engine as _;
        let sent = json!({
            "branch": branch, "head": head, "why": args["why"],
            "bundle": base64::engine::general_purpose::STANDARD.encode(&bytes),
        });
        let (said, outcome, _) = node.call(REPO_SUBMIT, &sent)?;
        did.note(&call.name, &args, outcome);
        if outcome != Outcome::Done {
            return Ok(refused(said));
        }
        return Ok(Reply {
            text: said,
            refused: false,
            asked: false,
            moved_on: false,
        });
    }
    let sent = json!({ "branch": args["branch"], "why": args["why"] });
    let (said, outcome, _) = node.call(REPO_CHECKOUT, &sent)?;
    if outcome != Outcome::Done {
        did.note(&call.name, &args, outcome);
        return Ok(refused(said));
    }
    let Some(handed) = handed_over(&said) else {
        did.note(&call.name, &args, Outcome::Failed);
        return Ok(refused(
            "the node's checkout could not be read as a bundle".to_owned(),
        ));
    };
    let Handed {
        branch,
        base,
        kept,
        bundle,
        receipt,
    } = handed;
    match repo::checkout(dir, &bundle, &branch, &base, kept, member, task) {
        Ok(out) => {
            did.note(&call.name, &args, Outcome::Done);
            Ok(Reply {
                text: said_after_checkout(&out, &base, kept, can_submit, task, &receipt),
                refused: false,
                asked: false,
                moved_on: false,
            })
        }
        Err(why) => {
            did.note(&call.name, &args, Outcome::Failed);
            Ok(refused(format!(
                "the checkout could not be unpacked: {why}"
            )))
        }
    }
}

/// The task a turn is about, from HQ's briefing (what `my_turn` starts with): the number in
/// its line `Your task: <title> (#<task>, <status>, <priority>)`, which comes before the
/// description and the thread. `None` for a turn about no task, or a briefing not in that
/// shape (a title over two lines); the model is then left to post for itself.
///
/// A title written to look like that line's end can name another task: the last word then
/// lands in that task's thread, which the member could post in anyway.
pub(crate) fn task_of(briefing: &str) -> Option<i64> {
    let line = briefing.lines().find(|l| l.starts_with("Your task: "))?;
    let (_, inside) = line.strip_suffix(')')?.rsplit_once("(#")?;
    let mut parts = inside.split(", ");
    let task = parts.next()?.parse().ok()?;
    (parts.count() == 2).then_some(task)
}

/// A tool's input schema as MCP lists it, or an empty object's for one that has none.
pub(crate) fn schema_of(tool: &Value) -> Value {
    match &tool["inputSchema"] {
        Value::Null => json!({ "type": "object", "properties": {} }),
        schema => schema.clone(),
    }
}

/// The team's tools, as MCP lists them, offered to the model as OpenAI functions.
fn functions(tools: &[Value]) -> Vec<Value> {
    tools
        .iter()
        .map(|tool| {
            json!({ "type": "function", "function": {
                "name": tool["name"],
                "description": tool["description"].as_str().unwrap_or(""),
                "parameters": schema_of(tool),
            } })
        })
        .collect()
}

/// The team's tools, as MCP lists them, offered to Claude as Anthropic's tools: a name, a
/// description and an input schema, and nothing else (no `type`, which would make one a tool
/// run on Anthropic's side, and the relay refuse the call). They go by name, so the prompt
/// they start is the same whatever order HQ and the node listed them in, and its cache holds.
fn anthropic_tools(tools: &[Value]) -> Vec<Value> {
    let mut offered: Vec<Value> = tools
        .iter()
        .map(|tool| {
            json!({
                "name": tool["name"],
                "description": tool["description"].as_str().unwrap_or(""),
                "input_schema": schema_of(tool),
            })
        })
        .collect();
    offered.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    offered
}

/// One tool call the model asked for, whichever API it came by.
pub(crate) struct Call {
    /// Its id, as the model gave it, for the answer to name.
    pub(crate) id: Value,
    /// The tool.
    pub(crate) name: String,
    /// Its arguments, as the model gave them: JSON text (OpenAI's) or an object (Anthropic's).
    pub(crate) arguments: Value,
}

/// What the model said in a round.
struct Said {
    /// Its words.
    text: String,
    /// The tools it called, in order.
    calls: Vec<Call>,
    /// Its message, as the conversation keeps it.
    message: Value,
}

/// What the worker answers one call with.
pub(crate) struct Reply {
    /// What the model reads.
    pub(crate) text: String,
    /// Whether it was refused, or could not be done.
    pub(crate) refused: bool,
    /// Whether it put something on the owner's desk, which ends the turn.
    pub(crate) asked: bool,
    /// Whether it moved the turn's own task on, or HQ said the turn has ended: either ends it.
    pub(crate) moved_on: bool,
}

/// The conversation with the model, in the API the relay speaks: the instructions, the
/// briefing, each round's answer and what each call came to.
struct Talk {
    api: Api,
    system: String,
    messages: Vec<Value>,
    tools: Vec<Value>,
    /// How hard Claude tries, when HQ said.
    effort: Option<String>,
}

impl Talk {
    /// A conversation that starts with the instructions and the briefing, offering `offered`.
    fn new(api: Api, system: String, briefing: String, offered: &[Value]) -> Self {
        let briefing = json!({ "role": "user", "content": briefing });
        match api {
            Api::OpenAi => Self {
                api,
                messages: vec![json!({ "role": "system", "content": system }), briefing],
                system: String::new(),
                tools: functions(offered),
                effort: None,
            },
            Api::Anthropic => Self {
                api,
                system,
                messages: vec![briefing],
                tools: anthropic_tools(offered),
                effort: None,
            },
        }
    }

    /// The body of one round's call for `model`. Claude's asks Anthropic to cache the whole
    /// prompt (the mark at the top puts the cache's breakpoint on its last block), so each
    /// round reads what the round before wrote: the tools, the instructions, the briefing and
    /// every round so far, which never change once sent. Only what is new is written.
    fn body(&self, model: &str) -> Value {
        if self.api == Api::OpenAi {
            return json!({ "model": model, "messages": self.messages, "tools": self.tools });
        }
        let mut body = json!({
            "model": model, "max_tokens": MOST_A_REPLY, "system": self.system,
            "messages": self.messages, "tools": self.tools,
            "cache_control": { "type": "ephemeral" },
        });
        if let Some(effort) = &self.effort {
            body["output_config"] = json!({ "effort": effort });
        }
        body
    }

    /// The conversation so far as plain text (ADR 0129), in the shape of the API it was held in.
    fn as_text(&self) -> String {
        match self.api {
            Api::OpenAi => crate::transcript::openai(&self.messages),
            Api::Anthropic => crate::transcript::anthropic(&self.system, &self.messages),
        }
    }

    /// The model's answer, kept as it said it.
    fn said(&mut self, said: &Said) {
        self.messages.push(said.message.clone());
    }

    /// What each call came to: OpenAI's one message a call; Anthropic's one user message with
    /// a result for every call, a refusal marked as an error.
    fn answered(&mut self, replies: Vec<(Value, Reply)>) {
        if self.api == Api::OpenAi {
            for (id, reply) in replies {
                self.messages
                    .push(json!({ "role": "tool", "tool_call_id": id, "content": reply.text }));
            }
            return;
        }
        let results: Vec<Value> = replies
            .into_iter()
            .map(|(id, reply)| {
                let mut result =
                    json!({ "type": "tool_result", "tool_use_id": id, "content": reply.text });
                if reply.refused {
                    result["is_error"] = json!(true);
                }
                result
            })
            .collect();
        self.messages
            .push(json!({ "role": "user", "content": results }));
    }
}

/// A tool call's arguments. In OpenAI's format they are a JSON object written as text; an
/// object is taken as it is (some servers send one), and empty text as no arguments.
fn arguments(given: &Value) -> Result<Value, String> {
    let parsed = match given {
        Value::String(text) if text.trim().is_empty() => return Ok(json!({})),
        Value::String(text) => serde_json::from_str(text).map_err(|e| {
            format!("the arguments are not JSON ({e}); send them again as one JSON object")
        })?,
        other => other.clone(),
    };
    if !parsed.is_object() {
        return Err("the arguments must be one JSON object".to_owned());
    }
    Ok(parsed)
}

/// What the model is told when it asks again for what it already has.
const ALREADY: &str = "You already have this result; use it, or do something else";

/// The calls a turn has made, by tool and arguments, and what each last said: so the same
/// call asked again with nothing new is answered from it. A small model will otherwise ask
/// for the same thing round after round.
#[derive(Default)]
pub(crate) struct Answered {
    /// The call made last.
    last: Option<String>,
    /// What each call last said.
    said: std::collections::HashMap<String, String>,
    /// What the worker already asked for the model before its first round (the claim): the
    /// model asking the same is told what HQ said, once, without asking HQ again.
    given: std::collections::HashMap<String, String>,
}

/// Runs one tool call the model asked for, and gives what the model reads back, and whether
/// it put a question on the owner's desk (which ends the turn). A refusal, HQ's or the
/// worker's own (arguments it cannot read), is said as one, so that the model can do
/// something about it. The same call asked again straight after is not sent at all; asked
/// again after something else, it is sent, and if it says just what it said before the model
/// is told it already has that.
pub(crate) fn run(
    hq: &mut Hq<'_>,
    node: &Node<'_>,
    on_the_node: &[String],
    task: Option<i64>,
    answered: &mut Answered,
    did: &mut Ledger,
    call: &Call,
) -> Result<Reply, Stopped> {
    let tool = call.name.as_str();
    let reply = |text: String, refused: bool| Reply {
        text,
        refused,
        asked: false,
        moved_on: false,
    };
    let args = match arguments(&call.arguments) {
        Ok(args) => args,
        Err(why) => {
            did.unreadable(tool);
            return Ok(reply(format!("Refused: {why}"), true));
        }
    };
    let key = format!("{tool} {args}");
    if let Some(said) = answered.given.remove(&key) {
        answered.said.insert(key.clone(), said.clone());
        answered.last = Some(key);
        return Ok(reply(said, false));
    }
    if answered.last.as_deref() == Some(key.as_str()) && answered.said.contains_key(&key) {
        return Ok(reply(ALREADY.to_owned(), false));
    }
    // A question to the owner carries what the turn actually did, so the reviewer and the
    // owner read the truth beside the claim.
    let sent = if tool == "ask_owner" {
        with_what_was_done(&args, did)
    } else {
        args.clone()
    };
    let (said, outcome, approval) = if on_the_node.iter().any(|c| c == tool) {
        node.call(tool, &sent)?
    } else {
        let (said, refused) = hq.call(tool, &sent)?;
        let outcome = if refused {
            Outcome::Refused
        } else {
            Outcome::Done
        };
        (said, outcome, false)
    };
    if tool != "ask_owner" {
        did.note(tool, &args, outcome);
    }
    let refused = outcome != Outcome::Done;
    let said = if refused {
        format!("Refused: {said}")
    } else {
        said
    };
    // A question to the owner ends the turn, and so does an action the node put on the
    // owner's desk for approval (ADR 0108): the member is woken when the owner decides.
    let asked = !refused && (tool == "ask_owner" || approval);
    let nothing_new = answered.said.get(&key) == Some(&said);
    answered.said.insert(key.clone(), said.clone());
    answered.last = Some(key);
    if nothing_new && !asked {
        return Ok(reply(ALREADY.to_owned(), false));
    }
    // HQ saying the turn has ended ends it, whatever the tool: a scenario's world when it is
    // over, the drain's wrap-up.
    let ended_by_hq = !refused && said.to_lowercase().contains("this turn has ended");
    Ok(Reply {
        text: said,
        refused,
        asked,
        moved_on: ended_by_hq || (!refused && moves_on(task, tool, &args)),
    })
}

/// Whether a call moves the turn's own task on, the turn's last step: to `in_review`,
/// `blocked` or `done`. Run 1831, told only in its instructions that this ends the turn, set
/// its task blocked and then called `set_status` six times more. Starting work (`doing`) or
/// moving a sub-task on is not the turn's end.
pub(crate) fn moves_on(task: Option<i64>, tool: &str, args: &Value) -> bool {
    tool == "set_status"
        && task.is_some_and(|t| args["task"].as_i64() == Some(t))
        && matches!(
            args["status"].as_str(),
            Some("in_review" | "blocked" | "done")
        )
}

/// The workbench's shell, as MCP lists a tool.
fn shell_tool() -> Value {
    json!({
        "name": SHELL,
        "description": "Run a command with bash in your sandbox, in /work, and read what it \
            printed (stdout and stderr together, its start and end when long) and its exit \
            code. The sandbox is offline: neither the internet nor the house's services are \
            reachable from it. Nothing you leave here outlives the turn.",
        "inputSchema": {
            "type": "object",
            "properties": {
                "command": { "type": "string", "description": "What bash runs." },
                "why": { "type": "string", "description": "Why you run it, in a few words." },
                "seconds": {
                    "type": "integer",
                    "description": "How long it may take: 60 unless you say, at most 300.",
                },
            },
            "required": ["command", "why"],
        },
    })
}

/// Runs one call of the workbench's shell, here in the sandbox, and notes it in the turn's
/// line. A command without a reason is refused, as a contract would; one that fails, or runs
/// out of time, is said as such to the model.
fn in_the_shell(shell: &Shell, did: &mut Ledger, call: &Call) -> Reply {
    let refused = |text: String| Reply {
        text: format!("Refused: {text}"),
        refused: true,
        asked: false,
        moved_on: false,
    };
    let args = match arguments(&call.arguments) {
        Ok(args) => args,
        Err(why) => {
            did.unreadable(SHELL);
            return refused(why);
        }
    };
    let command = args["command"].as_str().unwrap_or("").trim();
    let shown = cut(command);
    if command.is_empty() {
        did.ran("tried to run nothing, refused".to_owned());
        return refused("give the command to run, in `command`".to_owned());
    }
    if args["why"].as_str().is_none_or(|why| why.trim().is_empty()) {
        did.ran(format!("tried to run `{shown}`, refused"));
        return refused("say why you run it, in `why`".to_owned());
    }
    let within = Shell::within(args["seconds"].as_u64());
    let ran = match shell.run(command, within) {
        Ok(ran) => ran,
        Err(why) => {
            did.ran(format!("tried to run `{shown}`, failed"));
            return refused(why);
        }
    };
    let how = match (ran.timed_out, ran.code) {
        (true, _) => " (timed out)".to_owned(),
        (false, Some(0)) => String::new(),
        (false, Some(code)) => format!(" (exit {code})"),
        (false, None) => " (killed)".to_owned(),
    };
    did.ran(format!("ran `{shown}`{how}"));
    Reply {
        text: ran.said(within),
        refused: !ran.ok(),
        asked: false,
        moved_on: false,
    }
}

/// A command as the turn's line shows it: on one line, cut short.
fn cut(command: &str) -> String {
    let one_line: String = command
        .chars()
        .map(|c| if c.is_control() { ' ' } else { c })
        .collect();
    let one_line = one_line.trim();
    if one_line.chars().count() <= COMMAND_AT_MOST {
        return one_line.to_owned();
    }
    let kept: String = one_line.chars().take(COMMAND_AT_MOST).collect();
    format!("{kept}…")
}

/// Posts the model's last word in the task's thread, with `line`, saying what the turn
/// actually did, beside it. With no task, or no words, there is nothing to post.
pub(crate) fn last_word(
    hq: &mut Hq<'_>,
    task: Option<i64>,
    answer: &str,
    line: &str,
) -> Result<(), Stopped> {
    let Some(task) = task else {
        return Ok(());
    };
    if answer.is_empty() {
        return Ok(());
    }
    let text = beside(answer, line, POST_AT_MOST);
    // Marked as the worker's last word: HQ posts it, but does not count it as progress.
    let (said, refused) = hq.call(
        "post",
        &json!({ "task": task, "text": text, "last_word": true }),
    )?;
    if refused {
        return Err(Stopped::Failed(format!(
            "HQ refused the last word for task {task}: {said}"
        )));
    }
    Ok(())
}

/// How a tool call came back.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Outcome {
    /// It did what it was asked.
    Done,
    /// HQ, the node or the worker said no.
    Refused,
    /// It could not be done: the node unreachable, or failing.
    Failed,
}

/// The tool calls a turn actually made, and how each came back: what the line beside its
/// claims is written from. A model may say it restarted something it only looked at; this
/// is what the audit would show.
pub(crate) struct Ledger {
    /// The tools that only look.
    pub(crate) reads: Vec<String>,
    /// Each call made, in order: what it was, whether it only looked, and how it came back.
    pub(crate) calls: Vec<(String, bool, Outcome)>,
}

impl Ledger {
    /// The calls, as kept between the processes of a Claude Code turn (ADR 0117): the bridge
    /// writes them, the worker reads them for the line under the last word.
    pub(crate) fn kept(&self) -> Value {
        let calls: Vec<Value> = self
            .calls
            .iter()
            .map(|(said, read, outcome)| {
                let how = match outcome {
                    Outcome::Done => "done",
                    Outcome::Refused => "refused",
                    Outcome::Failed => "failed",
                };
                json!([said, read, how])
            })
            .collect();
        Value::Array(calls)
    }

    /// The calls read back from [`Self::kept`]; what does not read is left out.
    pub(crate) fn from_kept(kept: &Value) -> Self {
        let calls = kept
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|c| {
                let outcome = match c[2].as_str()? {
                    "done" => Outcome::Done,
                    "refused" => Outcome::Refused,
                    _ => Outcome::Failed,
                };
                Some((c[0].as_str()?.to_owned(), c[1].as_bool()?, outcome))
            })
            .collect();
        Self {
            reads: Vec::new(),
            calls,
        }
    }

    /// Notes a call that was made.
    fn note(&mut self, tool: &str, args: &Value, outcome: Outcome) {
        let (done, tried) = what_a_call_is(tool, args);
        let said = match outcome {
            Outcome::Done => done,
            Outcome::Refused => format!("tried to {tried}, refused"),
            Outcome::Failed => format!("tried to {tried}, failed"),
        };
        let read = self.reads.iter().any(|r| r == tool);
        self.calls.push((said, read, outcome));
    }

    /// Notes a command run in the workbench's shell, said as `said`: never a read, since a
    /// command may do what it likes inside the sandbox.
    fn ran(&mut self, said: String) {
        let outcome = if said.starts_with("ran ") {
            Outcome::Done
        } else {
            Outcome::Refused
        };
        self.calls.push((said, false, outcome));
    }

    /// Notes a call whose arguments could not be read, so it was never sent.
    fn unreadable(&mut self, tool: &str) {
        let read = self.reads.iter().any(|r| r == tool);
        self.calls.push((
            format!("tried to call {tool} with arguments that could not be read, refused"),
            read,
            Outcome::Refused,
        ));
    }

    /// The calls, one after another, the first [`NAMES_AT_MOST`] by name; `None` for none.
    fn list(&self) -> Option<String> {
        if self.calls.is_empty() {
            return None;
        }
        let mut named: Vec<&str> = self
            .calls
            .iter()
            .take(NAMES_AT_MOST)
            .map(|(said, _, _)| said.as_str())
            .collect();
        let more = self.calls.len().saturating_sub(NAMES_AT_MOST);
        let and_more = format!("and {more} more");
        if more > 0 {
            named.push(&and_more);
        }
        Some(named.join("; "))
    }

    /// Whether every call only looked.
    fn only_read(&self) -> bool {
        self.calls.iter().all(|(_, read, _)| *read)
    }

    /// The line under a turn's last word.
    pub(crate) fn line(&self) -> String {
        let Some(list) = self.list() else {
            return "This turn made no tool calls.".to_owned();
        };
        if self.only_read() {
            return format!("This turn did nothing but read: {list}.");
        }
        format!("This turn did: {list}.")
    }

    /// The line under a question to the owner.
    fn for_the_owner(&self) -> String {
        let Some(list) = self.list() else {
            return "What I actually did this turn: no tool calls.".to_owned();
        };
        if self.only_read() {
            return format!("What I actually did this turn: nothing but read: {list}.");
        }
        format!("What I actually did this turn: {list}.")
    }
}

/// A call in plain words: as done, and as tried. The tool is named, with its main argument.
fn what_a_call_is(tool: &str, args: &Value) -> (String, String) {
    let arg = |key: &str| -> String {
        let text = match &args[key] {
            Value::String(s) => s.trim().to_owned(),
            Value::Number(n) => n.to_string(),
            _ => String::new(),
        };
        text.chars().filter(|c| !c.is_control()).take(60).collect()
    };
    let both = |s: String| (s.clone(), s);
    match tool {
        "my_turn" => both("read what the turn is for".to_owned()),
        "homelab_status" => both("read the home lab's state".to_owned()),
        "container_logs" => both(format!("read {}'s log", arg("container"))),
        "restart_container" => (
            format!("restarted {}", arg("container")),
            format!("restart {}", arg("container")),
        ),
        "claim" => (
            format!("claimed task {}", arg("task")),
            format!("claim task {}", arg("task")),
        ),
        "post" => (
            format!("posted in task {}", arg("task")),
            format!("post in task {}", arg("task")),
        ),
        "set_status" => (
            format!("set task {} to {}", arg("task"), arg("status")),
            format!("set task {} to {}", arg("task"), arg("status")),
        ),
        "create_task" => (
            format!("created the task \"{}\"", arg("title")),
            format!("create the task \"{}\"", arg("title")),
        ),
        "check_back" => (
            format!("set a check-back in {} minutes", arg("minutes")),
            format!("set a check-back in {} minutes", arg("minutes")),
        ),
        "repo_checkout" => both("checked out the fork".to_owned()),
        "repo_submit" => (
            format!("submitted branch {}", arg("branch")),
            format!("submit branch {}", arg("branch")),
        ),
        _ => {
            let main = ["container", "item", "name", "id", "task", "query"]
                .iter()
                .map(|k| arg(k))
                .find(|v| !v.is_empty());
            let call = match main {
                Some(main) => format!("{tool} ({main})"),
                None => tool.to_owned(),
            };
            (format!("called {call}"), format!("call {call}"))
        }
    }
}

/// `text`, then a blank line and `line`, within `most` characters: the text is cut to make
/// room, never the line.
pub(crate) fn beside(text: &str, line: &str, most: usize) -> String {
    let room = most.saturating_sub(line.chars().count() + 2);
    let kept: String = text.trim().chars().take(room).collect();
    format!("{}\n\n{line}", kept.trim_end())
}

/// The words said, a paragraph each, in at most `most` characters: the newest kept when they
/// are too many, since they are the nearest to where the turn got to.
pub(crate) fn newest(words: &[String], most: usize) -> String {
    let all = words.join("\n\n");
    let count = all.chars().count();
    if count <= most {
        return all;
    }
    if most == 0 {
        return String::new();
    }
    let kept: String = all.chars().skip(count + 1 - most).collect();
    format!("…{}", kept.trim_start())
}

/// A question's arguments with what the turn actually did under its text.
fn with_what_was_done(args: &Value, did: &Ledger) -> Value {
    let mut sent = args.clone();
    if let Some(text) = args["text"].as_str() {
        sent["text"] = json!(beside(text, &did.for_the_owner(), ASK_AT_MOST));
    }
    sent
}

/// A reason as the worker may say it: never the token, and short. The token goes first, so
/// that cutting it short can never leave a piece of it.
pub(crate) fn quiet(why: &str, token: &str) -> String {
    let why = if token.is_empty() {
        why.to_owned()
    } else {
        why.replace(token, "[the turn's token]")
    };
    why.chars().take(SAYS_AT_MOST).collect()
}

/// An HTTP client that hands every status back to be read here, follows no redirect and goes
/// through no proxy: the token goes to the address it was given, or nowhere.
pub(crate) fn agent(within: Duration) -> ureq::Agent {
    ureq::Agent::config_builder()
        .timeout_global(Some(within))
        .http_status_as_error(false)
        .max_redirects(0)
        .proxy(None)
        .build()
        .into()
}

/// What HQ or the relay said with a refusal. Both put it in `error`: HQ as text, the relay as
/// OpenAI does, in its `message`.
fn what_it_said(body: &Value) -> String {
    let said = body["error"]
        .as_str()
        .or_else(|| body["error"]["message"].as_str())
        .unwrap_or("");
    if said.is_empty() {
        return String::new();
    }
    format!(": {said}")
}

/// Reads an answer from HQ or the relay (`who`): its JSON body (`null` when there is none), or
/// why the turn stops there.
fn read(
    who: &str,
    sent: Result<ureq::http::Response<ureq::Body>, ureq::Error>,
) -> Result<Value, Stopped> {
    let mut response =
        sent.map_err(|e| Stopped::Failed(format!("{who} could not be reached: {e}")))?;
    let status = response.status().as_u16();
    if matches!(status, 401 | 402) {
        return Err(Stopped::Over(format!("{who} answered {status}")));
    }
    let text = response
        .body_mut()
        .read_to_string()
        .map_err(|e| Stopped::Failed(format!("{who}'s answer could not be read: {e}")))?;
    let body = if text.trim().is_empty() {
        Value::Null
    } else {
        serde_json::from_str(&text).unwrap_or(Value::Null)
    };
    if !(200..300).contains(&status) {
        return Err(Stopped::Failed(format!(
            "{who} answered {status}{}",
            what_it_said(&body)
        )));
    }
    if body.is_null() && !text.trim().is_empty() {
        return Err(Stopped::Failed(format!("{who}'s answer is not JSON")));
    }
    Ok(body)
}

/// HQ's team protocol: JSON-RPC over HTTP, opened by the turn's token.
pub(crate) struct Hq<'a> {
    agent: ureq::Agent,
    url: &'a str,
    token: &'a str,
    /// The last request's id.
    id: u64,
    /// The protocol version HQ agreed to, sent with every message after `initialize`.
    version: Option<String>,
}

impl<'a> Hq<'a> {
    pub(crate) fn new(url: &'a str, token: &'a str) -> Self {
        Self {
            agent: agent(HQ_ANSWERS_WITHIN),
            url,
            token,
            id: 0,
            version: None,
        }
    }

    fn send(&self, message: &Value) -> Result<Value, Stopped> {
        let mut request = self
            .agent
            .post(self.url)
            .header("authorization", &format!("Bearer {}", self.token))
            .header("accept", "application/json");
        if let Some(version) = &self.version {
            request = request.header("mcp-protocol-version", version);
        }
        read("HQ", request.send_json(message))
    }

    /// A request, and its result.
    pub(crate) fn request(&mut self, method: &str, params: Value) -> Result<Value, Stopped> {
        self.id += 1;
        let reply = self.send(&json!({
            "jsonrpc": "2.0", "id": self.id, "method": method, "params": params,
        }))?;
        if let Some(message) = reply["error"]["message"].as_str() {
            return Err(Stopped::Failed(format!("HQ refused {method}: {message}")));
        }
        Ok(reply["result"].clone())
    }

    /// Opens the protocol: `initialize`, then the notification that the worker is ready.
    pub(crate) fn open(&mut self) -> Result<(), Stopped> {
        let agreed = self.request(
            "initialize",
            json!({
                "protocolVersion": PROTOCOL,
                "capabilities": {},
                "clientInfo": { "name": "endora-worker", "version": env!("CARGO_PKG_VERSION") },
            }),
        )?;
        let version = agreed["protocolVersion"].as_str().unwrap_or(PROTOCOL);
        self.version = Some(version.to_owned());
        self.send(&json!({ "jsonrpc": "2.0", "method": "notifications/initialized" }))?;
        Ok(())
    }

    /// Calls a tool: what it said, and whether that was a refusal.
    pub(crate) fn call(
        &mut self,
        tool: &str,
        arguments: &Value,
    ) -> Result<(String, bool), Stopped> {
        let result = self.request(
            "tools/call",
            json!({ "name": tool, "arguments": arguments }),
        )?;
        let text: Vec<&str> = result["content"]
            .as_array()
            .map(|items| items.iter().filter_map(|i| i["text"].as_str()).collect())
            .unwrap_or_default();
        Ok((
            text.join("\n"),
            result["isError"].as_bool().unwrap_or(false),
        ))
    }
}

/// The node's contracts (ADR 0106): listed as MCP lists tools, each called with its arguments
/// as a JSON body, opened by the same token.
pub(crate) struct Node<'a> {
    agent: ureq::Agent,
    url: &'a str,
    token: &'a str,
}

impl<'a> Node<'a> {
    pub(crate) fn new(url: &'a str, token: &'a str) -> Self {
        Self {
            agent: agent(HQ_ANSWERS_WITHIN),
            url: url.trim_end_matches('/'),
            token,
        }
    }

    /// The contracts offered. A node that offers none, or cannot say, leaves the turn to the
    /// team's tools: only a 401 or a 402 ends it.
    pub(crate) fn list(&self) -> Result<Vec<Value>, Stopped> {
        let sent = self
            .agent
            .get(self.url)
            .header("authorization", &format!("Bearer {}", self.token))
            .call();
        match read("the node", sent) {
            Ok(listed) => Ok(listed["tools"].as_array().cloned().unwrap_or_default()),
            Err(Stopped::Over(why)) => Err(Stopped::Over(why)),
            Err(Stopped::Failed(why)) => {
                eprintln!("worker: no contracts this turn: {why}");
                Ok(Vec::new())
            }
        }
    }

    /// Tells the node what the turn said, for HQ to keep a while (ADR 0129): the conversation as
    /// text, with the turn's token. It is the last thing a turn does and it never changes how
    /// the turn ended: a node that cannot be reached, or refuses, is said on stderr and the turn
    /// goes on its way.
    pub(crate) fn tell_conversation(&self, runtime: &str, text: &str) {
        let sent = self
            .agent
            .post(&crate::transcript::url(self.url))
            .header("authorization", &format!("Bearer {}", self.token))
            .send_json(json!({ "runtime": runtime, "text": crate::transcript::bounded(text) }));
        match sent {
            Ok(response) if response.status().is_success() => {}
            Ok(response) => eprintln!(
                "worker: its conversation was not kept: the node answered {}",
                response.status().as_u16()
            ),
            Err(e) => eprintln!(
                "worker: its conversation was not kept: {}",
                quiet(&e.to_string(), self.token)
            ),
        }
    }

    /// Calls a contract: what it said, how it came back (a 4xx is a refusal; a node that
    /// cannot be reached, or a 5xx, a failure), and whether the node put it on the owner's
    /// desk for approval, which ends the turn. Anything but a 401 or a 402 is the tool's own
    /// answer, for the model to read.
    fn call(&self, name: &str, arguments: &Value) -> Result<(String, Outcome, bool), Stopped> {
        let sent = self
            .agent
            .post(&format!("{}/{name}", self.url))
            .header("authorization", &format!("Bearer {}", self.token))
            .send_json(arguments);
        let mut response = match sent {
            Ok(response) => response,
            Err(e) => {
                return Ok((
                    format!("the node could not be reached: {e}"),
                    Outcome::Failed,
                    false,
                ));
            }
        };
        let status = response.status().as_u16();
        if matches!(status, 401 | 402) {
            return Err(Stopped::Over(format!("the node answered {status}")));
        }
        let body: Value = response
            .body_mut()
            .read_to_string()
            .ok()
            .and_then(|text| serde_json::from_str(&text).ok())
            .unwrap_or(Value::Null);
        if (200..300).contains(&status) {
            return Ok((
                body["text"].as_str().unwrap_or("").to_owned(),
                Outcome::Done,
                body["asked_owner"] == true,
            ));
        }
        let said = body["error"]
            .as_str()
            .map_or_else(|| format!("the node answered {status}"), str::to_owned);
        let outcome = if status >= 500 {
            Outcome::Failed
        } else {
            Outcome::Refused
        };
        Ok((said, outcome, false))
    }
}

/// The node's model relay, opened by the same token: OpenAI's chat completions to the house's
/// model, or Anthropic's Messages to Claude.
struct Model<'a> {
    agent: ureq::Agent,
    url: String,
    token: &'a str,
    model: &'a str,
    api: Api,
}

impl<'a> Model<'a> {
    fn new(setup: &'a Setup) -> Self {
        let base = setup.model_url.trim_end_matches('/');
        let url = match setup.api {
            Api::OpenAi => format!("{base}/chat/completions"),
            Api::Anthropic => format!("{base}/messages"),
        };
        Self {
            agent: agent(LONGEST_CALL),
            url,
            token: &setup.token,
            model: &setup.model,
            api: setup.api,
        }
    }

    /// One round: the conversation so far, and what the model said back.
    fn ask(&self, talk: &Talk) -> Result<Said, Stopped> {
        let mut request = self
            .agent
            .post(&self.url)
            .header("authorization", &format!("Bearer {}", self.token));
        if self.api == Api::Anthropic {
            request = request.header("anthropic-version", ANTHROPIC_VERSION);
        }
        let reply = read("the model relay", request.send_json(talk.body(self.model)))?;
        let no_message = || Stopped::Failed("the model relay's answer has no message".to_owned());
        if self.api == Api::OpenAi {
            let message = &reply["choices"][0]["message"];
            if !message.is_object() {
                return Err(no_message());
            }
            let calls: Vec<Value> = message["tool_calls"]
                .as_array()
                .cloned()
                .unwrap_or_default();
            return Ok(Said {
                text: message["content"].as_str().unwrap_or("").to_owned(),
                calls: calls
                    .iter()
                    .map(|c| Call {
                        id: c["id"].clone(),
                        name: c["function"]["name"].as_str().unwrap_or("").to_owned(),
                        arguments: c["function"]["arguments"].clone(),
                    })
                    .collect(),
                message: json!({ "role": "assistant", "content": message["content"],
                                 "tool_calls": calls }),
            });
        }
        let Some(content) = reply["content"].as_array() else {
            return Err(no_message());
        };
        let text: Vec<&str> = content
            .iter()
            .filter(|b| b["type"] == "text")
            .filter_map(|b| b["text"].as_str())
            .collect();
        let calls = content
            .iter()
            .filter(|b| b["type"] == "tool_use")
            .map(|b| Call {
                id: b["id"].clone(),
                name: b["name"].as_str().unwrap_or("").to_owned(),
                arguments: b["input"].clone(),
            })
            .collect();
        Ok(Said {
            text: text.join("\n"),
            calls,
            message: json!({ "role": "assistant", "content": content }),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use endora_team::application::{Author, Clock, Engine, NewTask, Settings};
    use endora_team::domain::{Priority, Role};
    use endora_team::infrastructure::{SimulatedRunner, SqliteTeamStore};

    struct Noon;
    impl Clock for Noon {
        fn now(&self) -> i64 {
            1_792_166_400_000
        }
    }

    #[test]
    fn the_words_on_the_way_keep_the_newest_when_they_are_too_many() {
        let words = ["Sonarr is down.".to_owned(), "Its disk is full.".to_owned()];
        assert_eq!(newest(&words, 100), "Sonarr is down.\n\nIts disk is full.");
        assert_eq!(newest(&words, 18), "…Its disk is full.");
        assert_eq!(newest(&words, 18).chars().count(), 18);
        assert_eq!(newest(&words, 0), "");
        assert_eq!(newest(&[], 10), "");
    }

    fn checked_out(branch: &str) -> repo::CheckedOut {
        repo::CheckedOut {
            path: "/work/repo".into(),
            branch: branch.to_owned(),
            head: "6efcf97c1954aaaaaaaa".to_owned(),
            files: 412,
        }
    }

    #[test]
    fn what_the_model_reads_after_a_checkout_depends_on_what_it_may_do_with_it() {
        let receipt = " (receipt r41)";
        // A developer on the fork: branch from the base, work, submit.
        let fresh = said_after_checkout(
            &checked_out("develop"),
            "develop",
            false,
            true,
            Some(7),
            receipt,
        );
        assert!(
            fresh.contains("git checkout -b hq/task-7-<slug> develop")
                && fresh.contains("repo_submit")
                && fresh.contains(receipt),
            "{fresh}"
        );
        // A developer on the team's kept branch: go on from it, rebase if the base moved,
        // submit again; it is not a fresh branch to cut.
        let kept = said_after_checkout(
            &checked_out("hq/task-7-add-pages"),
            "develop",
            true,
            true,
            Some(7),
            receipt,
        );
        assert!(
            kept.contains("kept branch hq/task-7-add-pages at 6efcf97c1954")
                && kept.contains("git rebase develop")
                && kept.contains("repo_submit"),
            "{kept}"
        );
        assert!(!kept.contains("git checkout -b"), "{kept}");
        // A tester: she reads it and cites the checkout; she submits and pushes nothing.
        let read = said_after_checkout(
            &checked_out("hq/task-7-add-pages"),
            "develop",
            true,
            false,
            Some(7),
            receipt,
        );
        assert!(
            read.contains("git diff develop..hq/task-7-add-pages")
                && read.contains("this checkout's receipt as a reading of your own")
                && read.contains(receipt),
            "{read}"
        );
        assert!(
            !read.contains("repo_submit") && !read.contains("git checkout -b"),
            "{read}"
        );
    }

    /// Every receipt the model is shown how to cite reads back as one in HQ: what the
    /// instructions teach, and what the node appends to each result. Task 687's tester cited
    /// "receipt r228" as she was taught and was refused for it three times.
    #[test]
    fn every_receipt_the_model_is_taught_to_cite_reads_back_as_evidence() {
        use endora_team::domain::evidence::Ref;
        let checkout_and_submit = ["repo_checkout".to_owned(), "repo_submit".to_owned()];
        let told = [
            instructions(true, &checkout_and_submit, true, true),
            instructions(true, &["repo_checkout".to_owned()], false, true),
        ]
        .join("\n");
        let taught: Vec<&str> = told
            .match_indices("(receipt ")
            .map(|(at, _)| {
                let from = &told[at + 1..];
                &from[..from.find(')').expect("the example is closed")]
            })
            .collect();
        assert!(taught.len() >= 2, "{told}");
        for cited in taught.into_iter().chain([" (receipt r41)"]) {
            assert!(
                matches!(Ref::parse(cited), Some(Ref::Receipt(_))),
                "{cited:?} is taught but not read as a receipt"
            );
        }
    }

    #[test]
    fn a_tester_is_told_the_wings_git_is_for_reading_and_a_developer_that_it_is_for_work() {
        let checkout_and_submit = ["repo_checkout".to_owned(), "repo_submit".to_owned()];
        let developer = instructions(true, &checkout_and_submit, false, true);
        assert!(
            developer.contains("then repo_submit that branch")
                && developer.contains("never recreate it")
                && developer.contains("recheck"),
            "{developer}"
        );
        // Checkout without submit is a tester's: a reading, whose receipt she cites.
        let tester = instructions(true, &["repo_checkout".to_owned()], false, true);
        assert!(
            tester.contains("for your reading")
                && tester.contains("that checkout's receipt")
                && tester.contains("as a reading of your own")
                && tester.contains("git diff"),
            "{tester}"
        );
        assert!(
            !tester.contains("repo_submit") && !tester.contains("recheck"),
            "she submits and rechecks nothing: {tester}"
        );
        // Neither offered: the wing is not mentioned.
        assert!(!instructions(true, &[], false, true).contains("wing's git"));
    }

    #[test]
    fn the_nodes_checkout_is_read_with_whether_it_is_a_kept_branch_and_its_receipt() {
        use base64::Engine as _;
        let bundle = base64::engine::general_purpose::STANDARD.encode(b"a bundle, as bytes");
        let fork =
            json!({ "branch": "develop", "head": "abc", "base": "develop", "bundle": bundle })
                .to_string();
        let handed = handed_over(&format!("{fork} (receipt r41)")).expect("a checkout");
        assert_eq!(
            (handed.branch.as_str(), handed.base.as_str(), handed.kept),
            ("develop", "develop", false)
        );
        assert_eq!(handed.bundle, b"a bundle, as bytes");
        assert_eq!(handed.receipt, " (receipt r41)");
        // The team's own kept branch says so, and says which base it sits on.
        let kept = json!({ "branch": "hq/task-7-x", "head": "abc", "base": "develop", "kept": true,
                           "bundle": bundle })
        .to_string();
        let handed = handed_over(&kept).expect("a kept checkout");
        assert_eq!(
            (handed.branch.as_str(), handed.base.as_str(), handed.kept),
            ("hq/task-7-x", "develop", true)
        );
        assert!(handed.receipt.is_empty());
        // No bundle, or no JSON, is no checkout; no base is the branch itself.
        assert!(handed_over("not json at all").is_none());
        assert!(handed_over(&json!({ "branch": "develop" }).to_string()).is_none());
        let bare = json!({ "branch": "develop", "bundle": bundle }).to_string();
        assert_eq!(handed_over(&bare).unwrap().base, "develop");
    }

    #[test]
    fn the_task_is_read_from_the_engines_own_briefing() {
        let hq = Engine::new(
            SqliteTeamStore::in_memory().unwrap(),
            SimulatedRunner::new(4),
            Noon,
            Settings::default(),
        );
        let dev = hq.hire("Jane", Role::Developer, 40, 2, 0).unwrap();
        let goal = hq
            .set_goal("Ship the watch tracker", "asked for", Author::Owner)
            .unwrap();
        // A title that looks like the line's own end, or a description that looks like the
        // line, does not fool it.
        for title in ["Add a list of watches", "Fix (#3, todo, normal) for good"] {
            hq.add_task(
                Author::Owner,
                NewTask {
                    goal,
                    project: None,
                    parent: None,
                    title: title.to_owned(),
                    description: "Your task: not this (#99, todo, normal)".to_owned(),
                    assignee: Some(dev),
                    priority: Priority::Normal,
                    waits_on: Vec::new(),
                },
            )
            .unwrap();
        }
        hq.tick().unwrap();
        let started = hq.runner().started();
        assert_eq!(started.len(), 2, "two slots, two turns");
        for turn in started {
            assert_eq!(task_of(&turn.told), turn.task.map(|t| t.0), "{}", turn.told);
        }
    }

    #[test]
    fn a_briefing_without_its_task_line_names_no_task() {
        assert_eq!(
            task_of(
                "You are Jane, the team's lead.\nThis turn is about no task: look over the team's work.\n"
            ),
            None
        );
        assert_eq!(
            task_of(
                "You are Jane, the team's developer.\nYour task: Add\na list (#7, todo, normal)\n"
            ),
            None,
            "a title over two lines is not guessed at"
        );
    }

    #[test]
    fn arguments_are_a_json_object_or_refused() {
        assert_eq!(
            arguments(&json!(r#"{"task": 7}"#)),
            Ok(json!({ "task": 7 }))
        );
        assert_eq!(arguments(&json!("")), Ok(json!({})), "no arguments at all");
        assert_eq!(arguments(&json!({ "task": 7 })), Ok(json!({ "task": 7 })));
        assert!(arguments(&json!(r#"{"task": "#)).is_err());
        assert!(arguments(&json!("[7]")).is_err());
        assert!(arguments(&Value::Null).is_err());
    }

    #[test]
    fn a_tool_with_no_input_schema_takes_an_empty_object() {
        assert_eq!(
            functions(&[json!({ "name": "my_turn" })]),
            [json!({ "type": "function", "function": {
                "name": "my_turn",
                "description": "",
                "parameters": { "type": "object", "properties": {} },
            } })]
        );
    }

    /// The node's container tools and one bridged read, as the node lists them.
    fn lab() -> Vec<String> {
        [
            "homelab_status",
            "container_logs",
            "restart_container",
            "nas_status",
        ]
        .map(str::to_owned)
        .to_vec()
    }

    #[test]
    fn the_home_labs_tools_are_named_as_the_turns_own() {
        let told = instructions(true, &lab(), false, false);
        assert!(
            told.contains(
                "The home lab tools are Endora's contracts, and this turn they are yours: \
                 homelab_status, container_logs, restart_container. Whatever your role, look \
                 with them yourself before you hand anything on, and never say you cannot read \
                 the home lab.\n- homelab_status"
            ),
            "{told}"
        );
        // Only what is offered is named: a cloud turn not given the logs is not told it has them.
        let told = instructions(
            true,
            &["homelab_status".to_owned(), "restart_container".to_owned()],
            false,
            false,
        );
        assert!(
            told.contains("they are yours: homelab_status, restart_container."),
            "{told}"
        );
        let told = instructions(true, &["weather".to_owned()], false, false);
        assert!(
            told.contains("The home lab tools are Endora's contracts:\n- homelab_status"),
            "{told}"
        );
        assert!(!told.contains("they are yours"), "{told}");
    }

    #[test]
    fn the_instructions_read_as_paragraphs_then_one_step_or_rule_a_line() {
        let told = instructions(true, &[], false, false);
        let parts: Vec<&str> = told.split("\n\n").collect();
        assert_eq!(parts.len(), 3, "{told}");
        assert!(!parts[0].contains('\n'), "{told}");
        let steps: Vec<&str> = parts[1].lines().collect();
        assert_eq!(steps[0], "Take your turn in these steps:");
        assert_eq!(steps.len(), 4, "{told}");
        for (n, step) in steps[1..].iter().enumerate() {
            assert!(step.starts_with(&format!("{}. ", n + 1)), "{step}");
        }
        assert!(told.contains("You cannot run commands"));
        assert!(
            told.contains("Each result of Endora's tools names its receipt (receipt r12)"),
            "{told}"
        );
        assert!(told.contains("a reading taken this turn, never memory"));
        assert!(!told.contains('\\') && !told.contains("  "), "{told}");
        assert!(told.ends_with("one or two sentences. That reply goes in your task's thread."));
        assert!(instructions(false, &[], false, false).ends_with("one or two sentences."));
        assert!(!told.contains("tell_story"), "{told}");
        // With the node's contracts, a paragraph more: what they may do, and what is the
        // owner's, one rule a line.
        let told = instructions(false, &lab(), false, false);
        let parts: Vec<&str> = told.split("\n\n").collect();
        assert_eq!(parts.len(), 5, "{told}");
        // And how the lab is built (ADR 0112), the house log's text, as a paragraph.
        assert_eq!(parts[3], endora_kernel::lab::lab_built_paragraph());
        let rules: Vec<&str> = parts[2].lines().collect();
        assert_eq!(rules.len(), 9, "{told}");
        assert!(
            told.contains(
                "- An alarm is a moment, not the state now: check homelab_status before you \
                 act, since what alarmed may be back up (Endora says so in the thread). If \
                 everything has recovered, say so and close the task: a routine's check as \
                 done, other work moved on with set_status."
            ),
            "{told}"
        );
        assert!(
            told.contains(
                "- Report something as a problem only when a tool shows it causing harm: an item \
                 stuck in a queue, a container down or unhealthy, a failure that repeats. Errors \
                 or odd lines in a log are evidence to explain, not problems in themselves. \
                 Normal operation (e.g. an RSS sync that finds releases and grabs none) is not a \
                 problem."
            ),
            "{told}"
        );
        assert!(told.contains("goes to the team's reviewer first"), "{told}");
        assert!(told.contains("owner's approval"), "{told}");
        // A request for the owner's yes says which of the task's criteria it serves, or it is
        // refused and never reaches the desk (ADR 0124, amended).
        assert!(
            told.contains(
                "an action that needs the owner's approval says which of your task's criteria it \
                 serves (serves), then goes on their desk and ends your turn"
            ),
            "{told}"
        );
        assert!(rules[1..].iter().all(|r| r.starts_with("- ")));
        assert!(told.contains("restart_container"));
        assert!(
            told.contains("After a fix, check back to confirm it held"),
            "{told}"
        );
        assert!(told.contains("a routine's check"), "{told}");
        assert!(!told.contains("  "), "{told}");
        // A member offered tell_story tells the team's lessons as stories: a paragraph of its
        // own.
        let told = instructions(true, &lab(), true, false);
        let parts: Vec<&str> = told.split("\n\n").collect();
        assert_eq!(parts.len(), 6, "{told}");
        assert_eq!(
            parts[4],
            "When the owner corrects you, or you learn something about this house that the team \
             will need again, tell it as a story with tell_story (when it matters, the tempting \
             move, what to do instead, and why) so nobody asks twice; cite a story by its handle \
             rather than retelling it."
        );
        assert!(told.ends_with("That reply goes in your task's thread."));
    }

    #[test]
    fn in_the_workbench_the_instructions_give_the_shell_a_paragraph() {
        let told = instructions(true, &lab(), true, true);
        let parts: Vec<&str> = told.split("\n\n").collect();
        assert_eq!(parts.len(), 7, "{told}");
        assert!(!told.contains("You cannot run commands"), "{told}");
        assert!(
            parts[1].contains(
                "1. Look: use the tools to find out what you need, the shell among them."
            ),
            "{told}"
        );
        assert_eq!(
            parts[4],
            "You have a real shell in a disposable sandbox: run commands, git and python there \
             with the shell tool, and say why. The sandbox is offline: neither the internet nor \
             the house's services are reachable from the shell, so use the house through the \
             team's tools. Anything you leave in /work is gone when the turn ends: write \
             durable findings in the task's thread, or as a story with tell_story."
        );
        assert!(!told.contains("  ") && !told.contains('\\'), "{told}");
        let told = instructions(true, &[], false, true);
        assert!(
            told.contains("durable findings in the task's thread.\n\n"),
            "no tell_story for a member without it: {told}"
        );
    }

    #[test]
    fn a_command_on_the_turns_line_is_one_line_cut_short() {
        assert_eq!(cut("  git status  "), "git status");
        assert_eq!(cut("cd repo\ngit log"), "cd repo git log");
        let long = "x".repeat(COMMAND_AT_MOST + 1);
        assert_eq!(cut(&long), format!("{}…", "x".repeat(COMMAND_AT_MOST)));
        assert_eq!(
            cut(&"é".repeat(COMMAND_AT_MOST)),
            "é".repeat(COMMAND_AT_MOST)
        );
    }

    #[test]
    fn a_reason_never_carries_the_token_not_even_cut_short() {
        let token = "secret-token-of-this-turn";
        let why = format!("{}Bearer {token}", "x".repeat(SAYS_AT_MOST - 10));
        let said = quiet(&why, token);
        assert!(!said.contains("sec"), "{said}");
        assert!(said.chars().count() <= SAYS_AT_MOST);
        assert_eq!(quiet("HQ answered 500", ""), "HQ answered 500");
    }
}
