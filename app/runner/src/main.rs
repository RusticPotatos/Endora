//! Endora's runner (ADR 0102, phase 4): the program on a host that takes the team's turns.
//! Every two seconds it asks HQ for work and starts each turn as a fresh worker container
//! through Docker's API, fenced on the team's internal network, then reports how it ended and
//! removes it. It holds Docker's socket, which is as much as holding the host, so it trusts
//! HQ with nothing but a run and its token: the image, the mounts, the network and the limits
//! are its own.

mod docker;
mod hq;
mod http;
mod runner;

use std::time::Duration;

use docker::Engine;
use hq::Hq;
use runner::{Runner, Work};

/// How often the runner asks HQ for work.
const EVERY: Duration = Duration::from_secs(2);

/// What the runner is set up with.
struct Config {
    hq: Hq,
    socket: String,
    work: Work,
}

/// The runner's setup from its environment (`get` reads one variable), with the defaults of
/// the compose file's `workers` profile.
fn config(get: impl Fn(&str) -> Option<String>) -> Result<Config, String> {
    // Empty is unset, as compose passes an unset `${VAR-}`.
    let or = |name: &str, default: &str| {
        get(name)
            .map(|v| v.trim().to_owned())
            .filter(|v| !v.is_empty())
            .unwrap_or_else(|| default.to_owned())
    };
    let capacity = or("RUNNER_CAPACITY", "2");
    let Some(capacity) = capacity.parse::<usize>().ok().filter(|c| *c >= 1) else {
        return Err(format!(
            "RUNNER_CAPACITY is not a number of turns: {capacity:?}"
        ));
    };
    Ok(Config {
        hq: Hq::new(
            &or("HQ_URL", "http://hq:8790"),
            or("HQ_RUNNER_TOKEN_FILE", "/runner-token/token"),
        )?,
        socket: or("DOCKER_SOCKET", "/var/run/docker.sock"),
        work: Work {
            image: or("WORKER_IMAGE", "endora-node:latest"),
            network: or("TEAM_NETWORK", "endora_team"),
            model_url: or("WORKER_MODEL_URL", "http://node:8788/team-relay/local/v1"),
            cloud_url: or("WORKER_CLOUD_URL", "http://node:8788/team-relay/v1"),
            hq_mcp: or("WORKER_HQ_MCP", "http://hq:8790/v1/team/mcp"),
            capacity,
        },
    })
}

/// Seconds since the Unix epoch.
fn now() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.as_secs())
}

fn main() {
    let config = match config(|name| std::env::var(name).ok()) {
        Ok(config) => config,
        Err(why) => {
            eprintln!("runner: {why}. The runner does not start.");
            std::process::exit(2);
        }
    };
    println!(
        "runner: asking {} for turns every {} s, {} at a time, each in {} on {}",
        config.hq.url(),
        EVERY.as_secs(),
        config.work.capacity,
        config.work.image,
        config.work.network
    );
    let mut runner = Runner::new(Engine::new(config.socket), config.hq, config.work);
    runner.take_back();
    loop {
        runner.step(now());
        std::thread::sleep(EVERY);
    }
}

#[cfg(test)]
mod tests {
    use super::config;

    #[test]
    fn the_runner_is_set_up_from_its_environment_with_the_composes_defaults() {
        let defaults = config(|_| None).unwrap();
        assert_eq!(defaults.hq.url(), "http://hq:8790");
        assert_eq!(defaults.socket, "/var/run/docker.sock");
        assert_eq!(defaults.work.image, "endora-node:latest");
        assert_eq!(defaults.work.network, "endora_team");
        assert_eq!(
            defaults.work.model_url,
            "http://node:8788/team-relay/local/v1"
        );
        assert_eq!(defaults.work.hq_mcp, "http://hq:8790/v1/team/mcp");
        assert_eq!(
            defaults.work.cloud_url, "http://node:8788/team-relay/v1",
            "the Anthropic relay, on the same team listener"
        );
        assert_eq!(defaults.work.capacity, 2);

        let given = config(|name| match name {
            "HQ_URL" => Some("http://192.168.1.10:8790".to_owned()),
            "RUNNER_CAPACITY" => Some(" 4 ".to_owned()),
            "WORKER_IMAGE" => Some("endora-node:test".to_owned()),
            "TEAM_NETWORK" => Some("".to_owned()),
            _ => None,
        })
        .unwrap();
        assert_eq!(given.hq.url(), "http://192.168.1.10:8790");
        assert_eq!(given.work.capacity, 4);
        assert_eq!(given.work.image, "endora-node:test");
        assert_eq!(given.work.network, "endora_team", "empty is unset");

        for capacity in ["0", "two", "-1"] {
            let refused = config(|name| (name == "RUNNER_CAPACITY").then(|| capacity.to_owned()));
            assert!(refused.is_err(), "{capacity}");
        }
        assert!(config(|name| (name == "HQ_URL").then(|| "https://hq:8790".to_owned())).is_err());
    }
}
