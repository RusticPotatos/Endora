//! Endora's runner (ADR 0102, phase 4): the program on a host that takes the team's turns.
//! Every two seconds it asks HQ for work and starts each turn as a fresh worker container
//! through Docker's API, fenced on the team's internal network, then reports how it ended and
//! removes it. It holds Docker's socket, which is as much as holding the host, so it trusts
//! HQ with nothing but a run and its token: the image, the mounts, the network and the limits
//! are its own. On the Pi (ADR 0110) it reaches HQ through the node's team gateway, and each
//! turn is a workbench on the `endora_sandbox` network, which reaches the gateway and nothing
//! else.

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
/// the compose file's `workers` profile. A runner whose `HQ_URL` is the node's team gateway
/// (`http://<nas>:8788/team-gateway`, ADR 0110) is the Pi's: its defaults are the workbench
/// image on the `endora_sandbox` network, with workbench turns reaching HQ and the model
/// relays only through that gateway. Anything set by name wins.
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
    let hq = Hq::new(
        &or("HQ_URL", "http://hq:8790"),
        or("HQ_RUNNER_TOKEN_FILE", "/runner-token/token"),
    )?;
    let pi = hq.through_the_gateway();
    // The gateway's listener, which also serves the model relays.
    let listener = hq.origin();
    let (image, network, model_url, cloud_url, hq_mcp) = if pi {
        (
            "endora-workbench:latest".to_owned(),
            "endora_sandbox".to_owned(),
            format!("{listener}/team-relay/local/v1"),
            format!("{listener}/team-relay/v1"),
            format!("{listener}/team-gateway/v2/mcp"),
        )
    } else {
        (
            "endora-node:latest".to_owned(),
            "endora_team".to_owned(),
            "http://node:8788/team-relay/local/v1".to_owned(),
            "http://node:8788/team-relay/v1".to_owned(),
            "http://hq:8790/v2/team/mcp".to_owned(),
        )
    };
    let workbench = match or("WORKBENCH", if pi { "1" } else { "0" }).as_str() {
        "1" => true,
        "0" => false,
        other => return Err(format!("WORKBENCH is 1 or 0, not {other:?}")),
    };
    // A check job (ADR 0115) fetches its bundle from the node's team listener and reports to
    // HQ: through the gateway on the Pi, straight to HQ on the NAS.
    let check_bundle = if pi {
        format!("{listener}/team-relay/check/")
    } else {
        "http://node:8788/team-relay/check/".to_owned()
    };
    let check_done = hq.check_done_prefix();
    let hq_mcp = or("WORKER_HQ_MCP", &hq_mcp);
    endora_protocol::team::canonical_endpoint(&hq_mcp).map_err(str::to_owned)?;
    Ok(Config {
        hq,
        socket: or("DOCKER_SOCKET", "/var/run/docker.sock"),
        work: Work {
            image: or("WORKER_IMAGE", &image),
            network: or("TEAM_NETWORK", &network),
            model_url: or("WORKER_MODEL_URL", &model_url),
            cloud_url: or("WORKER_CLOUD_URL", &cloud_url),
            hq_mcp,
            capacity,
            workbench,
            check_bundle,
            check_done,
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
    fn stale_worker_endpoint_overrides_require_an_explicit_upgrade() {
        for endpoint in [
            "http://hq:8790/v1/team/mcp",
            "http://node:8788/team-gateway/mcp",
        ] {
            let result = config(|name| (name == "WORKER_HQ_MCP").then(|| endpoint.to_owned()));
            assert!(result.err().unwrap().contains("v2"));
        }
    }

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
        assert_eq!(defaults.work.hq_mcp, "http://hq:8790/v2/team/mcp");
        assert_eq!(
            defaults.work.cloud_url, "http://node:8788/team-relay/v1",
            "the Anthropic relay, on the same team listener"
        );
        assert_eq!(defaults.work.capacity, 2);
        assert_eq!(
            defaults.work.check_bundle,
            "http://node:8788/team-relay/check/"
        );
        assert_eq!(defaults.work.check_done, "http://hq:8790/v1/runner/check/");

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
        assert!(!defaults.work.workbench, "the NAS's runner: no workbench");
    }

    #[test]
    fn a_runner_through_the_team_gateway_starts_workbench_turns_reaching_only_the_gateway() {
        let pi = config(|name| {
            (name == "HQ_URL").then(|| "http://192.168.1.10:8788/team-gateway".to_owned())
        })
        .unwrap();
        assert_eq!(pi.hq.url(), "http://192.168.1.10:8788/team-gateway");
        assert!(pi.work.workbench);
        assert_eq!(pi.work.image, "endora-workbench:latest");
        assert_eq!(pi.work.network, "endora_sandbox");
        assert_eq!(
            pi.work.hq_mcp,
            "http://192.168.1.10:8788/team-gateway/v2/mcp"
        );
        assert_eq!(
            pi.work.model_url,
            "http://192.168.1.10:8788/team-relay/local/v1"
        );
        assert_eq!(pi.work.cloud_url, "http://192.168.1.10:8788/team-relay/v1");
        assert_eq!(
            pi.work.check_bundle,
            "http://192.168.1.10:8788/team-relay/check/"
        );
        assert_eq!(
            pi.work.check_done,
            "http://192.168.1.10:8788/team-gateway/runner/check/"
        );

        let told = config(|name| match name {
            "HQ_URL" => Some("http://192.168.1.10:8788/team-gateway".to_owned()),
            "WORKER_IMAGE" => Some("endora-workbench:test".to_owned()),
            "TEAM_NETWORK" => Some("endora_other".to_owned()),
            "WORKBENCH" => Some("0".to_owned()),
            _ => None,
        })
        .unwrap();
        assert_eq!(
            told.work.image, "endora-workbench:test",
            "what it is told wins"
        );
        assert_eq!(told.work.network, "endora_other");
        assert!(!told.work.workbench);
        let nas = config(|name| (name == "WORKBENCH").then(|| "1".to_owned())).unwrap();
        assert!(nas.work.workbench, "turned on by name");
        assert!(config(|name| (name == "WORKBENCH").then(|| "yes".to_owned())).is_err());
    }
}
