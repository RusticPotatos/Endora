//! The runner's loop (spec section 15, phase 4, item 5). Every two seconds it kills the turns
//! HQ asks to stop, reports and removes the ones that ended (killing any past thirty
//! minutes), and while it has room asks HQ for the next turn and starts it in a fresh
//! container. HQ names only the run and its token; everything else about the container is
//! the runner's own, so HQ cannot choose the image, the mounts or the network.

use std::collections::{BTreeMap, BTreeSet};

use serde_json::{Value, json};

use crate::docker::{Docker, LEASE_LABEL, RUN_LABEL, State};
use crate::hq::{Hq, HqError};

/// The longest a turn's container runs before it is killed, in seconds.
pub const LONGEST_TURN: u64 = 30 * 60;

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
    if name == "local" {
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
        let text = |key: &str| handed[key].as_str().unwrap_or("").to_owned();
        Ok(Self {
            run,
            lease,
            token: token.to_owned(),
            member: text("member"),
            role: text("role"),
            task: handed["task"].as_u64(),
            cloud,
        })
    }
}

/// The body of `POST /containers/create` for a turn: the worker image and the fixed fence,
/// with the turn's own run, lease and token and nothing else of HQ's but its brain, which
/// picks one of the runner's own two relays (and names the model, for Claude).
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
    if work.workbench {
        env.push("ENDORA_WORKBENCH=1".to_owned());
        tmpfs["/work"] = json!(WORK_FOLDER);
    }
    json!({
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
    })
}

/// A turn's container the runner is watching.
struct Running {
    run: u64,
    lease: u64,
    container: String,
    /// When it started, in seconds since the Unix epoch.
    since: u64,
}

/// The runner: HQ on one side, Docker on the other.
pub struct Runner<D> {
    docker: D,
    hq: Hq,
    work: Work,
    running: Vec<Running>,
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
        self.fill(now);
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
            let succeeded = match state {
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
                    false
                }
                Some(State::Running) => {
                    self.running.push(turn);
                    continue;
                }
                Some(State::Exited(code)) => code == 0,
                // Removed by someone else: how it ended is not known, so not a success.
                None => false,
            };
            // Told before it is removed: while HQ cannot hear, the container keeps how it
            // ended. An answer of any kind is hearing.
            if let Err(e @ HqError::Unreachable(_)) = self.hq.ended(turn.run, turn.lease, succeeded)
            {
                self.trouble("HQ", &e.to_string());
                self.running.push(turn);
                continue;
            }
            if let Err(e) = self.docker.remove(&turn.container) {
                eprintln!("runner: could not remove run {}'s container: {e}", turn.run);
            }
            println!(
                "runner: run {} ended, {}",
                turn.run,
                if succeeded { "taken" } else { "failed" }
            );
        }
    }

    /// While there is room, asks HQ for the next turn and starts it.
    fn fill(&mut self, now: u64) {
        let room = self.work.capacity.saturating_sub(self.running.len());
        if room == 0 {
            return;
        }
        // A turn is taken from HQ only when Docker answers, so none fails for want of it.
        if let Err(e) = self.docker.ping() {
            return self.trouble("Docker", &e);
        }
        self.fine("Docker");
        for _ in 0..room {
            let free = self.work.capacity.saturating_sub(self.running.len());
            let handed = match self.hq.next(free) {
                Ok(Some(handed)) => handed,
                Ok(None) => return,
                Err(e) => return self.trouble("HQ", &e.to_string()),
            };
            match Turn::from_hq(&handed) {
                Ok(turn) => self.start(&turn, now),
                Err(Refused::NoRun) => {
                    eprintln!("runner: refused a turn from HQ whose run is not a number");
                }
                Err(Refused::NoLease(run)) => {
                    eprintln!("runner: refused run {run}: it came with no lease");
                }
                Err(Refused::Token(run, lease)) => {
                    eprintln!("runner: refused run {run}: its token is not 64 hex characters");
                    self.failed(run, lease);
                }
                Err(Refused::Brain(run, lease)) => {
                    eprintln!("runner: refused run {run}: its brain is not a model's name");
                    self.failed(run, lease);
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
                return self.failed(turn.run, turn.lease);
            }
        };
        if let Err(e) = self.docker.start(&container) {
            eprintln!("runner: could not start run {}: {e}", turn.run);
            if let Err(e) = self.docker.remove(&container) {
                eprintln!("runner: could not remove run {}'s container: {e}", turn.run);
            }
            return self.failed(turn.run, turn.lease);
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

    /// Reports a turn that never ran. If HQ cannot hear it, the engine's longest-turn limit
    /// ends it.
    fn failed(&mut self, run: u64, lease: u64) {
        match self.hq.ended(run, lease, false) {
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
                    let signed = header("authorization").as_deref()
                        == Some(&format!("Bearer {RUNNER_TOKEN}"));
                    let (status, reply) = match asked.as_str() {
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
                    Some((run, b["succeeded"].as_bool()?))
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
            hq_mcp: "http://hq:8790/v1/team/mcp".to_owned(),
            capacity,
            workbench: false,
        }
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
                    "ENDORA_HQ_MCP=http://hq:8790/v1/team/mcp",
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
            hq_mcp: format!("{gateway}/team-gateway/mcp"),
            capacity: 1,
            workbench: true,
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
                    "ENDORA_HQ_MCP=http://192.168.1.10:8788/team-gateway/mcp",
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
                },
            })]
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

        runner.step(T0 + 31 * 60);
        assert!(docker.calls().contains(&"kill c1".to_owned()));
        assert_eq!(hq.told(), [(5, false)]);
        assert!(docker.containers().is_empty(), "and removed");
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
                    created: T0,
                },
                Left {
                    id: "older".to_owned(),
                    run: "6".to_owned(),
                    lease: lease(6).to_string(),
                    created: T0 - LONGEST_TURN - 60,
                },
                Left {
                    id: "odd".to_owned(),
                    run: "not-a-run".to_owned(),
                    lease: String::new(),
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
        assert_eq!(docker.containers(), ["odd"], "not a turn: left alone");
        assert_eq!(hq.asked_with(), [2], "both places are free again");
    }

    #[test]
    fn a_cloud_brain_points_the_worker_at_the_anthropic_relay_on_the_same_listener() {
        let mut lead = turn(21);
        lead["brain"] = json!("claude-sonnet-5-5");
        lead["role"] = json!("lead");
        let mut local = turn(22);
        local["brain"] = json!("local");
        let hq = FakeHq::new(vec![lead, local]);
        let docker = FakeDocker::default();
        let mut runner = runner("brain", &hq, &docker, 2);
        runner.step(T0);
        let specs = docker.specs();
        assert_eq!(
            specs[0]["Env"],
            json!([
                format!("ENDORA_RUN_TOKEN={}", token(21)),
                "ENDORA_HQ_MCP=http://hq:8790/v1/team/mcp",
                "ENDORA_MODEL_URL=http://node:8788/team-relay/v1",
                "ENDORA_MODEL=claude-sonnet-5-5",
            ])
        );
        assert_eq!(
            specs[1]["Env"],
            json!([
                format!("ENDORA_RUN_TOKEN={}", token(22)),
                "ENDORA_HQ_MCP=http://hq:8790/v1/team/mcp",
                "ENDORA_MODEL_URL=http://node:8788/team-relay/local/v1",
            ])
        );
        assert_eq!(
            specs[0]["HostConfig"], specs[1]["HostConfig"],
            "the same fence either way"
        );
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
