//! Endora HQ's worker (ADR 0102, phase 4): takes one turn of a team member, in the fresh
//! container the runner starts for it, then exits. It is given its turn's token and where HQ
//! and the node's model relay are, and nothing else: no key, no mount, no other address.
//!
//! What its exit says, for the runner: 0, a turn taken (the model's last word, or its rounds
//! used up); 3, the turn was over before it ended (HQ or the relay answered 401 or 402); 1,
//! anything else, with a short reason on stderr. The token is never printed.

mod turn;

use turn::{Setup, Stopped, Taken};

/// HQ's team protocol, on the compose network.
const HQ_MCP: &str = "http://hq:8790/v1/team/mcp";
/// The node's relay to the local model, on the compose network.
const MODEL_URL: &str = "http://node:8787/team-relay/local/v1";
/// The model the worker names. The local relay uses its own, whatever the name.
const MODEL: &str = "local";
/// The most rounds of the model in a turn, unless told otherwise.
const MOST_ROUNDS: u32 = 12;

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
    Ok(Setup {
        token,
        mcp: given("ENDORA_HQ_MCP").unwrap_or_else(|| HQ_MCP.to_owned()),
        model_url: given("ENDORA_MODEL_URL").unwrap_or_else(|| MODEL_URL.to_owned()),
        model: given("ENDORA_MODEL").unwrap_or_else(|| MODEL.to_owned()),
        most_rounds,
    })
}

fn main() {
    let setup = match setup(|name| std::env::var(name).ok()) {
        Ok(setup) => setup,
        Err(why) => {
            eprintln!("worker: {why}");
            std::process::exit(FAILED);
        }
    };
    let code = match turn::take(&setup) {
        Ok(Taken::Answered) => TAKEN,
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
        assert_eq!(setup.model_url, "http://node:8787/team-relay/local/v1");
        assert_eq!(setup.model, "local");
        assert_eq!(setup.most_rounds, 12);
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
        assert_eq!(setup.most_rounds, 4);
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
