//! The home lab's map (ADR 0112): for every container, what it is, who made it and what it
//! hangs on, from Docker's own metadata, so the team knows how the lab is built before it
//! treats a stop as a fault. Gluetun restarting takes down what runs in its network, and it
//! all comes back by itself; a team that did not know that chased a fixed problem.
//!
//! Names and wiring only (Internal): [`project`] keeps a fixed handful of Docker's fields and
//! drops the rest, its environment, mounts and binds above all, wherever the inspection came
//! from. [`build`] resolves `container:<id>` network modes and compose `depends_on` labels
//! into who depends on whom, both ways, and says who manages each container.

use serde_json::{Value, json};

/// How long a map is kept before Docker is asked again.
pub const MAP_FOR_MS: i64 = 60_000;

/// The compose labels the map reads: the service, its project, and what it depends on.
pub const COMPOSE_LABELS: [&str; 3] = [
    "com.docker.compose.service",
    "com.docker.compose.project",
    "com.docker.compose.depends_on",
];

fn text(value: &Value) -> String {
    let said: String = value
        .as_str()
        .unwrap_or("")
        .chars()
        .filter(|c| !c.is_control())
        .take(160)
        .collect();
    crate::scrub_secrets(said.trim())
}

/// An image as `repo:tag`: a digest is dropped, and a bare image id says nothing.
fn image_of(raw: &str) -> String {
    let named = raw.split('@').next().unwrap_or("").trim();
    if named.starts_with("sha256:") {
        return String::new();
    }
    named.to_owned()
}

/// The restart policy as Docker names it: `no`, `always`, `unless-stopped`, `on-failure` (with
/// its count, when one is set).
fn restart_of(policy: &Value) -> String {
    let name = text(&policy["Name"]);
    let most = policy["MaximumRetryCount"].as_u64().unwrap_or(0);
    if name == "on-failure" && most > 0 {
        return format!("on-failure:{most}");
    }
    if name.is_empty() {
        return "no".to_owned();
    }
    name
}

/// The services a compose `depends_on` label names (`node:service_started:true,db:…`).
fn depends_of(label: &str) -> Vec<String> {
    label
        .split(',')
        .filter_map(|one| one.split(':').next())
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.chars().filter(|c| !c.is_control()).take(80).collect())
        .collect()
}

/// One container's inspection (Docker's `/containers/{id}/json`, or a row of
/// `/containers/json`) cut down to what the map needs: its id, name, image (`repo:tag`), state,
/// restart policy, network mode and three compose labels. Nothing else is carried: no
/// environment, no mounts, no binds, no other label.
#[must_use]
pub fn project(inspected: &Value) -> Value {
    let id = text(&inspected["Id"]);
    let id = if id.len() == 64 && id.bytes().all(|b| b.is_ascii_hexdigit()) {
        id
    } else {
        String::new()
    };
    let name = if inspected["Name"].is_string() {
        text(&inspected["Name"])
    } else {
        text(&inspected["Names"][0])
    };
    let image = if inspected["Config"]["Image"].is_string() {
        &inspected["Config"]["Image"]
    } else {
        &inspected["Image"]
    };
    let state = if inspected["State"].is_object() {
        text(&inspected["State"]["Status"])
    } else {
        text(&inspected["State"])
    };
    let labels = if inspected["Config"]["Labels"].is_object() {
        &inspected["Config"]["Labels"]
    } else {
        &inspected["Labels"]
    };
    let host = &inspected["HostConfig"];
    json!({
        "id": id,
        "name": name.trim_start_matches('/'),
        "image": image_of(&text(image)),
        "state": state,
        "restart": if host["RestartPolicy"].is_object() { restart_of(&host["RestartPolicy"]) } else { String::new() },
        "network_mode": text(&host["NetworkMode"]),
        "service": text(&labels[COMPOSE_LABELS[0]]),
        "project": text(&labels[COMPOSE_LABELS[1]]),
        "depends_on": depends_of(&text(&labels[COMPOSE_LABELS[2]])),
    })
}

/// Letters and digits only, lowercase: `home-assistant`, `home_assistant` and
/// `homeassistant` are one name.
fn plain(name: &str) -> String {
    name.chars()
        .filter(char::is_ascii_alphanumeric)
        .map(|c| c.to_ascii_lowercase())
        .collect()
}

/// Who manages a container: `Endora` for Endora's own (its name starts `endora`, or it is of
/// Endora's compose project), `ansible-nas role <name>` when an ansible-nas role is named as
/// it is, or `unknown`, which is also what every other container is when the role list is not
/// known (`roles` is `None`).
#[must_use]
pub fn managed_by(name: &str, project: &str, roles: Option<&[String]>) -> String {
    let lower = name.trim_start_matches('/').to_ascii_lowercase();
    if lower.starts_with("endora") || project.eq_ignore_ascii_case("endora") {
        return "Endora".to_owned();
    }
    let wanted = plain(&lower);
    let role = roles
        .into_iter()
        .flatten()
        .find(|r| r.eq_ignore_ascii_case(&lower))
        .or_else(|| {
            roles
                .into_iter()
                .flatten()
                .find(|r| !wanted.is_empty() && plain(r) == wanted)
        });
    match role {
        Some(role) => format!("ansible-nas role {role}"),
        None => "unknown".to_owned(),
    }
}

/// The owner's retired list (`ENDORA_LAB_RETIRED`): `name=note` pairs, `;` between them.
/// A pair without a name or a note is skipped.
#[must_use]
pub fn retired_of(said: &str) -> Vec<(String, String)> {
    said.split(';')
        .filter_map(|pair| pair.split_once('='))
        .map(|(name, note)| {
            let note: String = note.chars().filter(|c| !c.is_control()).take(200).collect();
            (name.trim().to_owned(), note.trim().to_owned())
        })
        .filter(|(name, note)| !name.is_empty() && !note.is_empty())
        .collect()
}

/// How one container hangs on another.
#[derive(Clone, Copy, PartialEq, Eq)]
enum How {
    /// It runs in the other's network (`NetworkMode: container:<id>`).
    Network,
    /// Compose starts it after the other (`depends_on`).
    Compose,
}

/// The map: every container (sorted by name) with its image, state, restart policy, who
/// manages it, what it depends on and what depends on it, and the owner's note if it is on
/// the retired list; the wiring, one line a container others hang on ("gluetun → deluge,
/// nzbget, prowlarr (run in its network)"); and the containers grouped by who manages them.
/// `containers` are [`project`]'s; `roles` the ansible-nas role names, `None` when unknown.
#[must_use]
pub fn build(
    containers: &[Value],
    roles: Option<&[String]>,
    retired: &[(String, String)],
) -> Value {
    let mut all: Vec<&Value> = containers.iter().filter(|c| c["name"] != "").collect();
    all.sort_by(|a, b| a["name"].as_str().cmp(&b["name"].as_str()));
    let name_of = |v: &Value| v["name"].as_str().unwrap_or("").to_owned();
    // What `container:<x>` names: an id (whole, or its first 12 or more), or a name.
    let network_of = |mode: &str| -> Option<String> {
        let target = mode.strip_prefix("container:")?.trim();
        if target.is_empty() {
            return None;
        }
        let found = all.iter().find(|c| {
            let id = c["id"].as_str().unwrap_or("");
            (!id.is_empty() && target.len() >= 12 && id.starts_with(target)) || c["name"] == target
        });
        Some(found.map_or_else(
            || {
                format!(
                    "an unknown container {}",
                    target.chars().take(12).collect::<String>()
                )
            },
            |c| name_of(c),
        ))
    };
    // What a compose service names: the container of that service in the same project.
    let service_of = |service: &str, project: &str| -> String {
        all.iter()
            .find(|c| c["service"] == service && c["project"] == project)
            .or_else(|| all.iter().find(|c| c["service"] == service))
            .map_or_else(|| service.to_owned(), |c| name_of(c))
    };
    let mut edges: Vec<(String, String, How)> = Vec::new(); // (on, by, how)
    for c in &all {
        let by = name_of(c);
        if let Some(on) = network_of(c["network_mode"].as_str().unwrap_or("")) {
            edges.push((on, by.clone(), How::Network));
        }
        for service in c["depends_on"].as_array().into_iter().flatten() {
            let on = service_of(
                service.as_str().unwrap_or(""),
                c["project"].as_str().unwrap_or(""),
            );
            if on != by && !edges.iter().any(|(o, b, _)| *o == on && *b == by) {
                edges.push((on, by.clone(), How::Compose));
            }
        }
    }
    let mut groups: Vec<(String, Vec<String>)> = Vec::new();
    let items: Vec<Value> = all
        .iter()
        .map(|c| {
            let name = name_of(c);
            let manager = managed_by(&name, c["project"].as_str().unwrap_or(""), roles);
            let group = if manager.starts_with("ansible-nas") {
                "ansible-nas"
            } else {
                manager.as_str()
            }
            .to_owned();
            match groups.iter_mut().find(|(g, _)| *g == group) {
                Some((_, names)) => names.push(name.clone()),
                None => groups.push((group, vec![name.clone()])),
            }
            let mut item = json!({ "name": name, "state": c["state"], "managed_by": manager });
            for key in ["image", "restart"] {
                if c[key].as_str().is_some_and(|v| !v.is_empty()) {
                    item[key] = c[key].clone();
                }
            }
            let on: Vec<&str> = edges
                .iter()
                .filter(|(_, b, _)| *b == name)
                .map(|(o, _, _)| o.as_str())
                .collect();
            if !on.is_empty() {
                item["depends_on"] = json!(on);
            }
            if let Some((o, _, _)) = edges
                .iter()
                .find(|(_, b, h)| *b == name && *h == How::Network)
            {
                item["in_network_of"] = o.as_str().into();
            }
            let by: Vec<&str> = edges
                .iter()
                .filter(|(o, _, _)| *o == name)
                .map(|(_, b, _)| b.as_str())
                .collect();
            if !by.is_empty() {
                item["needed_by"] = json!(by);
            }
            if let Some((_, note)) = retired.iter().find(|(r, _)| r.eq_ignore_ascii_case(&name)) {
                item["note"] = note.as_str().into();
            }
            item
        })
        .collect();
    json!({
        "wiring": wiring(&edges),
        "managed_by": groups.into_iter().map(|(g, names)| json!({ "by": g, "containers": names })).collect::<Vec<_>>(),
        "containers": items,
    })
}

/// One line a container others hang on, and how: "gluetun → deluge, nzbget, prowlarr (run in
/// its network)"; "node → hq (compose starts it after)".
fn wiring(edges: &[(String, String, How)]) -> Vec<String> {
    let mut lines = Vec::new();
    let mut ons: Vec<&str> = edges.iter().map(|(o, _, _)| o.as_str()).collect();
    ons.sort_unstable();
    ons.dedup();
    for on in ons {
        for how in [How::Network, How::Compose] {
            let mut by: Vec<&str> = edges
                .iter()
                .filter(|(o, _, h)| o == on && *h == how)
                .map(|(_, b, _)| b.as_str())
                .collect();
            if by.is_empty() {
                continue;
            }
            by.sort_unstable();
            let said = match (how, by.len()) {
                (How::Network, 1) => "runs in its network",
                (How::Network, _) => "run in its network",
                (How::Compose, 1) => "compose starts it after",
                (How::Compose, _) => "compose starts them after",
            };
            lines.push(format!("{on} → {} ({said})", by.join(", ")));
        }
    }
    lines
}

#[cfg(test)]
mod tests {
    use super::*;

    fn id(n: u8) -> String {
        format!("{n:02x}").repeat(32)
    }

    /// A container as Docker inspects it, with what the map must never carry.
    fn inspected(n: u8, name: &str, network: &str) -> Value {
        json!({
            "Id": id(n), "Name": format!("/{name}"), "Image": format!("sha256:{}", id(n)),
            "State": { "Status": "running", "Pid": 42 },
            "Config": {
                "Image": format!("lscr.io/linuxserver/{name}:latest@sha256:{}", id(9)),
                "Env": ["PUID=1000", "DELUGE_PASSWORD=fictional-secret"],
                "Labels": { "traefik.http.middlewares.auth.basicauth.users": "admin:$apr1$fictional" },
            },
            "HostConfig": {
                "NetworkMode": network,
                "RestartPolicy": { "Name": "unless-stopped", "MaximumRetryCount": 0 },
                "Binds": ["/srv/fictional/config:/config:rw"],
            },
            "Mounts": [{ "Source": "/srv/fictional/downloads", "Destination": "/downloads" }],
        })
    }

    fn roles() -> Vec<String> {
        ["deluge", "gluetun", "nzbget", "prowlarr", "home_assistant"]
            .map(str::to_owned)
            .to_vec()
    }

    #[test]
    fn what_runs_in_another_containers_network_is_resolved_to_its_name_and_both_ways() {
        let lab: Vec<Value> = [
            inspected(1, "gluetun", "bridge"),
            inspected(2, "deluge", &format!("container:{}", id(1))),
            inspected(3, "nzbget", &format!("container:{}", id(1))),
            inspected(4, "prowlarr", &format!("container:{}", &id(1)[..12])),
            inspected(5, "jellyfin", "host"),
        ]
        .iter()
        .map(project)
        .collect();
        let map = build(&lab, Some(&roles()), &[]);
        assert_eq!(
            map["wiring"],
            json!(["gluetun → deluge, nzbget, prowlarr (run in its network)"])
        );
        let of = |name: &str| {
            map["containers"]
                .as_array()
                .unwrap()
                .iter()
                .find(|c| c["name"] == name)
                .unwrap()
                .clone()
        };
        assert_eq!(of("deluge")["depends_on"], json!(["gluetun"]));
        assert_eq!(of("deluge")["in_network_of"], "gluetun");
        assert_eq!(of("prowlarr")["in_network_of"], "gluetun", "a short id too");
        assert_eq!(
            of("gluetun")["needed_by"],
            json!(["deluge", "nzbget", "prowlarr"])
        );
        assert!(
            of("jellyfin").get("depends_on").is_none() && of("jellyfin").get("needed_by").is_none()
        );
        assert_eq!(
            of("deluge")["image"],
            "lscr.io/linuxserver/deluge:latest",
            "no digest"
        );
        assert_eq!(of("deluge")["restart"], "unless-stopped");
        assert_eq!(of("deluge")["state"], "running");
    }

    #[test]
    fn a_network_owner_that_is_gone_and_compose_depends_on_are_said_too() {
        let mut observer = project(&inspected(
            6,
            "endora-docker-observer-1",
            "container:endora-node",
        ));
        observer["depends_on"] = json!(["node"]);
        observer["project"] = "endora".into();
        let mut node = project(&inspected(7, "endora-node", "bridge"));
        node["service"] = "node".into();
        node["project"] = "endora".into();
        let mut hq = project(&inspected(8, "endora-hq", "bridge"));
        hq["project"] = "endora".into();
        let mut label = inspected(8, "endora-hq", "bridge");
        label["Config"]["Labels"]["com.docker.compose.depends_on"] =
            "node:service_started:true".into();
        hq["depends_on"] = project(&label)["depends_on"].clone();
        let orphan = project(&inspected(9, "sabnzbd", &format!("container:{}", id(10))));
        let map = build(&[observer, node, hq, orphan], Some(&roles()), &[]);
        assert_eq!(
            map["wiring"],
            json!([
                format!(
                    "an unknown container {} → sabnzbd (runs in its network)",
                    &id(10)[..12]
                ),
                "endora-node → endora-docker-observer-1 (runs in its network)",
                "endora-node → endora-hq (compose starts it after)",
            ])
        );
    }

    #[test]
    fn who_manages_a_container_comes_from_the_role_list() {
        let roles = roles();
        assert_eq!(
            managed_by("deluge", "", Some(&roles)),
            "ansible-nas role deluge"
        );
        assert_eq!(
            managed_by("/gluetun", "", Some(&roles)),
            "ansible-nas role gluetun"
        );
        assert_eq!(
            managed_by("home-assistant", "", Some(&roles)),
            "ansible-nas role home_assistant",
            "one name, however it is joined"
        );
        assert_eq!(managed_by("endora-node", "", Some(&roles)), "Endora");
        assert_eq!(
            managed_by("docker-observer-1", "endora", Some(&roles)),
            "Endora"
        );
        assert_eq!(managed_by("quirky_hopper", "", Some(&roles)), "unknown");
        assert_eq!(
            managed_by("deluge", "", None),
            "unknown",
            "no role list, no guess"
        );
        let lab = vec![
            project(&inspected(1, "deluge", "")),
            project(&inspected(2, "endora-hq", "")),
            project(&inspected(3, "mystery", "")),
        ];
        let map = build(
            &lab,
            Some(&roles),
            &[(
                "Mystery".to_owned(),
                "retired 17 Oct: kept for its settings".to_owned(),
            )],
        );
        assert_eq!(
            map["managed_by"],
            json!([
                { "by": "ansible-nas", "containers": ["deluge"] },
                { "by": "Endora", "containers": ["endora-hq"] },
                { "by": "unknown", "containers": ["mystery"] },
            ])
        );
        assert_eq!(
            map["containers"][2]["note"],
            "retired 17 Oct: kept for its settings"
        );
        assert!(map["containers"][0].get("note").is_none());
    }

    #[test]
    fn the_environment_mounts_binds_and_other_labels_never_reach_the_map() {
        let raw = inspected(2, "deluge", &format!("container:{}", id(1)));
        let projected = project(&raw);
        let map = build(&[projected.clone()], Some(&roles()), &[]);
        for said in [projected.to_string(), map.to_string()] {
            for never in [
                "Env",
                "PASSWORD",
                "fictional-secret",
                "PUID",
                "Mounts",
                "/srv",
                "Binds",
                "/config",
                "basicauth",
                "apr1",
                "Pid",
                "sha256",
            ] {
                assert!(!said.contains(never), "{never} in {said}");
            }
        }
        let keys: Vec<&String> = projected.as_object().unwrap().keys().collect();
        assert_eq!(
            keys,
            [
                "depends_on",
                "id",
                "image",
                "name",
                "network_mode",
                "project",
                "restart",
                "service",
                "state"
            ]
        );
        // A secret-shaped value where a name should be is scrubbed, not passed on.
        let mut sneaky = raw;
        sneaky["Config"]["Labels"]["com.docker.compose.project"] =
            "token=ghp_fictionalfictionalfictionalfictional1".into();
        assert!(!project(&sneaky).to_string().contains("ghp_fictional"));
    }

    #[test]
    fn a_row_of_dockers_list_projects_alike() {
        let row = json!({
            "Id": id(1), "Names": ["/gluetun"], "Image": "qmcgaw/gluetun:v3", "State": "running",
            "Labels": { "com.docker.compose.service": "gluetun" },
            "HostConfig": { "NetworkMode": "bridge" },
        });
        let seen = project(&row);
        assert_eq!(
            (
                seen["name"].as_str(),
                seen["image"].as_str(),
                seen["state"].as_str()
            ),
            (Some("gluetun"), Some("qmcgaw/gluetun:v3"), Some("running"))
        );
        assert_eq!(seen["service"], "gluetun");
        assert_eq!(seen["restart"], "", "the list does not say");
    }

    #[test]
    fn the_retired_list_is_name_equals_note_pairs() {
        assert_eq!(
            retired_of(
                "owntone=retired 17 Oct, kept for its settings; =no name; bare; old = gone "
            ),
            [
                (
                    "owntone".to_owned(),
                    "retired 17 Oct, kept for its settings".to_owned()
                ),
                ("old".to_owned(), "gone".to_owned())
            ]
        );
        assert!(retired_of("").is_empty());
        assert_eq!(
            restart_of(&json!({ "Name": "on-failure", "MaximumRetryCount": 5 })),
            "on-failure:5"
        );
        assert_eq!(restart_of(&json!({ "Name": "" })), "no");
    }
}
