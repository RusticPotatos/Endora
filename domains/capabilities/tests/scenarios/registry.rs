//! The steps of `features/an-images-tags-from-its-registry.feature`: the team's `image_tags`
//! as the node runs it (`app/node/src/api/contracts.rs`), its arguments read by
//! [`registry::asked_of`] and looked up by [`registry::look_up`], against registries faked
//! from the scenario's words. Nothing reaches the network.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use cucumber::{given, then, when};
use endora_capabilities::registry::{self, Fetch};
use serde_json::{Value, json};

use super::Bridge;

/// The public registries, faked: each image's tags by registry host and repository, and every
/// address asked.
#[derive(Debug, Default)]
pub struct Registries {
    /// Tags in the registry's own order, by `(host, repository)`.
    images: HashMap<(String, String), Vec<String>>,
    /// Every address asked, in order.
    asked: Arc<Mutex<Vec<String>>>,
}

impl Registries {
    fn tags(&self, host: &str, repository: &str) -> Option<&Vec<String>> {
        self.images.get(&(host.to_owned(), repository.to_owned()))
    }

    /// Docker Hub's API: one page of tags, and a tag by its own address.
    fn docker_hub(&self, rest: &str) -> (u16, String) {
        let Some((repository, after)) = rest.split_once("/tags") else {
            return (404, String::new());
        };
        let Some(tags) = self.tags("docker.io", repository) else {
            return (404, String::new());
        };
        if let Some(tag) = after.strip_prefix('/') {
            return if tags.iter().any(|t| t == tag) {
                (200, json!({ "name": tag }).to_string())
            } else {
                (404, String::new())
            };
        }
        let results: Vec<Value> = tags.iter().map(|t| json!({ "name": t })).collect();
        (200, json!({ "results": results, "next": null }).to_string())
    }

    /// An OCI registry's API: an anonymous pull token, then the whole tag list on one page.
    fn oci(&self, host: &str, rest: &str) -> (u16, String) {
        if let Some(query) = rest
            .strip_prefix("token?")
            .or_else(|| rest.strip_prefix("v2/auth?"))
        {
            let repository = query
                .split("scope=repository:")
                .nth(1)
                .and_then(|r| r.strip_suffix(":pull"))
                .unwrap_or_default();
            return match self.tags(host, repository) {
                Some(_) => (200, json!({ "token": "anonymous" }).to_string()),
                None => (401, String::new()),
            };
        }
        let Some((repository, _)) = rest
            .strip_prefix("v2/")
            .and_then(|r| r.split_once("/tags/list"))
        else {
            return (404, String::new());
        };
        match self.tags(host, repository) {
            Some(tags) => (200, json!({ "tags": tags }).to_string()),
            None => (404, String::new()),
        }
    }
}

impl Fetch for Registries {
    fn get(&self, url: &str, _bearer: Option<&str>) -> Result<(u16, String), &'static str> {
        self.asked
            .lock()
            .expect("the list of addresses asked")
            .push(url.to_owned());
        if let Some(rest) = url.strip_prefix("https://hub.docker.com/v2/repositories/") {
            return Ok(self.docker_hub(rest));
        }
        for host in ["ghcr.io", "quay.io"] {
            if let Some(rest) = url.strip_prefix(&format!("https://{host}/")) {
                return Ok(self.oci(host, rest));
            }
        }
        Err("refused")
    }
}

#[given(regex = r#"^(Docker Hub|ghcr.io|quay.io) has the image "(.+)" with the tags "(.+)"$"#)]
fn registry_has(w: &mut Bridge, registry: String, repository: String, tags: String) {
    let host = if registry == "Docker Hub" {
        "docker.io".to_owned()
    } else {
        registry
    };
    let tags = tags.split(", ").map(str::to_owned).collect();
    w.registries.images.insert((host, repository), tags);
}

/// A member's `image_tags` call, as the node answers it.
fn looks_up(w: &mut Bridge, args: &Value) {
    w.answer = None;
    w.refused = None;
    let asked = match registry::asked_of(args) {
        Ok(asked) => asked,
        Err(why) => {
            w.refused = Some(why);
            return;
        }
    };
    match registry::look_up(&w.registries, &asked) {
        Ok(answer) => w.answer = Some(answer),
        Err(failed) => w.refused = Some(failed.said().to_owned()),
    }
}

#[when(expr = "a member looks up the tags of {string}, asking for {string}")]
fn looks_up_asking(w: &mut Bridge, image: String, tag: String) {
    looks_up(w, &json!({ "image": image, "tag": tag }));
}

#[when(expr = "a member looks up the plain releases of {string}")]
fn looks_up_releases(w: &mut Bridge, image: String) {
    looks_up(w, &json!({ "image": image, "semver_only": true }));
}

fn answer(w: &Bridge) -> &Value {
    w.answer
        .as_ref()
        .unwrap_or_else(|| panic!("the lookup was refused: {:?}", w.refused))
}

#[then(regex = r#"^the answer says "(.+)" (exists|does not exist)$"#)]
fn answer_says_exists(w: &mut Bridge, tag: String, exists: String) {
    let answer = answer(w);
    assert_eq!(answer["tag"], json!(tag), "{answer}");
    assert_eq!(answer["exists"], json!(exists == "exists"), "{answer}");
}

#[then(expr = "the newest tags are {string}")]
fn newest_tags(w: &mut Bridge, wanted: String) {
    let answer = answer(w);
    let newest: Vec<&str> = answer["newest"]
        .as_array()
        .expect("the newest tags")
        .iter()
        .filter_map(Value::as_str)
        .collect();
    assert_eq!(newest.join(", "), wanted, "{answer}");
}

#[then(expr = "the answer notes {string}")]
fn answer_notes(w: &mut Bridge, text: String) {
    let answer = answer(w);
    let note = answer["note"].as_str().unwrap_or_default();
    assert!(note.contains(&text), "{answer}");
}

#[then("no registry was asked")]
fn no_registry_asked(w: &mut Bridge) {
    let asked = w
        .registries
        .asked
        .lock()
        .expect("the list of addresses asked");
    assert!(asked.is_empty(), "asked: {asked:?}");
}
