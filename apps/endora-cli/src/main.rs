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
        ["direction", "list"] => Some(Action::Get("/v1/directions".to_owned())),
        ["target", "create", direction, statement] => Some(Action::Post(
            format!("/v1/directions/{direction}/targets"),
            json!({ "statement": statement }),
        )),
        ["target", "list", direction] => {
            Some(Action::Get(format!("/v1/directions/{direction}/targets")))
        }
        ["assumption", "create", target, statement] => Some(Action::Post(
            format!("/v1/targets/{target}/assumptions"),
            json!({ "statement": statement }),
        )),
        ["assumption", "list", target] => {
            Some(Action::Get(format!("/v1/targets/{target}/assumptions")))
        }
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
        ["experiment", "review", id, days] => Some(Action::Post(
            format!("/v1/experiments/{id}/review"),
            json!({ "in_days": days.parse::<u32>().ok()? }),
        )),
        ["reviews", "due"] => Some(Action::Get("/v1/reviews/due".to_owned())),
        ["observation", "record", experiment, note] => Some(Action::Post(
            format!("/v1/experiments/{experiment}/observations"),
            json!({ "note": note }),
        )),
        ["observation", "list", experiment] => Some(Action::Get(format!(
            "/v1/experiments/{experiment}/observations"
        ))),
        ["reflection", "create", target, summary, evidence] => {
            let evidence: Vec<&str> = evidence.split(',').filter(|s| !s.is_empty()).collect();
            Some(Action::Post(
                format!("/v1/targets/{target}/reflections"),
                json!({ "summary": summary, "evidence": evidence }),
            ))
        }
        ["reflection", "list", target] => {
            Some(Action::Get(format!("/v1/targets/{target}/reflections")))
        }
        ["process-change", "propose", reflection, description] => Some(Action::Post(
            format!("/v1/reflections/{reflection}/process-changes"),
            json!({ "description": description }),
        )),
        ["process-change", "list", reflection] => Some(Action::Get(format!(
            "/v1/reflections/{reflection}/process-changes"
        ))),
        ["process-change", "draft", reflection] => Some(Action::Post(
            format!("/v1/reflections/{reflection}/process-changes/draft"),
            json!({}),
        )),
        ["process-change", "approve", id] => Some(Action::Post(
            format!("/v1/process-changes/{id}/approve"),
            json!({}),
        )),
        ["process-change", "reject", id] => Some(Action::Post(
            format!("/v1/process-changes/{id}/reject"),
            json!({}),
        )),
        ["process-change", "decide", id, actor] => Some(Action::Post(
            format!("/v1/process-changes/{id}/decision"),
            json!({ "actor": actor }),
        )),
        ["audit"] => Some(Action::Get("/v1/audit".to_owned())),
        ["audit", limit] => Some(Action::Get(format!("/v1/audit?limit={limit}"))),
        ["activity"] => Some(Action::Get("/v1/activity".to_owned())),
        ["activity", limit] => Some(Action::Get(format!("/v1/activity?limit={limit}"))),
        ["export"] => Some(Action::Get("/v1/export".to_owned())),
        ["purge", "confirm"] => Some(Action::Post(
            "/v1/memory/purge".to_owned(),
            json!({ "confirm": true }),
        )),
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
           direction list                         list your directions\n  \
           target create <direction-id> <statement> add a target to a direction\n  \
           target list <direction-id>               list a direction's targets\n  \
           assumption create <target-id> <text>     add an assumption to a target\n  \
           assumption list <target-id>              list a target's assumptions\n  \
           experiment propose <assumption-id> <h> propose an experiment\n  \
           experiment list <assumption-id>        list an assumption's experiments\n  \
           experiment start <experiment-id>       start a proposed experiment\n  \
           experiment conclude <experiment-id>    conclude a running experiment\n  \
           experiment review <experiment-id> <days>  remind me to review it in N days\n  \
           reviews due                            list experiments due for review\n  \
           observation record <experiment-id> <n> record an observation\n  \
           observation list <experiment-id>       list an experiment's observations\n  \
           reflection create <target-id> <summary> <obs-ids>  reflect (obs-ids: comma-separated)\n  \
           reflection list <target-id>              list a target's reflections\n  \
           process-change propose <reflection-id> <desc>  propose a process change\n  \
           process-change list <reflection-id>    list a reflection's proposed changes\n  \
           process-change draft <reflection-id>   let the model draft a change (pending)\n  \
           process-change approve <id>            approve a proposed change\n  \
           process-change reject <id>             reject a proposed change\n  \
           process-change decide <id> <actor>     run policy on a change (audited)\n  \
           audit [limit]                          show recent audit records\n  \
           activity [limit]                       show the recent activity feed\n  \
           export                                 export all your data as JSON\n  \
           purge confirm                          permanently delete all your data\n\n\
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
    fn routes_direction_list() {
        assert_eq!(
            route(&["direction", "list"]),
            Some(Action::Get("/v1/directions".to_owned()))
        );
    }

    #[test]
    fn routes_target_create_and_list() {
        assert_eq!(
            route(&["target", "create", "42", "Run a 5k"]),
            Some(Action::Post(
                "/v1/directions/42/targets".to_owned(),
                json!({ "statement": "Run a 5k" })
            ))
        );
        assert_eq!(
            route(&["target", "list", "42"]),
            Some(Action::Get("/v1/directions/42/targets".to_owned()))
        );
    }

    #[test]
    fn routes_assumption_create_and_list() {
        assert_eq!(
            route(&["assumption", "create", "7", "Mornings are freest"]),
            Some(Action::Post(
                "/v1/targets/7/assumptions".to_owned(),
                json!({ "statement": "Mornings are freest" })
            ))
        );
        assert_eq!(
            route(&["assumption", "list", "7"]),
            Some(Action::Get("/v1/targets/7/assumptions".to_owned()))
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
    fn routes_review_commands() {
        assert_eq!(
            route(&["experiment", "review", "9", "7"]),
            Some(Action::Post(
                "/v1/experiments/9/review".to_owned(),
                json!({ "in_days": 7 })
            ))
        );
        // A non-numeric day count is not a valid review command.
        assert_eq!(route(&["experiment", "review", "9", "soon"]), None);
        assert_eq!(
            route(&["reviews", "due"]),
            Some(Action::Get("/v1/reviews/due".to_owned()))
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
    fn routes_reflection_create_splits_evidence() {
        assert_eq!(
            route(&["reflection", "create", "3", "mornings worked", "10,11"]),
            Some(Action::Post(
                "/v1/targets/3/reflections".to_owned(),
                json!({ "summary": "mornings worked", "evidence": ["10", "11"] })
            ))
        );
        assert_eq!(
            route(&["reflection", "list", "3"]),
            Some(Action::Get("/v1/targets/3/reflections".to_owned()))
        );
    }

    #[test]
    fn routes_process_change_commands() {
        assert_eq!(
            route(&["process-change", "propose", "6", "Default to mornings"]),
            Some(Action::Post(
                "/v1/reflections/6/process-changes".to_owned(),
                json!({ "description": "Default to mornings" })
            ))
        );
        assert_eq!(
            route(&["process-change", "approve", "7"]),
            Some(Action::Post(
                "/v1/process-changes/7/approve".to_owned(),
                json!({})
            ))
        );
    }

    #[test]
    fn routes_process_change_draft() {
        assert_eq!(
            route(&["process-change", "draft", "6"]),
            Some(Action::Post(
                "/v1/reflections/6/process-changes/draft".to_owned(),
                json!({})
            ))
        );
    }

    #[test]
    fn routes_decision_and_audit() {
        assert_eq!(
            route(&["process-change", "decide", "7", "act_within_policy"]),
            Some(Action::Post(
                "/v1/process-changes/7/decision".to_owned(),
                json!({ "actor": "act_within_policy" })
            ))
        );
        assert_eq!(route(&["audit"]), Some(Action::Get("/v1/audit".to_owned())));
        assert_eq!(
            route(&["audit", "5"]),
            Some(Action::Get("/v1/audit?limit=5".to_owned()))
        );
    }

    #[test]
    fn routes_activity() {
        assert_eq!(
            route(&["activity"]),
            Some(Action::Get("/v1/activity".to_owned()))
        );
        assert_eq!(
            route(&["activity", "20"]),
            Some(Action::Get("/v1/activity?limit=20".to_owned()))
        );
    }

    #[test]
    fn routes_export_and_purge() {
        assert_eq!(
            route(&["export"]),
            Some(Action::Get("/v1/export".to_owned()))
        );
        assert_eq!(
            route(&["purge", "confirm"]),
            Some(Action::Post(
                "/v1/memory/purge".to_owned(),
                json!({ "confirm": true })
            ))
        );
        // A bare `purge` is not a confirmation.
        assert_eq!(route(&["purge"]), None);
    }

    #[test]
    fn unknown_command_is_none() {
        assert_eq!(route(&["nope"]), None);
        assert_eq!(route(&["target", "create"]), None);
    }
}
