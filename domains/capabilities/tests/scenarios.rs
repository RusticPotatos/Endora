//! The bridge's scenarios (docs/testing.md): the owner's words in `features/*.feature`, run
//! against the bridge's policy as the node composes it (`app/node/src/api/bridge.rs`): each
//! capability or tool server's tool is a candidate, the node asks the policy which are offered,
//! and a call is resolved against that same list.

use cucumber::cli::Parser as _;
use cucumber::{World, given, then, when};
use endora_capabilities::Stance;
use endora_capabilities::bridge::{Bridged, Candidate, Kind, bridge, switches_the_owners_devices};

/// One scenario's bridge: what Endora has, and what the team was offered.
#[derive(Debug, Default, World)]
struct Bridge {
    /// The tool server the owner paired with Home Assistant, by name.
    paired: String,
    /// Every capability and tool server's tool Endora has, as the node describes them.
    candidates: Vec<Candidate>,
    /// The team's tool list, as last drawn up.
    offered: Vec<Bridged>,
    /// Why the last call was refused, or `None` when it was allowed.
    refused: Option<String>,
}

impl Bridge {
    /// A candidate as the node makes one: ready, the owner's stance on it, and whether it
    /// switches the owner's devices decided by the server the owner paired.
    fn add(&mut self, id: &str, kind: Kind, stance: Stance) {
        self.candidates.push(Candidate {
            id: id.to_owned(),
            description: "Does a thing.".to_owned(),
            input_schema: None,
            kind,
            stance,
            from_a_server: id.contains('.'),
            ready: true,
            administration: false,
            third_party: false,
            owners_devices: switches_the_owners_devices(id, kind, &self.paired),
        });
    }

    fn has(&self, name: &str) -> bool {
        self.offered.iter().any(|b| b.name == name)
    }
}

#[given(expr = "the owner paired Home Assistant with the tool server {string}")]
fn paired(w: &mut Bridge, server: String) {
    w.paired = server;
}

#[given(expr = "that server has the act {string}, switched on")]
fn servers_act(w: &mut Bridge, tool: String) {
    let id = format!("{}.{tool}", w.paired);
    w.add(&id, Kind::Act, Stance::Auto);
}

#[given(expr = "that server has the read {string}, the owner's named reader")]
fn servers_reader(w: &mut Bridge, tool: String) {
    let id = format!("{}.{tool}", w.paired);
    w.add(&id, Kind::Read, Stance::Auto);
}

#[given(expr = "the tool server {string} has the act {string}, switched on")]
fn a_servers_act(w: &mut Bridge, server: String, tool: String) {
    let id = format!("{server}.{tool}");
    w.add(&id, Kind::Act, Stance::Auto);
}

#[given(expr = "Endora has the read {string}, switched on")]
fn endoras_read(w: &mut Bridge, id: String) {
    w.add(&id, Kind::Read, Stance::Auto);
}

#[when("the team's tools are drawn up")]
fn drawn_up(w: &mut Bridge) {
    w.offered = w.candidates.iter().filter_map(|c| bridge(c).ok()).collect();
}

#[then(expr = "{string} is not among them")]
fn not_offered(w: &mut Bridge, name: String) {
    assert!(!w.has(&name), "{name} is offered: {:?}", w.offered);
}

#[then(expr = "{string} is among them")]
fn offered(w: &mut Bridge, name: String) {
    assert!(w.has(&name), "{name} is not offered: {:?}", w.offered);
}

#[when(expr = "a member calls {string} anyway")]
fn calls(w: &mut Bridge, id: String) {
    let candidate = w
        .candidates
        .iter()
        .find(|c| c.id == id)
        .unwrap_or_else(|| panic!("Endora has no {id}"));
    w.refused = bridge(candidate).err().map(str::to_owned);
}

#[then(expr = "it is refused: {string}")]
fn refused(w: &mut Bridge, why: String) {
    assert_eq!(w.refused.as_deref(), Some(why.as_str()));
}

/// cucumber's command line, less what `cargo test` passes every test binary (`--test-threads`,
/// `--nocapture` and the like), which it would otherwise refuse. A bare word is a scenario
/// name to run, as it is for libtest.
fn options() -> cucumber::cli::Opts<
    cucumber::parser::basic::Cli,
    cucumber::runner::basic::Cli,
    cucumber::writer::basic::Cli,
> {
    const WITH_A_VALUE: &[&str] = &["--test-threads", "--skip", "--format", "--color", "-Z"];
    let mut args = std::env::args();
    let mut kept = vec![args.next().unwrap_or_default()];
    while let Some(arg) = args.next() {
        if WITH_A_VALUE.contains(&arg.as_str()) {
            args.next();
            continue;
        }
        if arg.starts_with("--test-threads=") || (arg.starts_with('-') && !arg.starts_with("--")) {
            continue;
        }
        if arg.starts_with("--") {
            if arg.starts_with("--name") || arg.starts_with("--tags") {
                kept.push(arg);
            }
            continue;
        }
        kept.push("--name".to_owned());
        kept.push(arg);
    }
    cucumber::cli::Opts::try_parse_from(kept).expect("cucumber's own options")
}

fn main() {
    futures::executor::block_on(
        Bridge::cucumber()
            .with_cli(options())
            .fail_on_skipped()
            .run_and_exit("features"),
    );
}
