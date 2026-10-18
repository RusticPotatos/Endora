//! Endora HQ's worker (ADR 0102, phase 4): takes one turn of a team member, in the fresh
//! container the runner starts for it, then exits. It is given its turn's token and where HQ
//! and the node's model relay are, and nothing else: no key, no mount, no other address. The
//! node's contracts (ADR 0106) are beside its model relay, on the same listener. A member who
//! thinks with Claude (ADR 0109) is given the Anthropic relay, the model's name and, when HQ
//! chose one, the turn's effort (`ENDORA_EFFORT`), and the worker speaks Anthropic's Messages
//! API to it, asking it to cache each round's prompt; otherwise OpenAI's, to the local relay.
//!
//! In the workbench (ring 1, `ENDORA_WORKBENCH=1`), the disposable sandbox whose only way out
//! is Endora's egress proxy, the model is also given a shell, run right there in `/work`.
//!
//! What its exit says, for the runner: 0, a turn taken (the model's last word, or its rounds
//! used up); 3, the turn was over before it ended (HQ or the relay answered 401 or 402); 1,
//! anything else, with a short reason on stderr. The token is never printed.

mod check;
mod repo;
mod shell;
mod turn;

use turn::{Api, EFFORTS, Setup, Stopped, Taken};

/// HQ's team protocol, on the compose network.
const HQ_MCP: &str = "http://hq:8790/v1/team/mcp";
/// The node's relay to the local model, on the compose network.
const MODEL_URL: &str = "http://node:8788/team-relay/local/v1";
/// The node's contracts, on the compose network.
const CONTRACTS: &str = "http://node:8788/team-relay/contracts";
/// The model the worker names. The local relay uses its own, whatever the name.
const MODEL: &str = "local";
/// The most rounds of the model in a turn, unless told otherwise.
const MOST_ROUNDS: u32 = 12;
/// Where the workbench's shell runs: the sandbox's writable home, gone when the turn ends.
const WORKBENCH_DIR: &str = "/work";

/// A turn taken.
const TAKEN: i32 = 0;
/// Anything else.
const FAILED: i32 = 1;
/// HQ or the relay ended the turn.
const OVER: i32 = 3;

/// What the worker is given, from its environment (`env` looks a name up).
fn setup(env: impl Fn(&str) -> Option<String>) -> Result<Setup, String> {
    let given = |name: &str| {
        env(name)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
    };
    let Some(token) = given("ENDORA_RUN_TOKEN") else {
        return Err("ENDORA_RUN_TOKEN is not set: a turn starts only with its token".to_owned());
    };
    let most_rounds = match given("ENDORA_MOST_ROUNDS") {
        None => MOST_ROUNDS,
        Some(n) => n.parse().ok().filter(|n| *n > 0).ok_or_else(|| {
            format!("ENDORA_MOST_ROUNDS must be a whole number above zero, not {n}")
        })?,
    };
    let model_url = given("ENDORA_MODEL_URL").unwrap_or_else(|| MODEL_URL.to_owned());
    // The contracts are on the node's team listener, beside its model relay: where the relay
    // is, unless told otherwise.
    let contracts = given("ENDORA_CONTRACTS").unwrap_or_else(|| {
        model_url.split_once("/team-relay/").map_or_else(
            || CONTRACTS.to_owned(),
            |(node, _)| format!("{node}/team-relay/contracts"),
        )
    });
    let model = given("ENDORA_MODEL").unwrap_or_else(|| MODEL.to_owned());
    // A Claude model is reached through the Anthropic relay, in Anthropic's own API; anything
    // else through the local relay, in OpenAI's.
    let api = if model.starts_with("claude-") {
        Api::Anthropic
    } else {
        Api::OpenAi
    };
    // How hard Claude tries, as HQ decided for this turn: one of the five, or the model's own.
    let effort = match given("ENDORA_EFFORT") {
        None => None,
        Some(e) if EFFORTS.contains(&e.as_str()) => Some(e),
        Some(e) => {
            return Err(format!(
                "ENDORA_EFFORT must be one of {}, not {e}",
                EFFORTS.join(", ")
            ));
        }
    };
    // The workbench (ring 1) says so: only there is the model given a shell.
    let workbench = (given("ENDORA_WORKBENCH").as_deref() == Some("1")).then(|| {
        given("ENDORA_WORKBENCH_DIR")
            .unwrap_or_else(|| WORKBENCH_DIR.to_owned())
            .into()
    });
    Ok(Setup {
        token,
        mcp: given("ENDORA_HQ_MCP").unwrap_or_else(|| HQ_MCP.to_owned()),
        model_url,
        contracts,
        model,
        api,
        most_rounds,
        workbench,
        effort,
    })
}

fn main() {
    // A check job (ADR 0115): no model, no turn, a job token alone; the workbench run to check
    // a submitted branch and report.
    if std::env::var("ENDORA_CHECK").as_deref() == Ok("1") {
        let code = match check::setup(|name| std::env::var(name).ok()) {
            Ok(job) => check::run(&job),
            Err(why) => {
                eprintln!("check: {why}");
                FAILED
            }
        };
        std::process::exit(code);
    }
    let setup = match setup(|name| std::env::var(name).ok()) {
        Ok(setup) => setup,
        Err(why) => {
            eprintln!("worker: {why}");
            std::process::exit(FAILED);
        }
    };
    let code = match turn::take(&setup) {
        Ok(Taken::Answered) => TAKEN,
        Ok(Taken::AskedTheOwner) => {
            eprintln!("worker: asked the owner, which ends the turn");
            TAKEN
        }
        Ok(Taken::OutOfRounds) => {
            eprintln!(
                "worker: the {} rounds ran out before the model finished",
                setup.most_rounds
            );
            TAKEN
        }
        Err(Stopped::Over(why)) => {
            eprintln!("worker: the turn is over: {why}");
            OVER
        }
        Err(Stopped::Failed(why)) => {
            eprintln!("worker: {why}");
            FAILED
        }
    };
    std::process::exit(code);
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn a_token_is_all_it_needs_the_rest_is_the_compose_networks() {
        let setup = setup(env(&[("ENDORA_RUN_TOKEN", "a-turns-token")])).unwrap();
        assert_eq!(setup.token, "a-turns-token");
        assert_eq!(setup.mcp, "http://hq:8790/v1/team/mcp");
        assert_eq!(setup.model_url, "http://node:8788/team-relay/local/v1");
        assert_eq!(setup.contracts, "http://node:8788/team-relay/contracts");
        assert_eq!(setup.model, "local");
        assert_eq!(setup.api, Api::OpenAi);
        assert_eq!(setup.most_rounds, 12);
        assert_eq!(setup.workbench, None, "no shell outside the workbench");
    }

    #[test]
    fn the_workbench_says_so_and_its_shell_runs_in_work() {
        let at = |pairs: &[(&str, &str)]| {
            let mut all = vec![("ENDORA_RUN_TOKEN", "a-turns-token")];
            all.extend_from_slice(pairs);
            setup(env(&all)).unwrap().workbench
        };
        assert_eq!(at(&[("ENDORA_WORKBENCH", "1")]), Some("/work".into()));
        assert_eq!(
            at(&[
                ("ENDORA_WORKBENCH", "1"),
                ("ENDORA_WORKBENCH_DIR", "/tmp/elsewhere")
            ]),
            Some("/tmp/elsewhere".into())
        );
        for no in ["0", "", "yes", "true"] {
            assert_eq!(at(&[("ENDORA_WORKBENCH", no)]), None, "{no}");
        }
        assert_eq!(at(&[("ENDORA_WORKBENCH_DIR", "/work")]), None);
    }

    #[test]
    fn the_contracts_are_beside_the_relay_unless_told() {
        let at = |pairs: &[(&str, &str)]| {
            let mut all = vec![("ENDORA_RUN_TOKEN", "a-turns-token")];
            all.extend_from_slice(pairs);
            setup(env(&all)).unwrap().contracts
        };
        assert_eq!(
            at(&[(
                "ENDORA_MODEL_URL",
                "http://192.168.1.10:8788/team-relay/local/v1"
            )]),
            "http://192.168.1.10:8788/team-relay/contracts"
        );
        assert_eq!(
            at(&[("ENDORA_MODEL_URL", "http://192.168.1.10:11434/v1")]),
            "http://node:8788/team-relay/contracts",
            "a model elsewhere: the node's own address"
        );
        assert_eq!(
            at(&[(
                "ENDORA_CONTRACTS",
                "http://192.168.1.10:8788/team-relay/contracts"
            )]),
            "http://192.168.1.10:8788/team-relay/contracts"
        );
    }

    #[test]
    fn what_it_is_given_is_what_it_uses() {
        let setup = setup(env(&[
            ("ENDORA_RUN_TOKEN", "a-turns-token"),
            ("ENDORA_HQ_MCP", "http://192.168.1.10:8790/v1/team/mcp"),
            ("ENDORA_MODEL_URL", "http://192.168.1.10:8787/team-relay/v1"),
            ("ENDORA_MODEL", "claude-haiku-5-5"),
            ("ENDORA_MOST_ROUNDS", "4"),
        ]))
        .unwrap();
        assert_eq!(setup.mcp, "http://192.168.1.10:8790/v1/team/mcp");
        assert_eq!(setup.model_url, "http://192.168.1.10:8787/team-relay/v1");
        assert_eq!(setup.model, "claude-haiku-5-5");
        assert_eq!(setup.api, Api::Anthropic, "Claude speaks Anthropic's API");
        assert_eq!(setup.most_rounds, 4);
    }

    #[test]
    fn the_effort_is_one_of_five_or_left_to_the_model() {
        let at = |effort: &str| {
            setup(env(&[
                ("ENDORA_RUN_TOKEN", "a-turns-token"),
                ("ENDORA_EFFORT", effort),
            ]))
            .map(|s| s.effort)
        };
        assert_eq!(
            setup(env(&[("ENDORA_RUN_TOKEN", "t")])).unwrap().effort,
            None
        );
        assert_eq!(at(""), Ok(None));
        for effort in EFFORTS {
            assert_eq!(at(effort), Ok(Some(effort.to_owned())));
        }
        for odd in ["extreme", "HIGH", "low\nENDORA_MODEL=x"] {
            assert!(at(odd).is_err(), "{odd}");
        }
    }

    #[test]
    fn no_token_or_no_rounds_is_no_turn() {
        assert!(setup(env(&[])).is_err());
        assert!(setup(env(&[("ENDORA_RUN_TOKEN", "  ")])).is_err());
        for rounds in ["0", "many", "-1"] {
            assert!(
                setup(env(&[
                    ("ENDORA_RUN_TOKEN", "a-turns-token"),
                    ("ENDORA_MOST_ROUNDS", rounds)
                ]))
                .is_err(),
                "{rounds}"
            );
        }
    }
}
