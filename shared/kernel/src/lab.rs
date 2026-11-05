//! How the home lab is built (ADR 0112): the standing knowledge every member of HQ's team is
//! given, in the CEO's house log (seeded once, when it is empty) and in the worker's home-lab
//! instructions. One text in one place, so the two cannot drift apart. Words only: what the
//! lab actually holds is the `homelab_map` contract's to say. And the one rule of which paths
//! the team may never touch ([`forbidden_path`]).

/// The title of the house log's entry.
pub const LAB_BUILT_TITLE: &str = "How this lab is built";

/// What the team knows about how the home lab is built, a sentence or two each.
pub const LAB_BUILT: [&str; 3] = [
    "The home lab is managed by ansible-nas: each app is a role, and the playbook creates its \
     container. homelab_map shows who owns what and what depends on what.",
    "A container that crashed may be restarted; anything else (settings, memory limits, new or \
     removed containers) is a proposal to change ansible-nas, for the owner to approve and run.",
    "Containers that run in another's network (e.g. behind gluetun) stop when it restarts and \
     come back on their own: check homelab_map and the current state before treating that as \
     a fault.",
];

/// [`LAB_BUILT`] as one short paragraph, as the worker's instructions carry it.
#[must_use]
pub fn lab_built_paragraph() -> String {
    LAB_BUILT.join(" ")
}

/// Whether a path is one the team may never touch (ADR 0115): the owner's values live in
/// `group_vars/*.yml`, `host_vars/` and every inventory, and anything named for a vault. The
/// node refuses a branch that changes one at submission, and HQ checks it again at review, by
/// this one rule, so it never rests on one place.
#[must_use]
pub fn forbidden_path(path: &str) -> bool {
    let lower = path.trim().trim_start_matches("./").to_ascii_lowercase();
    lower.split('/').any(|p| {
        matches!(
            p,
            "group_vars"
                | "host_vars"
                | "inventories"
                | "inventory"
                | "inventory.yml"
                | "inventory.ini"
        ) || p.contains("vault")
    })
}

/// Whether a path a branch changes is refused as the owner's values (ADR 0115, amended
/// 2026-11-05): [`forbidden_path`] in the lab's fork, whose layout those names describe, and no
/// path in a public repository outside the lab, whose `inventory/` package or
/// `vaultwarden_client.py` is its own code. This scopes a filename rule only: it does not say an
/// outside repository holds no secret, and the briefing's never-copy rule still applies there.
/// `outside_lab` is the trusted context:
/// the repository the owner named on the item's project, never a member's word. The node holds
/// a submission to it and HQ holds the review to it, so the two agree.
#[must_use]
pub fn refused_as_owners_values(path: &str, outside_lab: bool) -> bool {
    !outside_lab && forbidden_path(path)
}

/// The only addresses an outside repository may have (ADR 0115, amended 2026-11-01).
pub const OUTSIDE_REPO_SHAPE: &str = "https://github.com/<owner>/<name>";

/// The repository outside the lab an item names (ADR 0115, amended 2026-11-01: round 4's F4, a
/// small task in a public open-source repository), as one address: only a public
/// `https://github.com/<owner>/<name>` (with `.git` or a trailing `/` at most), returned in that
/// plain form. Anything else is refused: another scheme or host, a credential, a port, a query,
/// a path past the repository, a name git or GitHub would not take. HQ checks it when the owner
/// sets it and the node again before it mirrors it, by this one rule.
///
/// # Errors
/// Why not, as the owner reads it.
pub fn outside_repo(url: &str) -> Result<String, String> {
    let refused = || {
        Err(format!(
            "only a public {OUTSIDE_REPO_SHAPE} may be named as an outside repository, not {:?}",
            url.trim()
        ))
    };
    let Some(rest) = url.trim().strip_prefix("https://github.com/") else {
        return refused();
    };
    let rest = rest.strip_suffix('/').unwrap_or(rest);
    let rest = rest.strip_suffix(".git").unwrap_or(rest);
    let Some((owner, name)) = rest.split_once('/') else {
        return refused();
    };
    let owner_ok = (1..=39).contains(&owner.len())
        && owner
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || b == b'-')
        && !owner.starts_with('-');
    let name_ok = (1..=100).contains(&name.len())
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'-' | b'_' | b'.'))
        && !name.starts_with('.');
    if !owner_ok || !name_ok {
        return refused();
    }
    Ok(format!("https://github.com/{owner}/{name}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The autonomy climb's round-1 fixtures: harmless software paths, and the lab's protected
    /// ones. Outside the lab, every path is the repository's own; in the lab, every protected
    /// path is still refused, and so is a harmless-looking one the old rule caught.
    #[test]
    fn the_owners_values_are_refused_in_the_lab_and_nowhere_else() {
        let harmless = [
            "shopkeep/inventory/stock.py",
            "inventory/models.py",
            "app/vaultwarden_client.py",
            "pkg/vaulted_cache.py",
            "src/inventory.py",
            "docs/inventory.md",
        ];
        let protected = [
            "inventory/hosts.yml",
            "inventories/prod/hosts",
            "group_vars/all.yml",
            "host_vars/nas.yml",
            "inventory.yml",
            "inventory.ini",
            "vault.yml",
            "roles/x/vars/vault.yml",
            "./Group_Vars/all.yml",
        ];
        for path in harmless.iter().chain(&protected) {
            assert!(!refused_as_owners_values(path, true), "{path} outside");
        }
        for path in protected {
            assert!(refused_as_owners_values(path, false), "{path} in the lab");
        }
        assert!(refused_as_owners_values("app/vaultwarden_client.py", false));
        assert!(!refused_as_owners_values("src/inventory.py", false));
    }

    #[test]
    fn an_outside_repository_is_a_public_github_https_address_in_one_plain_form() {
        for said in [
            "https://github.com/example/tiny",
            " https://github.com/example/tiny.git ",
            "https://github.com/example/tiny/",
        ] {
            assert_eq!(
                outside_repo(said).as_deref(),
                Ok("https://github.com/example/tiny")
            );
        }
        assert_eq!(
            outside_repo("https://github.com/Jane-Doe/py_lib.v2").as_deref(),
            Ok("https://github.com/Jane-Doe/py_lib.v2")
        );
    }

    #[test]
    fn any_other_address_is_refused() {
        for said in [
            "",
            "http://github.com/example/tiny",
            "git@github.com:example/tiny.git",
            "ssh://git@github.com/example/tiny",
            "https://gitlab.com/example/tiny",
            "https://github.com.example.com/example/tiny",
            "https://www.github.com/example/tiny",
            // A credential in the address (split so it reads as no mailbox).
            concat!("https://user:token", "@", "github.com/example/tiny"),
            "https://github.com:443/example/tiny",
            "https://github.com/example",
            "https://github.com/example/tiny/tree/main",
            "https://github.com/example/tiny?tab=readme",
            "https://github.com/example/tiny#readme",
            "https://github.com/example/..",
            "https://github.com/../tiny",
            "https://github.com/-x/tiny",
            "https://github.com/example/ tiny",
            "file:///srv/tiny.git",
            "/srv/tiny.git",
        ] {
            let why = outside_repo(said).unwrap_err();
            assert!(why.contains(OUTSIDE_REPO_SHAPE), "{said}: {why}");
        }
    }

    #[test]
    fn the_paragraph_is_the_three_sentences_on_one_line() {
        let said = lab_built_paragraph();
        assert!(said.starts_with("The home lab is managed by ansible-nas"));
        assert!(said.contains("homelab_map"));
        assert!(said.contains("behind gluetun"));
        assert!(!said.contains('\n'));
        assert!(
            LAB_BUILT.iter().all(|s| !s.contains("  ")),
            "no doubled spaces"
        );
    }
}
