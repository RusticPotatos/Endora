//! Endora CLI binary.
//!
//! A thin, replaceable client: it turns commands into requests against the
//! node's versioned protocol and prints the result. It holds no authority and
//! no domain logic. Command routing is a pure function ([`route`]) so it can be
//! tested without a running node.

#![forbid(unsafe_code)]

mod client;

use std::process::ExitCode;

use client::Client;
use serde_json::{Value, json};

/// The HTTP action a command maps to.
#[derive(Debug, PartialEq, Eq)]
enum Action {
    /// A GET against the given path.
    Get(String),
    /// A POST of a JSON body against the given path.
    Post(String, Value),
}

fn main() -> ExitCode {
    let args: Vec<String> = std::env::args().skip(1).collect();
    let args: Vec<&str> = args.iter().map(String::as_str).collect();

    match &args[..] {
        [] | ["help"] | ["-h"] | ["--help"] => {
            print_usage();
            return ExitCode::SUCCESS;
        }
        _ => {}
    }

    let Some(action) = route(&args) else {
        eprintln!("error: unknown command\n");
        print_usage();
        return ExitCode::FAILURE;
    };

    match execute(&action) {
        Ok(code) => code,
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::FAILURE
        }
    }
}

/// Maps command-line arguments to an [`Action`], or `None` if unrecognized.
fn route(args: &[&str]) -> Option<Action> {
    match args {
        ["health"] => Some(Action::Get("/health".to_owned())),
        ["direction", "create", title] => Some(Action::Post(
            "/v1/directions".to_owned(),
            json!({ "title": title }),
        )),
        ["goal", "create", direction, statement] => Some(Action::Post(
            format!("/v1/directions/{direction}/goals"),
            json!({ "statement": statement }),
        )),
        ["goal", "list", direction] => {
            Some(Action::Get(format!("/v1/directions/{direction}/goals")))
        }
        ["assumption", "create", goal, statement] => Some(Action::Post(
            format!("/v1/goals/{goal}/assumptions"),
            json!({ "statement": statement }),
        )),
        ["assumption", "list", goal] => Some(Action::Get(format!("/v1/goals/{goal}/assumptions"))),
        ["experiment", "propose", assumption, hypothesis] => Some(Action::Post(
            format!("/v1/assumptions/{assumption}/experiments"),
            json!({ "hypothesis": hypothesis }),
        )),
        ["experiment", "list", assumption] => Some(Action::Get(format!(
            "/v1/assumptions/{assumption}/experiments"
        ))),
        ["experiment", "start", id] => Some(Action::Post(
            format!("/v1/experiments/{id}/start"),
            json!({}),
        )),
        ["experiment", "conclude", id] => Some(Action::Post(
            format!("/v1/experiments/{id}/conclude"),
            json!({}),
        )),
        ["observation", "record", experiment, note] => Some(Action::Post(
            format!("/v1/experiments/{experiment}/observations"),
            json!({ "note": note }),
        )),
        ["observation", "list", experiment] => Some(Action::Get(format!(
            "/v1/experiments/{experiment}/observations"
        ))),
        _ => None,
    }
}

/// Runs an action against the node and prints the JSON response. The exit code
/// reflects the HTTP status: success for 2xx, failure otherwise.
fn execute(action: &Action) -> Result<ExitCode, client::ClientError> {
    let client = Client::new(base_url());
    let (status, body) = match action {
        Action::Get(path) => client.get(path)?,
        Action::Post(path, payload) => client.post(path, payload)?,
    };
    println!("{}", serde_json::to_string_pretty(&body)?);
    if (200..300).contains(&status) {
        Ok(ExitCode::SUCCESS)
    } else {
        Ok(ExitCode::FAILURE)
    }
}

/// The node base URL, overridable via `ENDORA_URL`.
fn base_url() -> String {
    std::env::var("ENDORA_URL").unwrap_or_else(|_| "http://127.0.0.1:8787".to_owned())
}

fn print_usage() {
    println!("{}", endora_application::platform_identity());
    println!(
        "\nUsage: endora <command>\n\n\
         Commands:\n  \
           health                                 check the node is up\n  \
           direction create <title>               create a direction\n  \
           goal create <direction-id> <statement> add a goal to a direction\n  \
           goal list <direction-id>               list a direction's goals\n  \
           assumption create <goal-id> <text>     add an assumption to a goal\n  \
           assumption list <goal-id>              list a goal's assumptions\n  \
           experiment propose <assumption-id> <h> propose an experiment\n  \
           experiment list <assumption-id>        list an assumption's experiments\n  \
           experiment start <experiment-id>       start a proposed experiment\n  \
           experiment conclude <experiment-id>    conclude a running experiment\n  \
           observation record <experiment-id> <n> record an observation\n  \
           observation list <experiment-id>       list an experiment's observations\n\n\
         Environment:\n  \
           ENDORA_URL   node base URL (default http://127.0.0.1:8787)"
    );
}

#[cfg(test)]
mod tests {
    use super::{Action, route};
    use serde_json::json;

    #[test]
    fn routes_health() {
        assert_eq!(route(&["health"]), Some(Action::Get("/health".to_owned())));
    }

    #[test]
    fn routes_direction_create() {
        assert_eq!(
            route(&["direction", "create", "Be healthier"]),
            Some(Action::Post(
                "/v1/directions".to_owned(),
                json!({ "title": "Be healthier" })
            ))
        );
    }

    #[test]
    fn routes_goal_create_and_list() {
        assert_eq!(
            route(&["goal", "create", "42", "Run a 5k"]),
            Some(Action::Post(
                "/v1/directions/42/goals".to_owned(),
                json!({ "statement": "Run a 5k" })
            ))
        );
        assert_eq!(
            route(&["goal", "list", "42"]),
            Some(Action::Get("/v1/directions/42/goals".to_owned()))
        );
    }

    #[test]
    fn routes_assumption_create_and_list() {
        assert_eq!(
            route(&["assumption", "create", "7", "Mornings are freest"]),
            Some(Action::Post(
                "/v1/goals/7/assumptions".to_owned(),
                json!({ "statement": "Mornings are freest" })
            ))
        );
        assert_eq!(
            route(&["assumption", "list", "7"]),
            Some(Action::Get("/v1/goals/7/assumptions".to_owned()))
        );
    }

    #[test]
    fn routes_experiment_commands() {
        assert_eq!(
            route(&["experiment", "propose", "5", "Try mornings"]),
            Some(Action::Post(
                "/v1/assumptions/5/experiments".to_owned(),
                json!({ "hypothesis": "Try mornings" })
            ))
        );
        assert_eq!(
            route(&["experiment", "start", "9"]),
            Some(Action::Post(
                "/v1/experiments/9/start".to_owned(),
                json!({})
            ))
        );
        assert_eq!(
            route(&["experiment", "conclude", "9"]),
            Some(Action::Post(
                "/v1/experiments/9/conclude".to_owned(),
                json!({})
            ))
        );
    }

    #[test]
    fn routes_observation_commands() {
        assert_eq!(
            route(&["observation", "record", "9", "felt good"]),
            Some(Action::Post(
                "/v1/experiments/9/observations".to_owned(),
                json!({ "note": "felt good" })
            ))
        );
        assert_eq!(
            route(&["observation", "list", "9"]),
            Some(Action::Get("/v1/experiments/9/observations".to_owned()))
        );
    }

    #[test]
    fn unknown_command_is_none() {
        assert_eq!(route(&["nope"]), None);
        assert_eq!(route(&["goal", "create"]), None);
    }
}
