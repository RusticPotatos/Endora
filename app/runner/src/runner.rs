//! The runner's loop (spec section 15, phase 4, item 5). Every two seconds it kills the turns
//! HQ asks to stop, reports and removes the ones that ended (killing any past thirty
//! minutes), and while it has room asks HQ for the next turn and starts it in a fresh
//! container. HQ names only the run and its token; everything else about the container is
//! the runner's own, so HQ cannot choose the image, the mounts or the network.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::docker::{CHECK_LABEL, Docker, LEASE_LABEL, RUN_LABEL, State};
use crate::hq::{Ending, Hq, HqError};

/// The longest a turn's container runs before it is killed, in seconds.
pub const LONGEST_TURN: u64 = 30 * 60;

/// The longest a check job's container runs before it is killed, in seconds (ADR 0115): three
/// checks of five minutes each, and the fetch.
pub const LONGEST_CHECK: u64 = 20 * 60;

/// The exit code of a container killed, as Docker reports one.
const KILLED: i64 = 137;

/// How many of a worker's last lines on stderr go into its ending.
const STDERR_LINES: usize = 8;

/// The most characters of a worker's last words that go to HQ.
const LONGEST_WHY: usize = 400;

/// How long a run of letters, digits, `_` and `-` must be to be taken for a token (a turn's
/// is 64 hex characters, the runner's 32 or more) and taken out of what goes to HQ.
const TOKEN_LENGTH: usize = 24;

/// What a worker's exit code says (see `endora-worker`): 0, a turn taken; 3, the turn was
/// over before it ended; 1, it failed; -1, its container never started; 137, killed, after
/// thirty minutes when the runner killed it.
pub fn meaning(code: Option<i64>, past_time: bool) -> String {
    match code {
        None => "its container was gone before it was seen to end".to_owned(),
        Some(0) => "taken".to_owned(),
        Some(-1) => "its container never started".to_owned(),
        Some(KILLED) if past_time => "killed after thirty minutes".to_owned(),
        Some(KILLED) => "killed (exit 137)".to_owned(),
        Some(code) => format!("exit {code}"),
    }
}

/// `text` with every token-shaped word (24 or more letters, digits, `_` and `-` together)
/// replaced by `<token>`: a worker never prints its token, and what it prints goes to HQ
/// without one all the same.
pub fn scrubbed(text: &str) -> String {
    let mut out = String::new();
    let mut word = String::new();
    let flush = |word: &mut String, out: &mut String| {
        if word.chars().count() >= TOKEN_LENGTH {
            out.push_str("<token>");
        } else {
            out.push_str(word);
        }
        word.clear();
    };
    for c in text.chars() {
        if c.is_ascii_alphanumeric() || c == '_' || c == '-' {
            word.push(c);
            continue;
        }
        flush(&mut word, &mut out);
        out.push(c);
    }
    flush(&mut word, &mut out);
    out
}

/// Why a turn ended, for HQ: what its exit code means and then the worker's last words on
/// stderr (`tail`), scrubbed of tokens, one line after another, the last [`LONGEST_WHY`]
/// characters of them when there are more.
pub fn why(meaning: &str, tail: &str) -> String {
    let tail = scrubbed(tail);
    let lines: Vec<&str> = tail
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();
    if lines.is_empty() {
        return meaning.to_owned();
    }
    let said = lines.join("; ");
    let count = said.chars().count();
    if count <= LONGEST_WHY {
        return format!("{meaning}: {said}");
    }
    let kept: String = said.chars().skip(count - LONGEST_WHY).collect();
    format!("{meaning}: …{kept}")
}

/// What every turn's container is, whatever HQ says.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Work {
    /// The worker image: Endora's own, with `endora-worker` in it.
    pub image: String,
    /// The fenced network the worker is on, which reaches only HQ and the node.
    pub network: String,
    /// The node's relay to the house's own model, as the worker reaches it.
    pub model_url: String,
    /// The node's Anthropic relay, on the same team listener, for a member HQ says thinks with
    /// Claude: the fence is the same either way.
    pub cloud_url: String,
    /// HQ's team protocol, as the worker reaches it.
    pub hq_mcp: String,
    /// How many turns run at once.
    pub capacity: usize,
    /// Whether each turn is a workbench (ADR 0110, ring 1 on the Pi): the worker gets a
    /// writable `/work` of its own for a shell and git, offline, and more room to use it.
    pub workbench: bool,
    /// Where a check job fetches its bundle (ADR 0115), up to the check's id: the node's team
    /// listener, `.../team-relay/check/`.
    pub check_bundle: String,
    /// Where a check job reports, up to the check's id: HQ's `/runner/check/`, through the
    /// gateway when the runner goes through it.
    pub check_done: String,
}

/// A check job HQ handed out (ADR 0115), once checked: the branch is in the environment for
/// the log; the bundle itself is fetched by the job with its token.
pub struct CheckJob {
    id: u64,
    token: String,
    branch: String,
}

impl CheckJob {
    /// Checks a check job as HQ handed it out: `kind: "check"`, a numeric id and a token of
    /// 64 hex characters.
    ///
    /// # Errors
    /// Why it is refused, in a line.
    pub fn from_hq(handed: &Value) -> Result<Self, String> {
        if handed["kind"] != "check" {
            return Err("not a check".to_owned());
        }
        let id = handed["check"]
            .as_u64()
            .ok_or_else(|| "a check from HQ has no numeric id".to_owned())?;
        let token = handed["token"].as_str().unwrap_or("");
        if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(format!("check {id}: its token is not 64 hex characters"));
        }
        let branch = handed["branch"].as_str().unwrap_or("").to_owned();
        if branch.chars().any(char::is_control) || branch.len() > 200 {
            return Err(format!("check {id}: its branch is not a name"));
        }
        Ok(Self {
            id,
            token: token.to_owned(),
            branch,
        })
    }
}

/// The body of `POST /containers/create` for a check job (ADR 0115): the workbench image with
/// the same fence as a turn, no model and no turn (no run token, no HQ, no relay), the job's
/// own token and where to fetch and report, and nothing else.
pub fn check_spec(work: &Work, job: &CheckJob) -> Value {
    let env = vec![
        "ENDORA_CHECK=1".to_owned(),
        format!("ENDORA_CHECK_ID={}", job.id),
        format!("ENDORA_CHECK_TOKEN={}", job.token),
        format!(
            "ENDORA_CHECK_BUNDLE_URL={}{}/bundle",
            work.check_bundle, job.id
        ),
        format!("ENDORA_CHECK_DONE_URL={}{}/done", work.check_done, job.id),
        "ENDORA_WORKBENCH=1".to_owned(),
    ];
    json!({
        "Image": work.image,
        "Entrypoint": ["endora-worker"],
        "Env": env,
        "Labels": { CHECK_LABEL: job.id.to_string(), "endora.branch": job.branch },
        "HostConfig": {
            "NetworkMode": work.network,
            "Memory": 1024 * 1024 * 1024,
            "NanoCpus": 2_000_000_000_i64,
            "PidsLimit": 256,
            "ReadonlyRootfs": true,
            "CapDrop": ["ALL"],
            "SecurityOpt": ["no-new-privileges:true"],
            "Tmpfs": { "/tmp": "size=256m", "/data": "size=1m", "/work": WORK_FOLDER },
            "Dns": [NO_RESOLVER],
        },
    })
}

/// A workbench's `/work`: in memory, gone with the container, owned by the image's user.
const WORK_FOLDER: &str = "size=256m,uid=10001,gid=10001,mode=0700";

/// A turn HQ handed out, once checked. Not `Debug`: it holds the turn's token.
pub struct Turn {
    run: u64,
    lease: u64,
    token: String,
    member: String,
    role: String,
    task: Option<u64>,
    /// The Claude model the member thinks with, or `None` for the house's own.
    cloud: Option<String>,
    /// How hard Claude tries this turn, as HQ decided, or `None` for the model's own.
    effort: Option<String>,
    /// Whether Claude Code runs it (ADR 0117), started by the worker in the workbench.
    claude_code: bool,
    /// Whether it may have the workbench's shell where the runner has one: not the check after
    /// a deploy, as HQ says (ADR 0115, amended). An older HQ says nothing: it may.
    shell: bool,
}

/// Why a turn HQ handed out was refused.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Refused {
    /// Its run is not a number: there is nothing to report it as.
    NoRun,
    /// It has no lease: HQ would take no report of it.
    NoLease(u64),
    /// Its token is not 64 hex characters.
    Token(u64, u64),
    /// Its brain is neither `local` nor a Claude model's name.
    Brain(u64, u64),
    /// Its effort is not one of Anthropic's five.
    Effort(u64, u64),
    /// Its runtime is neither `worker` nor `claude-code`, or Claude Code for a brain that is
    /// not Claude (ADR 0117).
    Runtime(u64, u64),
}

/// The efforts a turn may name: Anthropic's own, each of which goes into the worker's
/// environment as it is.
const EFFORTS: [&str; 5] = ["low", "medium", "high", "xhigh", "max"];

/// The effort a turn names, `None` for none, or `Err` for anything but one of [`EFFORTS`].
fn effort_of(effort: &Value) -> Result<Option<String>, ()> {
    match effort {
        Value::Null => Ok(None),
        Value::String(e) if EFFORTS.contains(&e.as_str()) => Ok(Some(e.clone())),
        _ => Err(()),
    }
}

/// The Claude model a turn's brain names, `None` for the house's own model, or `Err` for a
/// brain that is neither. Only a plain model name passes (`claude-`, then lowercase letters,
/// digits and dashes, at most 64 in all): it goes into the worker's environment, and which
/// model a role may use is the relay's to decide.
fn cloud_of(brain: &Value) -> Result<Option<String>, ()> {
    let name = match brain {
        Value::Null => return Ok(None),
        Value::String(name) => name.as_str(),
        _ => return Err(()),
    };
    // The house's own model, the owner's Mac (ADR 0130) and DeepSeek Flash (the owner,
    // 2026-10-28): all through the node's local relay, which sends a Mac member's calls to the
    // Mac and a DeepSeek member's to DeepSeek, and to nothing else.
    if name == "local" || name == "mac" || name == "deepseek-flash" {
        return Ok(None);
    }
    let Some(rest) = name.strip_prefix("claude-") else {
        return Err(());
    };
    let plain = rest
        .bytes()
        .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit() || b == b'-');
    if rest.is_empty() || !plain || name.len() > 64 {
        return Err(());
    }
    Ok(Some(name.to_owned()))
}

impl Turn {
    /// Checks a turn as HQ handed it out.
    ///
    /// # Errors
    /// [`Refused`] if its run or lease is not a number, its token not 64 hex characters, or
    /// its brain not one the runner can give the worker.
    pub fn from_hq(handed: &Value) -> Result<Self, Refused> {
        let Some(run) = handed["run"].as_u64() else {
            return Err(Refused::NoRun);
        };
        let Some(lease) = handed["lease"].as_u64() else {
            return Err(Refused::NoLease(run));
        };
        let token = handed["token"].as_str().unwrap_or("");
        if token.len() != 64 || !token.bytes().all(|b| b.is_ascii_hexdigit()) {
            return Err(Refused::Token(run, lease));
        }
        let Ok(cloud) = cloud_of(&handed["brain"]) else {
            return Err(Refused::Brain(run, lease));
        };
        let Ok(effort) = effort_of(&handed["effort"]) else {
            return Err(Refused::Effort(run, lease));
        };
        // What runs it (ADR 0117): the worker unless HQ says Claude Code, which speaks only
        // Anthropic's API, so only for a Claude brain. An older HQ says nothing: the worker.
        let claude_code = match &handed["runtime"] {
            Value::Null => false,
            Value::String(r) if r == "worker" => false,
            Value::String(r) if r == "claude-code" && cloud.is_some() => true,
            _ => return Err(Refused::Runtime(run, lease)),
        };
        let text = |key: &str| handed[key].as_str().unwrap_or("").to_owned();
        Ok(Self {
            run,
            lease,
            token: token.to_owned(),
            member: text("member"),
            role: text("role"),
            task: handed["task"].as_u64(),
            cloud,
            effort,
            claude_code,
            shell: handed["shell"].as_bool().unwrap_or(true),
        })
    }
}

/// The body of `POST /containers/create` for a turn: the worker image and the fixed fence,
/// with the turn's own run, lease and token and nothing else of HQ's but its brain, which
/// picks one of the runner's own two relays (and names the model and its effort, for
/// Claude).
pub fn spec(work: &Work, turn: &Turn) -> Value {
    let mut env = vec![
        format!("ENDORA_RUN_TOKEN={}", turn.token),
        format!("ENDORA_HQ_MCP={}", work.hq_mcp),
    ];
    match &turn.cloud {
        None => env.push(format!("ENDORA_MODEL_URL={}", work.model_url)),
        Some(model) => {
            env.push(format!("ENDORA_MODEL_URL={}", work.cloud_url));
            env.push(format!("ENDORA_MODEL={model}"));
            if let Some(effort) = &turn.effort {
                env.push(format!("ENDORA_EFFORT={effort}"));
            }
        }
    }
    // 256 MB, one CPU, 128 processes; a workbench a gigabyte (its `/work` is memory too),
    // two CPUs and 256 processes, for a shell, git and a build.
    let (memory, cpus, pids) = if work.workbench {
        (1024 * 1024 * 1024, 2_000_000_000_i64, 256)
    } else {
        (256 * 1024 * 1024, 1_000_000_000_i64, 128)
    };
    // The image declares a `/data` volume, which would otherwise be an unbounded writable
    // disk: a megabyte in memory instead, gone with the container.
    let mut tmpfs = json!({ "/tmp": "size=64m", "/data": "size=1m" });
    // The workbench's shell and git, unless HQ says the turn has none (ADR 0115, amended):
    // then the worker offers neither, in the same fence.
    let shell = work.workbench && turn.shell;
    if work.workbench {
        tmpfs["/work"] = json!(WORK_FOLDER);
    }
    if shell {
        env.push("ENDORA_WORKBENCH=1".to_owned());
    }
    // Claude Code (ADR 0117) is in the workbench image alone; the worker starts it there.
    if turn.claude_code && shell {
        env.push("ENDORA_RUNTIME=claude-code".to_owned());
    }
    let mut spec = json!({
        "Image": work.image,
        "Entrypoint": ["endora-worker"],
        "Env": env,
        "Labels": { RUN_LABEL: turn.run.to_string(), LEASE_LABEL: turn.lease.to_string() },
        "HostConfig": {
            "NetworkMode": work.network,
            "Memory": memory,
            "NanoCpus": cpus,
            "PidsLimit": pids,
            "ReadonlyRootfs": true,
            "CapDrop": ["ALL"],
            "SecurityOpt": ["no-new-privileges:true"],
            "Tmpfs": tmpfs,
        },
    });
    if work.workbench {
        // Docker's own resolver forwards a sandbox's lookups through the host, past the
        // network's egress rules, so a lookup could carry data out ("<secret>.example.com").
        // Pointed at an address that never answers (TEST-NET-1), no name resolves; the
        // workbench reaches the gateway by address, and needs none.
        spec["HostConfig"]["Dns"] = json!([NO_RESOLVER]);
    }
    spec
}

/// An address that never answers (RFC 5737's TEST-NET-1): a sandbox's DNS server.
const NO_RESOLVER: &str = "192.0.2.1";

/// A turn's container the runner is watching.
struct Running {
    run: u64,
    lease: u64,
    container: String,
    /// When it started, in seconds since the Unix epoch.
    since: u64,
}

/// A check job's container the runner is watching (ADR 0115).
struct Checking {
    id: u64,
    token: String,
    container: String,
    since: u64,
}

/// The runner: HQ on one side, Docker on the other.
pub struct Runner<D> {
    docker: D,
    hq: Hq,
    work: Work,
    running: Vec<Running>,
    checking: Vec<Checking>,
    /// The runs HQ asked to stop whose containers are not yet killed. HQ tells each once, so
    /// a kill that fails is tried again every round until it takes.
    stopping: BTreeSet<u64>,
    /// The trouble last said about each thing, so one that lasts is said once, not every
    /// two seconds.
    said: BTreeMap<&'static str, String>,
}

impl<D: Docker> Runner<D> {
    /// A runner with nothing running.
    pub fn new(docker: D, hq: Hq, work: Work) -> Self {
        Self {
            docker,
            hq,
            work,
            running: Vec::new(),
            checking: Vec::new(),
            stopping: BTreeSet::new(),
            said: BTreeMap::new(),
        }
    }

    /// Takes back the turns' containers an earlier runner left, to report and remove like
    /// its own.
    pub fn take_back(&mut self) {
        let left = match self.docker.left() {
            Ok(left) => left,
            Err(e) => {
                eprintln!("runner: could not look for turns left running: {e}");
                return;
            }
        };
        for container in left {
            // A check's container (ADR 0115) reports for itself; one left behind is removed,
            // and HQ gives up a check that never reported.
            if !container.check.is_empty() {
                println!(
                    "runner: check {} was left by an earlier runner; removed",
                    container.check
                );
                if let Err(e) = self.docker.remove(&container.id) {
                    eprintln!("runner: could not remove it: {e}");
                }
                continue;
            }
            let Ok(run) = container.run.parse::<u64>() else {
                continue;
            };
            // One with no lease is reported under none, which HQ refuses; it is removed all
            // the same.
            let lease = container.lease.parse::<u64>().unwrap_or(0);
            println!("runner: run {run} was left by an earlier runner; taken back");
            self.running.push(Running {
                run,
                lease,
                container: container.id,
                since: container.created,
            });
        }
    }

    /// One round: stops, endings, then new turns. `now` is seconds since the Unix epoch.
    pub fn step(&mut self, now: u64) {
        self.stop_asked();
        self.check(now);
        self.watch_checks(now);
        self.fill(now);
    }

    /// How many turns and checks run now.
    fn busy(&self) -> usize {
        self.running.len() + self.checking.len()
    }

    /// Removes each check job's container that ended (the job reported for itself; one that
    /// ended badly without reporting is reported failed with its token), and kills one past
    /// [`LONGEST_CHECK`].
    fn watch_checks(&mut self, now: u64) {
        for job in std::mem::take(&mut self.checking) {
            let state = match self.docker.inspect(&job.container) {
                Ok(state) => state,
                Err(e) => {
                    self.trouble("Docker", &e);
                    self.checking.push(job);
                    continue;
                }
            };
            let failed = match state {
                Some(State::Running) if now.saturating_sub(job.since) > LONGEST_CHECK => {
                    if let Err(e) = self.docker.kill(&job.container) {
                        eprintln!(
                            "runner: could not kill check {}, past its time: {e}",
                            job.id
                        );
                        self.checking.push(job);
                        continue;
                    }
                    Some("the check ran past twenty minutes and was killed".to_owned())
                }
                Some(State::Running) => {
                    self.checking.push(job);
                    continue;
                }
                Some(State::Exited(0)) | None => None,
                Some(State::Exited(code)) => Some(format!(
                    "the check ended with exit {code} without reporting"
                )),
            };
            if let Some(why) = failed {
                println!("runner: check {} failed: {why}", job.id);
                if let Err(e) = self.hq.check_failed(job.id, &job.token, &why) {
                    eprintln!("runner: HQ did not take check {}'s failure: {e}", job.id);
                }
            } else {
                println!("runner: check {} ended", job.id);
            }
            if let Err(e) = self.docker.remove(&job.container) {
                eprintln!("runner: could not remove check {}'s container: {e}", job.id);
            }
        }
    }

    /// Starts a check job in a fresh container, or reports it failed with its token.
    fn start_check(&mut self, job: CheckJob, now: u64) {
        let started = self
            .docker
            .create(&check_spec(&self.work, &job))
            .and_then(|container| match self.docker.start(&container) {
                Ok(()) => Ok(container),
                Err(e) => {
                    if let Err(e) = self.docker.remove(&container) {
                        eprintln!("runner: could not remove check {}'s container: {e}", job.id);
                    }
                    Err(e)
                }
            });
        match started {
            Ok(container) => {
                println!("runner: check {} started on {:?}", job.id, job.branch);
                self.checking.push(Checking {
                    id: job.id,
                    token: job.token,
                    container,
                    since: now,
                });
            }
            Err(e) => {
                eprintln!("runner: could not start check {}: {e}", job.id);
                let why = format!("the check never started: {}", scrubbed(&e));
                if let Err(e) = self.hq.check_failed(job.id, &job.token, &why) {
                    eprintln!("runner: HQ did not take check {}'s failure: {e}", job.id);
                }
            }
        }
    }

    /// Kills the turns HQ asks to stop, and any an earlier round could not. Each is then
    /// reported and removed as it ends.
    fn stop_asked(&mut self) {
        match self.hq.stops() {
            Ok(runs) => {
                self.fine("HQ");
                self.stopping.extend(runs);
            }
            // The stops already heard are still killed.
            Err(e) => self.trouble("HQ", &e.to_string()),
        }
        let running = &self.running;
        self.stopping
            .retain(|run| running.iter().any(|t| t.run == *run));
        let mut failed = None;
        for turn in self
            .running
            .iter()
            .filter(|t| self.stopping.contains(&t.run))
        {
            match self.docker.kill(&turn.container) {
                Ok(()) => {}
                Err(e) => failed = Some(format!("could not stop run {}: {e}", turn.run)),
            }
        }
        let Some(failed) = failed else {
            self.stopping.clear();
            return;
        };
        self.trouble("Docker", &failed);
    }

    /// Reports each turn that ended, and removes it; kills one that ran too long.
    fn check(&mut self, now: u64) {
        for turn in std::mem::take(&mut self.running) {
            let state = match self.docker.inspect(&turn.container) {
                Ok(state) => state,
                Err(e) => {
                    self.trouble("Docker", &e);
                    self.running.push(turn);
                    continue;
                }
            };
            self.fine("Docker");
            let (code, past_time) = match state {
                Some(State::Running) if now.saturating_sub(turn.since) > LONGEST_TURN => {
                    if let Err(e) = self.docker.kill(&turn.container) {
                        eprintln!(
                            "runner: could not kill run {}, past its time: {e}",
                            turn.run
                        );
                        self.running.push(turn);
                        continue;
                    }
                    println!("runner: run {} ran past thirty minutes; killed", turn.run);
                    (Some(KILLED), true)
                }
                Some(State::Running) => {
                    self.running.push(turn);
                    continue;
                }
                Some(State::Exited(code)) => (Some(code), false),
                // Removed by someone else: how it ended is not known, so not a success.
                None => (None, false),
            };
            let ending = self.ending(turn.run, &turn.container, code, past_time);
            // Told before it is removed: while HQ cannot hear, the container keeps how it
            // ended. An answer of any kind is hearing.
            if let Err(e @ HqError::Unreachable(_)) = self.hq.ended(turn.run, turn.lease, &ending) {
                self.trouble("HQ", &e.to_string());
                self.running.push(turn);
                continue;
            }
            if let Err(e) = self.docker.remove(&turn.container) {
                eprintln!("runner: could not remove run {}'s container: {e}", turn.run);
            }
            if ending.succeeded {
                println!("runner: run {} ended, taken", turn.run);
                continue;
            }
            println!("runner: run {} failed: {}", turn.run, ending.why);
        }
    }

    /// How a turn ended, in words HQ keeps on its record: what the exit code means and, for
    /// one that did not succeed, the worker's last words on stderr, read before the container
    /// is removed and its log with it. A log Docker will not give leaves the meaning alone.
    fn ending(&self, run: u64, container: &str, code: Option<i64>, past_time: bool) -> Ending {
        let meaning = meaning(code, past_time);
        let succeeded = code == Some(0);
        if succeeded || code.is_none() {
            return Ending {
                succeeded,
                code,
                why: meaning,
            };
        }
        let tail = match self.docker.stderr_tail(container, STDERR_LINES) {
            Ok(tail) => tail,
            Err(e) => {
                eprintln!("runner: could not read run {run}'s log: {e}");
                String::new()
            }
        };
        Ending {
            succeeded,
            code,
            why: why(&meaning, &tail),
        }
    }

    /// While there is room, asks HQ for the next turn (or check job) and starts it.
    fn fill(&mut self, now: u64) {
        let room = self.work.capacity.saturating_sub(self.busy());
        if room == 0 {
            return;
        }
        // A turn is taken from HQ only when Docker answers, so none fails for want of it.
        if let Err(e) = self.docker.ping() {
            return self.trouble("Docker", &e);
        }
        self.fine("Docker");
        for _ in 0..room {
            let free = self.work.capacity.saturating_sub(self.busy());
            let handed = match self.hq.next(free) {
                Ok(Some(handed)) => handed,
                Ok(None) => return,
                Err(e) => return self.trouble("HQ", &e.to_string()),
            };
            // A check job (ADR 0115) comes before any turn: model-free, with its own token.
            if handed["kind"] == "check" {
                match CheckJob::from_hq(&handed) {
                    Ok(job) => self.start_check(job, now),
                    Err(why) => eprintln!("runner: refused a check from HQ: {why}"),
                }
                continue;
            }
            match Turn::from_hq(&handed) {
                // Claude Code is in the workbench image alone: elsewhere the turn is refused,
                // never quietly run on the worker instead (the pilot compares the two).
                Ok(turn) if turn.claude_code && !self.work.workbench => {
                    self.refused(
                        turn.run,
                        turn.lease,
                        "Claude Code runs only in the workbench",
                    );
                }
                Ok(turn) => self.start(&turn, now),
                Err(Refused::NoRun) => {
                    eprintln!("runner: refused a turn from HQ whose run is not a number");
                }
                Err(Refused::NoLease(run)) => {
                    eprintln!("runner: refused run {run}: it came with no lease");
                }
                Err(Refused::Token(run, lease)) => {
                    self.refused(run, lease, "its token is not 64 hex characters");
                }
                Err(Refused::Brain(run, lease)) => {
                    self.refused(run, lease, "its brain is not a model's name");
                }
                Err(Refused::Effort(run, lease)) => {
                    self.refused(run, lease, "its effort is not one of the five");
                }
                Err(Refused::Runtime(run, lease)) => {
                    self.refused(run, lease, "its runtime is not one the runner can start");
                }
            }
        }
    }

    /// Starts a turn in a fresh container, or reports it failed.
    fn start(&mut self, turn: &Turn, now: u64) {
        let container = match self.docker.create(&spec(&self.work, turn)) {
            Ok(container) => container,
            Err(e) => {
                eprintln!("runner: could not make run {}'s container: {e}", turn.run);
                return self.never_ran(turn.run, turn.lease, &e);
            }
        };
        if let Err(e) = self.docker.start(&container) {
            eprintln!("runner: could not start run {}: {e}", turn.run);
            if let Err(e) = self.docker.remove(&container) {
                eprintln!("runner: could not remove run {}'s container: {e}", turn.run);
            }
            return self.never_ran(turn.run, turn.lease, &e);
        }
        println!(
            "runner: run {} started: {:?}, {:?}, task {}",
            turn.run,
            turn.member,
            turn.role,
            turn.task
                .map_or_else(|| "none".to_owned(), |t| t.to_string())
        );
        self.running.push(Running {
            run: turn.run,
            lease: turn.lease,
            container,
            since: now,
        });
    }

    /// Refuses a turn HQ handed out, saying why, and reports it as one that never ran.
    fn refused(&mut self, run: u64, lease: u64, why: &str) {
        eprintln!("runner: refused run {run}: {why}");
        self.never_ran(run, lease, why);
    }

    /// Reports a turn whose container never started, with what went wrong. If HQ cannot hear
    /// it, the engine's longest-turn limit ends it.
    fn never_ran(&mut self, run: u64, lease: u64, what: &str) {
        let ending = Ending {
            succeeded: false,
            code: Some(-1),
            why: why(&meaning(Some(-1), false), what),
        };
        match self.hq.ended(run, lease, &ending) {
            Ok(()) => {}
            Err(e @ HqError::Unreachable(_)) => self.trouble("HQ", &e.to_string()),
            Err(e) => eprintln!("runner: HQ did not take run {run}'s failure: {e}"),
        }
    }

    /// Says a trouble once, not every two seconds while it lasts.
    fn trouble(&mut self, about: &'static str, what: &str) {
        if self.said.get(about).is_some_and(|said| said == what) {
            return;
        }
        eprintln!("runner: {what}");
        self.said.insert(about, what.to_owned());
    }

    /// A trouble is over.
    fn fine(&mut self, about: &'static str) {
        if self.said.remove(about).is_some() {
            println!("runner: {about} answers again");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::docker::Left;
    use std::collections::VecDeque;
    use std::io::{Read as _, Write as _};
    use std::sync::{Arc, Mutex};

    const RUNNER_TOKEN: &str = "a-runner-token-of-at-least-thirty-two-characters";

    /// When the tests' turns start.
    const T0: u64 = 1_700_000_000;

    /// A turn's token: 64 hex characters, its run's own.
    fn token(run: u64) -> String {
        format!("{run:064x}")
    }

    /// A turn's lease, as HQ hands it out: not its run, so the two cannot be mixed up unseen.
    const fn lease(run: u64) -> u64 {
        run + 1000
    }

    /// A turn as HQ hands it out.
    fn turn(run: u64) -> Value {
        json!({ "run": run, "lease": lease(run), "token": token(run), "member": "John Doe", "role": "developer", "task": 4 })
    }

    /// HQ, pretending: hands out its turns in order, asks for its stops, and keeps what it
    /// was asked and told.
    struct FakeHq {
        url: String,
        turns: Arc<Mutex<VecDeque<Value>>>,
        stops: Arc<Mutex<Vec<u64>>>,
        seen: Arc<Mutex<Vec<(String, Value)>>>,
    }

    impl FakeHq {
        fn new(turns: Vec<Value>) -> Self {
            let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            let url = format!("http://{}", listener.local_addr().unwrap());
            let turns = Arc::new(Mutex::new(VecDeque::from(turns)));
            let stops = Arc::new(Mutex::new(Vec::new()));
            let seen = Arc::new(Mutex::new(Vec::new()));
            let (to_hand, to_stop, kept) = (turns.clone(), stops.clone(), seen.clone());
            std::thread::spawn(move || {
                for stream in listener.incoming() {
                    let Ok(mut stream) = stream else { continue };
                    let mut head = Vec::new();
                    let mut byte = [0u8];
                    while !head.ends_with(b"\r\n\r\n") {
                        if stream.read_exact(&mut byte).is_err() {
                            break;
                        }
                        head.push(byte[0]);
                    }
                    let head = String::from_utf8_lossy(&head).into_owned();
                    let mut words = head.split(' ');
                    let asked = format!(
                        "{} {}",
                        words.next().unwrap_or(""),
                        words.next().unwrap_or("")
                    );
                    let header = |name: &str| {
                        head.lines()
                            .filter_map(|l| l.split_once(": "))
                            .find(|(k, _)| k.eq_ignore_ascii_case(name))
                            .map(|(_, v)| v.to_owned())
                    };
                    let length: usize = header("content-length")
                        .and_then(|v| v.parse().ok())
                        .unwrap_or(0);
                    let mut body = vec![0u8; length];
                    let _ = stream.read_exact(&mut body);
                    let body: Value = serde_json::from_slice(&body).unwrap_or(Value::Null);
                    let bearer = header("authorization").unwrap_or_default();
                    let signed = bearer == format!("Bearer {RUNNER_TOKEN}");
                    // A check's report (ADR 0115) is signed with the job's own token.
                    let a_check = asked.starts_with("POST /v1/runner/check/");
                    let (status, reply) = match asked.as_str() {
                        a if a_check => {
                            let id: u64 = a
                                .trim_start_matches("POST /v1/runner/check/")
                                .trim_end_matches("/done")
                                .parse()
                                .unwrap_or(0);
                            if bearer == format!("Bearer {}", check_token(id)) {
                                (204, Value::Null)
                            } else {
                                (404, json!({ "error": "no check has that token" }))
                            }
                        }
                        _ if !signed => (401, json!({ "error": "not the runner" })),
                        "POST /v1/runner/next" => match to_hand.lock().unwrap().pop_front() {
                            Some(turn) => (200, turn),
                            None => (204, Value::Null),
                        },
                        // Each stop told once, as HQ tells it.
                        "GET /v1/runner/stops" => (
                            200,
                            json!({ "runs": std::mem::take(&mut *to_stop.lock().unwrap()) }),
                        ),
                        a if a.starts_with("POST /v1/runner/") && a.ends_with("/ended") => {
                            (200, json!({}))
                        }
                        _ => (404, json!({ "error": "no such route" })),
                    };
                    kept.lock().unwrap().push((asked, body));
                    let reply = if status == 204 {
                        String::new()
                    } else {
                        reply.to_string()
                    };
                    let _ = write!(
                        stream,
                        "HTTP/1.1 {status} X\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{reply}",
                        reply.len()
                    );
                }
            });
            Self {
                url,
                turns,
                stops,
                seen,
            }
        }

        /// How many places the runner said it had, each time it asked for a turn.
        fn asked_with(&self) -> Vec<u64> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .filter(|(a, _)| a == "POST /v1/runner/next")
                .filter_map(|(_, b)| b["free"].as_u64())
                .collect()
        }

        /// What it was told of how runs ended. Each report must quote its run's lease.
        fn told(&self) -> Vec<(u64, bool)> {
            self.endings()
                .into_iter()
                .map(|(run, b)| (run, b["succeeded"].as_bool().unwrap()))
                .collect()
        }

        /// Each check reported failed by the runner itself, with why.
        fn check_failures(&self) -> Vec<(u64, String)> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .filter_map(|(a, b)| {
                    let id = a
                        .strip_prefix("POST /v1/runner/check/")?
                        .strip_suffix("/done")?
                        .parse()
                        .ok()?;
                    Some((id, b["summary"].as_str().unwrap_or("").to_owned()))
                })
                .collect()
        }

        /// Each report of how a run ended, whole.
        fn endings(&self) -> Vec<(u64, Value)> {
            self.seen
                .lock()
                .unwrap()
                .iter()
                .filter_map(|(a, b)| {
                    let run = a
                        .strip_prefix("POST /v1/runner/")?
                        .strip_suffix("/ended")?
                        .parse()
                        .ok()?;
                    assert_eq!(b["lease"].as_u64(), Some(lease(run)), "run {run}'s lease");
                    Some((run, b.clone()))
                })
                .collect()
        }
    }

    /// What the pretend Docker holds.
    #[derive(Default)]
    struct Pretend {
        made: u32,
        specs: Vec<Value>,
        containers: BTreeMap<String, State>,
        calls: Vec<String>,
        left: Vec<Left>,
        /// Docker cannot be reached.
        down: bool,
        /// How many kills fail before they work again.
        kills_fail: u32,
        /// What each container's worker wrote to stderr.
        stderr: BTreeMap<String, String>,
        /// The containers whose log Docker will not give.
        no_log: BTreeSet<String>,
    }

    /// Docker, pretending; a clone sees the same containers.
    #[derive(Clone, Default)]
    struct FakeDocker(Arc<Mutex<Pretend>>);

    impl FakeDocker {
        fn calls(&self) -> Vec<String> {
            self.0.lock().unwrap().calls.clone()
        }
        fn specs(&self) -> Vec<Value> {
            self.0.lock().unwrap().specs.clone()
        }
        fn containers(&self) -> Vec<String> {
            self.0.lock().unwrap().containers.keys().cloned().collect()
        }
        /// A container's worker ends with `code`.
        fn exits(&self, id: &str, code: i64) {
            self.0
                .lock()
                .unwrap()
                .containers
                .insert(id.to_owned(), State::Exited(code));
        }
        /// A container's worker ends with `code`, having written `stderr`.
        fn exits_saying(&self, id: &str, code: i64, stderr: &str) {
            self.exits(id, code);
            self.0
                .lock()
                .unwrap()
                .stderr
                .insert(id.to_owned(), stderr.to_owned());
        }
    }

    impl Docker for FakeDocker {
        fn ping(&self) -> Result<(), String> {
            if self.0.lock().unwrap().down {
                return Err("Docker cannot be reached".to_owned());
            }
            Ok(())
        }
        fn create(&self, spec: &Value) -> Result<String, String> {
            let mut docker = self.0.lock().unwrap();
            if docker.down {
                return Err("Docker cannot be reached".to_owned());
            }
            docker.made += 1;
            let id = format!("c{}", docker.made);
            docker.specs.push(spec.clone());
            docker.containers.insert(id.clone(), State::Exited(-1));
            docker.calls.push(format!("create {id}"));
            Ok(id)
        }
        fn start(&self, id: &str) -> Result<(), String> {
            let mut docker = self.0.lock().unwrap();
            docker.containers.insert(id.to_owned(), State::Running);
            docker.calls.push(format!("start {id}"));
            Ok(())
        }
        fn inspect(&self, id: &str) -> Result<Option<State>, String> {
            Ok(self.0.lock().unwrap().containers.get(id).copied())
        }
        fn kill(&self, id: &str) -> Result<(), String> {
            let mut docker = self.0.lock().unwrap();
            docker.calls.push(format!("kill {id}"));
            if docker.kills_fail > 0 {
                docker.kills_fail -= 1;
                return Err("Docker is busy".to_owned());
            }
            if docker.containers.get(id) == Some(&State::Running) {
                docker.containers.insert(id.to_owned(), State::Exited(137));
            }
            Ok(())
        }
        fn stderr_tail(&self, id: &str, lines: usize) -> Result<String, String> {
            let mut docker = self.0.lock().unwrap();
            docker.calls.push(format!("logs {id}"));
            if docker.no_log.contains(id) {
                return Err("Docker would not give its log (500)".to_owned());
            }
            if !docker.containers.contains_key(id) {
                return Err("Docker would not give its log (404)".to_owned());
            }
            let all = docker.stderr.get(id).cloned().unwrap_or_default();
            let kept: Vec<&str> = all.lines().rev().take(lines).collect();
            Ok(kept.into_iter().rev().map(|l| format!("{l}\n")).collect())
        }
        fn remove(&self, id: &str) -> Result<(), String> {
            let mut docker = self.0.lock().unwrap();
            docker.containers.remove(id);
            docker.calls.push(format!("remove {id}"));
            Ok(())
        }
        fn left(&self) -> Result<Vec<Left>, String> {
            Ok(self.0.lock().unwrap().left.clone())
        }
    }

    /// The runner's token file, as HQ would have made it.
    fn token_file(name: &str) -> std::path::PathBuf {
        let dir = std::env::temp_dir().join(format!("endora-runner-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("token");
        std::fs::write(&file, format!("{RUNNER_TOKEN}\n")).unwrap();
        file
    }

    fn work(capacity: usize) -> Work {
        Work {
            image: "endora-node:latest".to_owned(),
            network: "endora_team".to_owned(),
            model_url: "http://node:8788/team-relay/local/v1".to_owned(),
            cloud_url: "http://node:8788/team-relay/v1".to_owned(),
            hq_mcp: "http://hq:8790/v2/team/mcp".to_owned(),
            capacity,
            workbench: false,
            check_bundle: "http://node:8788/team-relay/check/".to_owned(),
            check_done: "http://hq:8790/v1/runner/check/".to_owned(),
        }
    }

    /// A check job's token: 64 hex characters, the job's own.
    fn check_token(id: u64) -> String {
        format!("{id:032x}{id:032x}")
    }

    /// A check job as HQ hands it out.
    fn check_job(id: u64) -> Value {
        json!({ "kind": "check", "check": id, "token": check_token(id), "task": 3,
                "branch": "hq/task-3-tidy", "head": "a1b2c3d4e5f6" })
    }

    fn runner(name: &str, hq: &FakeHq, docker: &FakeDocker, capacity: usize) -> Runner<FakeDocker> {
        let hq = Hq::new(&hq.url, token_file(name)).unwrap();
        Runner::new(docker.clone(), hq, work(capacity))
    }

    #[test]
    fn a_handed_turn_becomes_one_fenced_container_with_its_own_env_and_nothing_of_hqs() {
        let mut handed = turn(12);
        // What HQ cannot choose, however it asks.
        handed["image"] = json!("someone/else:latest");
        handed["network"] = json!("host");
        handed["mounts"] = json!(["/:/host"]);
        handed["env"] = json!(["ENDORA_MODEL_URL=http://192.168.1.10:11434/v1"]);
        let hq = FakeHq::new(vec![handed]);
        let docker = FakeDocker::default();
        let mut runner = runner("spec", &hq, &docker, 2);
        runner.step(T0);

        assert_eq!(docker.calls(), ["create c1", "start c1"]);
        assert_eq!(
            docker.specs(),
            [json!({
                "Image": "endora-node:latest",
                "Entrypoint": ["endora-worker"],
                "Env": [
                    format!("ENDORA_RUN_TOKEN={}", token(12)),
                    "ENDORA_HQ_MCP=http://hq:8790/v2/team/mcp",
                    "ENDORA_MODEL_URL=http://node:8788/team-relay/local/v1",
                ],
                "Labels": { "endora.run": "12", "endora.lease": "1012" },
                "HostConfig": {
                    "NetworkMode": "endora_team",
                    "Memory": 268_435_456,
                    "NanoCpus": 1_000_000_000,
                    "PidsLimit": 128,
                    "ReadonlyRootfs": true,
                    "CapDrop": ["ALL"],
                    "SecurityOpt": ["no-new-privileges:true"],
                    "Tmpfs": { "/tmp": "size=64m", "/data": "size=1m" },
                },
            })]
        );
        assert_eq!(
            hq.asked_with(),
            [2, 1],
            "asked again with the place left, and HQ had nothing more"
        );
        assert!(hq.told().is_empty(), "nothing has ended");
    }

    /// The check after a deploy (ADR 0115, amended): HQ says its turn has no shell, and the
    /// runner starts it in the same fence without the workbench's shell, git or Claude Code: the
    /// worker offers none, whatever the model asks. An older HQ says nothing: as before.
    #[test]
    fn a_turn_handed_without_a_shell_is_started_without_one() {
        let gateway = "http://192.168.1.10:8788";
        let pi = Work {
            image: "endora-workbench:latest".to_owned(),
            network: "endora_sandbox".to_owned(),
            model_url: format!("{gateway}/team-relay/local/v1"),
            cloud_url: format!("{gateway}/team-relay/v1"),
            hq_mcp: format!("{gateway}/team-gateway/v2/mcp"),
            capacity: 1,
            workbench: true,
            check_bundle: format!("{gateway}/team-relay/check/"),
            check_done: format!("{gateway}/team-gateway/runner/check/"),
        };
        let env_of = |handed: &Value| {
            let started = spec(&pi, &Turn::from_hq(handed).expect("a turn"));
            (
                started["Env"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .filter_map(|e| e.as_str().map(str::to_owned))
                    .collect::<Vec<_>>(),
                started["HostConfig"]["NetworkMode"].clone(),
                started["HostConfig"]["Dns"].clone(),
            )
        };
        let mut checking = turn(12);
        checking["shell"] = json!(false);
        checking["brain"] = json!("claude-haiku-5-5");
        checking["runtime"] = json!("claude-code");
        let (env, network, dns) = env_of(&checking);
        assert!(
            !env.iter()
                .any(|e| e.starts_with("ENDORA_WORKBENCH") || e.starts_with("ENDORA_RUNTIME")),
            "{env:?}"
        );
        assert_eq!(network, "endora_sandbox", "the same fence");
        assert_eq!(dns, json!([NO_RESOLVER]), "the same fence");
        let (env, _, _) = env_of(&turn(13));
        assert!(env.contains(&"ENDORA_WORKBENCH=1".to_owned()), "{env:?}");
        let mut with = turn(14);
        with["shell"] = json!(true);
        let (env, _, _) = env_of(&with);
        assert!(env.contains(&"ENDORA_WORKBENCH=1".to_owned()), "{env:?}");
    }

    #[test]
    fn a_workbench_turn_has_its_own_offline_work_folder_and_reaches_only_the_gateway() {
        let mut handed = turn(12);
        handed["env"] = json!(["HTTPS_PROXY=http://192.168.1.10:3128"]);
        let hq = FakeHq::new(vec![handed]);
        let docker = FakeDocker::default();
        let gateway = "http://192.168.1.10:8788";
        let pi = Work {
            image: "endora-workbench:latest".to_owned(),
            network: "endora_sandbox".to_owned(),
            model_url: format!("{gateway}/team-relay/local/v1"),
            cloud_url: format!("{gateway}/team-relay/v1"),
            hq_mcp: format!("{gateway}/team-gateway/v2/mcp"),
            capacity: 1,
            workbench: true,
            check_bundle: format!("{gateway}/team-relay/check/"),
            check_done: format!("{gateway}/team-gateway/runner/check/"),
        };
        let mut runner = Runner::new(
            docker.clone(),
            Hq::new(&hq.url, token_file("workbench")).unwrap(),
            pi,
        );
        runner.step(T0);
        assert_eq!(
            docker.specs(),
            [json!({
                "Image": "endora-workbench:latest",
                "Entrypoint": ["endora-worker"],
                "Env": [
                    format!("ENDORA_RUN_TOKEN={}", token(12)),
                    "ENDORA_HQ_MCP=http://192.168.1.10:8788/team-gateway/v2/mcp",
                    "ENDORA_MODEL_URL=http://192.168.1.10:8788/team-relay/local/v1",
                    "ENDORA_WORKBENCH=1",
                ],
                "Labels": { "endora.run": "12", "endora.lease": "1012" },
                "HostConfig": {
                    "NetworkMode": "endora_sandbox",
                    // A gigabyte, two CPUs, 256 processes: room for a shell, git and a build,
                    // the work folder's memory included.
                    "Memory": 1_073_741_824,
                    "NanoCpus": 2_000_000_000,
                    "PidsLimit": 256,
                    "ReadonlyRootfs": true,
                    "CapDrop": ["ALL"],
                    "SecurityOpt": ["no-new-privileges:true"],
                    "Tmpfs": {
                        "/tmp": "size=64m",
                        "/data": "size=1m",
                        "/work": "size=256m,uid=10001,gid=10001,mode=0700",
                    },
                    // Docker's own resolver forwards a sandbox's lookups through the host, past
                    // the network's egress rules: a lookup can carry data out. Pointed at an
                    // address that never answers, no name resolves.
                    "Dns": ["192.0.2.1"],
                },
            })]
        );
    }

    #[test]
    fn a_claude_code_turn_runs_in_the_workbench_alone_and_only_on_claude() {
        let gateway = "http://192.168.1.10:8788";
        let bench = |workbench: bool| Work {
            image: "endora-workbench:latest".to_owned(),
            network: "endora_sandbox".to_owned(),
            model_url: format!("{gateway}/team-relay/local/v1"),
            cloud_url: format!("{gateway}/team-relay/v1"),
            hq_mcp: format!("{gateway}/team-gateway/v2/mcp"),
            capacity: 4,
            workbench,
            check_bundle: format!("{gateway}/team-relay/check/"),
            check_done: format!("{gateway}/team-gateway/runner/check/"),
        };
        let handed = |run: u64, brain: &str, runtime: Value| {
            let mut t = turn(run);
            t["brain"] = json!(brain);
            t["runtime"] = runtime;
            t
        };
        let hq = FakeHq::new(vec![
            handed(1, "claude-haiku-5-5", json!("claude-code")),
            handed(2, "claude-haiku-5-5", json!("worker")),
            handed(3, "local", json!("claude-code")),
            handed(4, "claude-haiku-5-5", json!("codex")),
        ]);
        let docker = FakeDocker::default();
        let mut runner = Runner::new(
            docker.clone(),
            Hq::new(&hq.url, token_file("claude-code")).unwrap(),
            bench(true),
        );
        runner.step(T0);
        let envs: Vec<Vec<String>> = docker
            .specs()
            .iter()
            .map(|s| {
                s["Env"]
                    .as_array()
                    .unwrap()
                    .iter()
                    .map(|e| e.as_str().unwrap().to_owned())
                    .collect()
            })
            .collect();
        assert_eq!(envs.len(), 2, "two started: {envs:?}");
        assert!(envs[0].contains(&"ENDORA_RUNTIME=claude-code".to_owned()));
        assert!(envs[0].contains(&"ENDORA_MODEL=claude-haiku-5-5".to_owned()));
        assert!(
            !envs[1].iter().any(|e| e.starts_with("ENDORA_RUNTIME")),
            "the worker is the default"
        );
        let endings = hq.endings();
        let why: Vec<(u64, String)> = endings
            .iter()
            .map(|(run, e)| (*run, e["why"].as_str().unwrap().to_owned()))
            .collect();
        assert_eq!(
            why,
            [
                (
                    3,
                    "its container never started: its runtime is not one the runner can start"
                        .to_owned()
                ),
                (
                    4,
                    "its container never started: its runtime is not one the runner can start"
                        .to_owned()
                ),
            ]
        );
        // Outside the workbench there is no Claude Code: refused, never run on the worker.
        let hq = FakeHq::new(vec![handed(5, "claude-haiku-5-5", json!("claude-code"))]);
        let docker = FakeDocker::default();
        let mut runner = Runner::new(
            docker.clone(),
            Hq::new(&hq.url, token_file("claude-code-nas")).unwrap(),
            bench(false),
        );
        runner.step(T0);
        assert!(docker.specs().is_empty());
        assert_eq!(
            hq.endings()[0].1["why"],
            "its container never started: Claude Code runs only in the workbench"
        );
    }

    #[test]
    fn an_exited_container_is_reported_with_its_exit_code_and_removed() {
        let hq = FakeHq::new(vec![turn(1), turn(2), turn(3)]);
        let docker = FakeDocker::default();
        let mut runner = runner("exited", &hq, &docker, 3);
        runner.step(T0);
        docker.exits("c1", 0);
        docker.exits("c2", 3);
        runner.step(T0 + 2);

        assert_eq!(hq.told(), [(1, true), (2, false)]);
        assert_eq!(docker.containers(), ["c3"], "the ended ones are removed");
        assert!(docker.calls().contains(&"remove c1".to_owned()));
        assert!(docker.calls().contains(&"remove c2".to_owned()));
        runner.step(T0 + 4);
        assert_eq!(hq.told().len(), 2, "each is told once");
    }

    #[test]
    fn a_failed_turns_exit_code_and_last_words_reach_hq_without_its_token() {
        let hq = FakeHq::new(vec![turn(1), turn(2), turn(3), turn(4)]);
        let docker = FakeDocker::default();
        let mut runner = runner("why", &hq, &docker, 4);
        runner.step(T0);
        docker.exits_saying("c1", 0, "worker: asked the owner, which ends the turn\n");
        // What a worker says when HQ ended its turn under it, with the token it never prints
        // and a container id, as a bad day might print them.
        docker.exits_saying(
            "c2",
            3,
            &format!(
                "round 1\nround 2\n\nworker: the turn is over: HQ answered 401 ({} on c0ffee_1234)\n",
                token(2)
            ),
        );
        docker.exits_saying("c3", 1, "worker: the model answered 500\n");
        docker.0.lock().unwrap().no_log.insert("c3".to_owned());
        docker.exits("c4", 1);
        runner.step(T0 + 2);

        let endings = hq.endings();
        let of = |run: u64| endings.iter().find(|(r, _)| *r == run).unwrap().1.clone();
        assert_eq!(
            of(1),
            json!({ "succeeded": true, "lease": lease(1), "code": 0, "why": "taken" }),
            "a taken turn's words are not read"
        );
        let over = of(2);
        assert_eq!(over["succeeded"], false);
        assert_eq!(over["code"], 3);
        assert_eq!(
            over["why"],
            "exit 3: round 1; round 2; worker: the turn is over: HQ answered 401 (<token> on c0ffee_1234)"
        );
        assert!(
            !over.to_string().contains(&token(2)),
            "the turn's token never goes to HQ"
        );
        assert_eq!(
            of(3)["why"],
            "exit 1",
            "a log Docker will not give leaves the exit code's meaning"
        );
        assert_eq!(of(4)["why"], "exit 1", "a worker that said nothing");
        assert!(
            docker.calls().iter().all(|c| c != "logs c1"),
            "a taken turn's log is not read"
        );
        assert!(
            docker.containers().is_empty(),
            "each is removed all the same"
        );
    }

    #[test]
    fn a_turn_the_runner_refuses_is_reported_as_one_that_never_started_and_why() {
        let mut odd = turn(7);
        odd["brain"] = json!("gpt-5");
        let hq = FakeHq::new(vec![odd]);
        let docker = FakeDocker::default();
        let mut runner = runner("refused-why", &hq, &docker, 1);
        runner.step(T0);
        let endings = hq.endings();
        assert_eq!(endings.len(), 1);
        let (run, refused) = &endings[0];
        assert_eq!(*run, 7);
        assert_eq!(refused["succeeded"], false);
        assert_eq!(refused["code"], -1);
        assert_eq!(
            refused["why"],
            "its container never started: its brain is not a model's name"
        );
    }

    #[test]
    fn a_workers_words_are_scrubbed_of_tokens_and_kept_to_their_last_four_hundred() {
        assert_eq!(
            scrubbed(&format!("Bearer {} x", token(2))),
            "Bearer <token> x",
            "a turn's 64 hex characters"
        );
        assert_eq!(
            scrubbed("a-runner-token-of-at-least-thirty-two-characters, said HQ"),
            "<token>, said HQ"
        );
        assert_eq!(
            scrubbed("http://192.168.1.10:8788/team-relay/local/v1 answered 502"),
            "http://192.168.1.10:8788/team-relay/local/v1 answered 502",
            "ordinary words and addresses stay"
        );
        assert_eq!(why("exit 1", ""), "exit 1");
        assert_eq!(why("exit 1", "\n  \n"), "exit 1");
        assert_eq!(
            why("exit 1", "one\ntwo\n"),
            "exit 1: one; two",
            "line after line"
        );
        let long = format!("{}end", "x ".repeat(300));
        let kept = why("exit 1", &long);
        assert!(kept.starts_with("exit 1: …"), "{kept}");
        assert!(kept.ends_with("x end"), "the last words are the ones kept");
        assert_eq!(kept.chars().count(), "exit 1: …".chars().count() + 400);
    }

    #[test]
    fn a_stop_kills_its_turn_and_only_its_turn() {
        let hq = FakeHq::new(vec![turn(12), turn(13)]);
        let docker = FakeDocker::default();
        let mut runner = runner("stop", &hq, &docker, 2);
        runner.step(T0);
        hq.stops.lock().unwrap().extend([12, 99]);
        runner.step(T0 + 2);

        let calls = docker.calls();
        assert!(calls.contains(&"kill c1".to_owned()));
        assert!(!calls.contains(&"kill c2".to_owned()), "13 was not asked");
        assert_eq!(hq.told(), [(12, false)], "a killed turn did not succeed");
        assert_eq!(docker.containers(), ["c2"]);
    }

    #[test]
    fn a_stop_whose_kill_failed_is_tried_again_though_hq_tells_it_once() {
        let hq = FakeHq::new(vec![turn(12)]);
        let docker = FakeDocker::default();
        let mut runner = runner("retry", &hq, &docker, 1);
        runner.step(T0);
        hq.stops.lock().unwrap().push(12);
        docker.0.lock().unwrap().kills_fail = 1;
        runner.step(T0 + 2);
        assert_eq!(docker.containers(), ["c1"], "the kill failed");
        assert!(hq.told().is_empty());

        runner.step(T0 + 4);
        let kills = docker.calls().iter().filter(|c| *c == "kill c1").count();
        assert_eq!(kills, 2, "tried again");
        assert_eq!(hq.told(), [(12, false)]);
        assert!(docker.containers().is_empty());
        runner.step(T0 + 6);
        assert_eq!(
            docker.calls().iter().filter(|c| *c == "kill c1").count(),
            2,
            "once it took, never again"
        );
    }

    #[test]
    fn a_turn_past_thirty_minutes_is_killed_and_reported_failed() {
        let hq = FakeHq::new(vec![turn(5)]);
        let docker = FakeDocker::default();
        let mut runner = runner("long", &hq, &docker, 1);
        runner.step(T0);
        runner.step(T0 + 29 * 60);
        assert!(hq.told().is_empty(), "29 minutes is allowed");
        assert_eq!(docker.containers(), ["c1"]);

        docker
            .0
            .lock()
            .unwrap()
            .stderr
            .insert("c1".to_owned(), "worker: round 11 of 12\n".to_owned());
        runner.step(T0 + 31 * 60);
        assert!(docker.calls().contains(&"kill c1".to_owned()));
        assert_eq!(hq.told(), [(5, false)]);
        let (_, ending) = &hq.endings()[0];
        assert_eq!(ending["code"], 137);
        assert_eq!(
            ending["why"], "killed after thirty minutes: worker: round 11 of 12",
            "HQ hears it was the runner, and what the worker was at"
        );
        assert!(docker.containers().is_empty(), "and removed");
    }

    #[test]
    fn a_check_job_runs_the_workbench_model_free_with_its_own_token_and_is_removed_when_done() {
        let hq = FakeHq::new(vec![check_job(9), turn(12)]);
        let docker = FakeDocker::default();
        let mut runner = runner("check", &hq, &docker, 2);
        runner.step(T0);
        let specs = docker.specs();
        assert_eq!(specs.len(), 2, "the check, then the turn");
        let check = &specs[0];
        assert_eq!(
            check["Env"],
            json!([
                "ENDORA_CHECK=1",
                "ENDORA_CHECK_ID=9",
                format!("ENDORA_CHECK_TOKEN={}", check_token(9)),
                "ENDORA_CHECK_BUNDLE_URL=http://node:8788/team-relay/check/9/bundle",
                "ENDORA_CHECK_DONE_URL=http://hq:8790/v1/runner/check/9/done",
                "ENDORA_WORKBENCH=1",
            ]),
            "no run token, no HQ, no model: a job token and two addresses"
        );
        assert_eq!(check["Labels"][CHECK_LABEL], "9");
        assert_eq!(check["Labels"][RUN_LABEL], Value::Null);
        assert_eq!(check["HostConfig"]["NetworkMode"], "endora_team");
        assert_eq!(check["HostConfig"]["ReadonlyRootfs"], true);
        assert_eq!(check["HostConfig"]["Dns"], json!([NO_RESOLVER]));
        assert!(check["HostConfig"]["Tmpfs"]["/work"].is_string());
        assert_eq!(specs[1]["Labels"][RUN_LABEL], "12");
        assert_eq!(hq.asked_with(), [2, 1], "a check takes a place");
        // The job reports for itself: when its container ends well, it is only removed.
        docker.exits("c1", 0);
        runner.step(T0 + 10);
        assert_eq!(docker.containers(), ["c2"], "the check's container is gone");
        assert!(
            hq.check_failures().is_empty(),
            "nothing reported by the runner"
        );
        assert!(hq.told().is_empty(), "a check is no run: nothing ended");
        // One that ends badly without reporting is reported failed with the job's token; one
        // past twenty minutes is killed and reported.
        let hq = FakeHq::new(vec![check_job(10), check_job(11)]);
        let docker = FakeDocker::default();
        let mut bad = self::runner("check-bad", &hq, &docker, 2);
        bad.step(T0);
        docker.exits("c1", 1);
        bad.step(T0 + LONGEST_CHECK + 1);
        let mut failures = hq.check_failures();
        failures.sort();
        assert_eq!(failures.len(), 2, "{failures:?}");
        assert!(
            failures[0].1.contains("exit 1 without reporting"),
            "{failures:?}"
        );
        assert!(
            failures[1].1.contains("past twenty minutes"),
            "{failures:?}"
        );
        assert!(docker.containers().is_empty());
        // A check with no token, or one that is not 64 hex characters, never reaches Docker.
        let mut odd = check_job(12);
        odd["token"] = json!("short");
        let mut none = check_job(13);
        none.as_object_mut().unwrap().remove("check");
        let hq = FakeHq::new(vec![odd, none]);
        let docker = FakeDocker::default();
        let mut odd_runner = self::runner("check-odd", &hq, &docker, 2);
        odd_runner.step(T0);
        assert!(
            docker.calls().iter().all(|c| c == "ping"),
            "{:?}",
            docker.calls()
        );
    }

    #[test]
    fn a_malformed_run_or_token_is_refused_and_never_reaches_docker() {
        let mut word = turn(0);
        word["run"] = json!("12; docker run --privileged");
        let mut negative = turn(0);
        negative["run"] = json!(-3);
        let mut short = turn(13);
        short["token"] = json!("abc123");
        let mut not_hex = turn(14);
        not_hex["token"] = json!(format!("{}zz", &token(14)[2..]));
        let mut broken = turn(15);
        broken["token"] = json!(format!("{}\nX", &token(15)[2..]));
        let mut none = turn(16);
        none.as_object_mut().unwrap().remove("token");
        let mut no_lease = turn(17);
        no_lease.as_object_mut().unwrap().remove("lease");
        let hq = FakeHq::new(vec![word, negative, short, not_hex, broken, none, no_lease]);
        let docker = FakeDocker::default();
        let mut runner = runner("malformed", &hq, &docker, 7);
        runner.step(T0);

        assert!(docker.calls().is_empty(), "nothing reached Docker");
        assert_eq!(
            hq.told(),
            [(13, false), (14, false), (15, false), (16, false)],
            "each with a run and lease is reported failed; one without has nothing to report"
        );
    }

    #[test]
    fn no_more_turns_run_than_the_runner_has_room_for() {
        let hq = FakeHq::new(vec![turn(1), turn(2), turn(3)]);
        let docker = FakeDocker::default();
        let mut runner = runner("capacity", &hq, &docker, 2);
        runner.step(T0);
        assert_eq!(docker.specs().len(), 2);
        assert_eq!(hq.asked_with(), [2, 1]);

        runner.step(T0 + 2);
        assert_eq!(hq.asked_with(), [2, 1], "full: HQ is not asked");
        assert_eq!(docker.specs().len(), 2);

        docker.exits("c1", 0);
        runner.step(T0 + 4);
        assert_eq!(hq.asked_with(), [2, 1, 1]);
        assert_eq!(docker.containers(), ["c2", "c3"]);
        assert_eq!(hq.turns.lock().unwrap().len(), 0);
    }

    #[test]
    fn no_turn_is_taken_from_hq_while_docker_cannot_start_it() {
        let hq = FakeHq::new(vec![turn(9)]);
        let docker = FakeDocker::default();
        docker.0.lock().unwrap().down = true;
        let mut runner = runner("down", &hq, &docker, 2);
        runner.step(T0);
        assert!(hq.asked_with().is_empty(), "HQ was not asked");
        assert!(
            hq.told().is_empty(),
            "so no turn was failed for want of Docker"
        );

        docker.0.lock().unwrap().down = false;
        runner.step(T0 + 2);
        assert_eq!(docker.calls(), ["create c1", "start c1"]);
    }

    #[test]
    fn an_ending_hq_cannot_hear_is_kept_until_it_can() {
        let hq = FakeHq::new(vec![turn(8)]);
        let docker = FakeDocker::default();
        let file = token_file("unheard");
        let mut runner = Runner::new(docker.clone(), Hq::new(&hq.url, &file).unwrap(), work(1));
        runner.step(T0);
        docker.exits("c1", 0);
        std::fs::remove_file(&file).unwrap();
        runner.step(T0 + 2);
        assert!(hq.told().is_empty());
        assert_eq!(docker.containers(), ["c1"], "kept, with its exit code");

        std::fs::write(&file, RUNNER_TOKEN).unwrap();
        runner.step(T0 + 4);
        assert_eq!(hq.told(), [(8, true)]);
        assert!(docker.containers().is_empty());
    }

    #[test]
    fn turns_an_earlier_runner_left_are_taken_back() {
        let hq = FakeHq::new(Vec::new());
        let docker = FakeDocker::default();
        {
            let mut pretend = docker.0.lock().unwrap();
            pretend
                .containers
                .insert("old".to_owned(), State::Exited(0));
            pretend
                .containers
                .insert("older".to_owned(), State::Running);
            pretend.containers.insert("odd".to_owned(), State::Running);
            pretend.left = vec![
                Left {
                    id: "old".to_owned(),
                    run: "7".to_owned(),
                    lease: lease(7).to_string(),
                    check: String::new(),
                    created: T0,
                },
                Left {
                    id: "older".to_owned(),
                    run: "6".to_owned(),
                    lease: lease(6).to_string(),
                    check: String::new(),
                    created: T0 - LONGEST_TURN - 60,
                },
                Left {
                    id: "odd".to_owned(),
                    run: "not-a-run".to_owned(),
                    lease: String::new(),
                    check: String::new(),
                    created: T0,
                },
                Left {
                    id: "stale-check".to_owned(),
                    run: String::new(),
                    lease: String::new(),
                    check: "4".to_owned(),
                    created: T0,
                },
            ];
        }
        let mut runner = runner("left", &hq, &docker, 2);
        runner.take_back();
        runner.step(T0 + 2);

        let mut told = hq.told();
        told.sort_unstable();
        assert_eq!(
            told,
            [(6, false), (7, true)],
            "one ended while the runner was away, one ran too long"
        );
        assert_eq!(
            docker.containers(),
            ["odd"],
            "not a turn: left alone; a stale check's container is removed"
        );
        assert_eq!(hq.asked_with(), [2], "both places are free again");
    }

    #[test]
    fn a_cloud_brain_points_the_worker_at_the_anthropic_relay_on_the_same_listener() {
        let mut lead = turn(21);
        lead["brain"] = json!("claude-sonnet-5-5");
        lead["role"] = json!("lead");
        lead["effort"] = json!("medium");
        let mut local = turn(22);
        local["brain"] = json!("local");
        local["effort"] = json!("high");
        let hq = FakeHq::new(vec![lead, local]);
        let docker = FakeDocker::default();
        let mut runner = runner("brain", &hq, &docker, 2);
        runner.step(T0);
        let specs = docker.specs();
        assert_eq!(
            specs[0]["Env"],
            json!([
                format!("ENDORA_RUN_TOKEN={}", token(21)),
                "ENDORA_HQ_MCP=http://hq:8790/v2/team/mcp",
                "ENDORA_MODEL_URL=http://node:8788/team-relay/v1",
                "ENDORA_MODEL=claude-sonnet-5-5",
                "ENDORA_EFFORT=medium",
            ])
        );
        assert_eq!(
            specs[1]["Env"],
            json!([
                format!("ENDORA_RUN_TOKEN={}", token(22)),
                "ENDORA_HQ_MCP=http://hq:8790/v2/team/mcp",
                "ENDORA_MODEL_URL=http://node:8788/team-relay/local/v1",
            ])
        );
        assert_eq!(
            specs[0]["HostConfig"], specs[1]["HostConfig"],
            "the same fence either way"
        );
    }

    #[test]
    fn a_turn_on_deepseek_goes_to_the_local_relay_which_sends_it_to_deepseek() {
        // The node's local relay knows the turn's brain from HQ and answers it from the
        // DeepSeek entry alone; the worker speaks OpenAI's API to it, as for the house's models.
        let mut on_deepseek = turn(25);
        on_deepseek["brain"] = json!("deepseek-flash");
        let hq = FakeHq::new(vec![on_deepseek]);
        let docker = FakeDocker::default();
        let mut first = runner("deepseek", &hq, &docker, 1);
        first.step(T0);
        assert_eq!(
            docker.specs()[0]["Env"],
            json!([
                format!("ENDORA_RUN_TOKEN={}", token(25)),
                "ENDORA_HQ_MCP=http://hq:8790/v2/team/mcp",
                "ENDORA_MODEL_URL=http://node:8788/team-relay/local/v1",
            ])
        );
    }

    #[test]
    fn a_turn_on_the_macs_brain_goes_to_the_local_relay_and_never_to_anthropic() {
        let mut on_the_mac = turn(23);
        on_the_mac["brain"] = json!("mac");
        let hq = FakeHq::new(vec![on_the_mac]);
        let docker = FakeDocker::default();
        let mut first = runner("mac", &hq, &docker, 1);
        first.step(T0);
        assert_eq!(
            docker.specs()[0]["Env"],
            json!([
                format!("ENDORA_RUN_TOKEN={}", token(23)),
                "ENDORA_HQ_MCP=http://hq:8790/v2/team/mcp",
                "ENDORA_MODEL_URL=http://node:8788/team-relay/local/v1",
            ])
        );
        let mut claude_code = turn(24);
        claude_code["brain"] = json!("mac");
        claude_code["runtime"] = json!("claude-code");
        let hq = FakeHq::new(vec![claude_code]);
        let docker = FakeDocker::default();
        let mut second = runner("mac-cc", &hq, &docker, 1);
        second.step(T0);
        assert!(
            docker.specs().is_empty(),
            "Claude Code speaks only Anthropic's API"
        );
        assert_eq!(hq.told(), [(24, false)]);
    }

    #[test]
    fn an_effort_that_is_not_one_of_the_five_is_refused_and_reported_failed() {
        for odd in [
            json!("medium\nENDORA_CONTRACTS=http://192.168.1.10/"),
            json!("extreme"),
            json!("High"),
            json!(3),
        ] {
            let mut handed = turn(31);
            handed["brain"] = json!("claude-opus-5-5");
            handed["effort"] = odd.clone();
            let hq = FakeHq::new(vec![handed]);
            let docker = FakeDocker::default();
            let mut runner = runner("odd-effort", &hq, &docker, 1);
            runner.step(T0);
            assert!(docker.specs().is_empty(), "{odd}: no container");
            assert_eq!(hq.told(), [(31, false)], "{odd}");
        }
    }

    #[test]
    fn a_brain_that_is_not_a_model_name_is_refused_and_reported_failed() {
        for odd in [
            json!("claude-sonnet-5-5\nENDORA_CONTRACTS=http://192.168.1.10/"),
            json!("gpt-5"),
            json!("claude-"),
            json!(7),
            json!(format!("claude-{}", "a".repeat(64))),
        ] {
            let mut handed = turn(30);
            handed["brain"] = odd.clone();
            let hq = FakeHq::new(vec![handed]);
            let docker = FakeDocker::default();
            let mut runner = runner("odd-brain", &hq, &docker, 1);
            runner.step(T0);
            assert!(docker.specs().is_empty(), "{odd}: no container");
            assert_eq!(hq.told(), [(30, false)], "{odd}");
        }
    }
}
