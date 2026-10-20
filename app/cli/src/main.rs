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
           audit [limit]                          show recent audit records\n  \
           activity [limit]                       show the recent activity feed\n  \
           export                                 export Endora's record as JSON\n  \
           purge confirm                          permanently delete Endora's record\n\n\
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
    fn routes_audit() {
        assert_eq!(route(&["audit"]), Some(Action::Get("/v1/audit".to_owned())));
        assert_eq!(
            route(&["audit", "10"]),
            Some(Action::Get("/v1/audit?limit=10".to_owned()))
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
        // The chat and what served it are retired (ADR 0136).
        for gone in [&["chat"][..], &["preference", "list"], &["brain"]] {
            assert_eq!(route(gone), None);
        }
    }
}
