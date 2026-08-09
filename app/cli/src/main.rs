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
    /// A DELETE against the given path.
    Delete(String),
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
        ["chat"] => Some(Action::Get("/v1/chat".to_owned())),
        ["chat", message] => Some(Action::Post(
            "/v1/chat".to_owned(),
            json!({ "message": message }),
        )),
        ["preference", "list"] => Some(Action::Get("/v1/preferences".to_owned())),
        ["preference", "add", text] => Some(Action::Post(
            "/v1/preferences".to_owned(),
            json!({ "text": text }),
        )),
        ["preference", "delete", id] => Some(Action::Delete(format!("/v1/preferences/{id}"))),
        ["understanding"] => Some(Action::Get("/v1/understanding".to_owned())),
        ["understanding", "affirm", id] => Some(Action::Post(
            format!("/v1/understanding/{id}/affirm"),
            json!({}),
        )),
        ["understanding", "correct", id] => Some(Action::Post(
            format!("/v1/understanding/{id}/correct"),
            json!({}),
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
        Action::Delete(path) => client.delete(path)?,
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
           chat \"<message>\"                       talk to the butler\n  \
           chat                                   show the conversation so far\n  \
           preference list                        what the butler has learned\n  \
           preference add <text>                  remember a preference\n  \
           preference delete <id>                 forget a preference\n  \
           understanding                          what Endora believes about you, and why\n  \
           understanding affirm <id>              confirm a belief (raises its confidence)\n  \
           understanding correct <id>             mark a belief wrong (drops it)\n  \
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
    fn routes_chat() {
        assert_eq!(
            route(&["chat", "I want to run more"]),
            Some(Action::Post(
                "/v1/chat".to_owned(),
                json!({ "message": "I want to run more" })
            ))
        );
        assert_eq!(route(&["chat"]), Some(Action::Get("/v1/chat".to_owned())));
    }

    #[test]
    fn routes_preferences() {
        assert_eq!(
            route(&["preference", "add", "I prefer mornings"]),
            Some(Action::Post(
                "/v1/preferences".to_owned(),
                json!({ "text": "I prefer mornings" })
            ))
        );
        assert_eq!(
            route(&["preference", "list"]),
            Some(Action::Get("/v1/preferences".to_owned()))
        );
        assert_eq!(
            route(&["preference", "delete", "9"]),
            Some(Action::Delete("/v1/preferences/9".to_owned()))
        );
    }

    #[test]
    fn routes_audit() {
        assert_eq!(route(&["audit"]), Some(Action::Get("/v1/audit".to_owned())));
        assert_eq!(
            route(&["audit", "10"]),
            Some(Action::Get("/v1/audit?limit=10".to_owned()))
        );
    }

    #[test]
    fn routes_understanding() {
        assert_eq!(
            route(&["understanding"]),
            Some(Action::Get("/v1/understanding".to_owned()))
        );
        assert_eq!(
            route(&["understanding", "affirm", "7"]),
            Some(Action::Post(
                "/v1/understanding/7/affirm".to_owned(),
                json!({})
            ))
        );
        assert_eq!(
            route(&["understanding", "correct", "7"]),
            Some(Action::Post(
                "/v1/understanding/7/correct".to_owned(),
                json!({})
            ))
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
