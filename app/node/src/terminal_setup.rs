//! Interactive private setup for the explicit handheld console.
use endora_capabilities::{CapabilitySettingsRepository, ConfigStore};
use endora_infrastructure::SqliteStore;
use std::io::{self, Write};

fn prompt(label: &str) -> Result<String, Box<dyn std::error::Error>> {
    print!("{label}");
    io::stdout().flush()?;
    let mut text = String::new();
    if io::stdin().read_line(&mut text)? == 0 {
        return Err("Setup cancelled".into());
    }
    Ok(text.trim().to_owned())
}
pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let db = std::env::var("ENDORA_DB").unwrap_or_else(|_| "endora.db".into());
    let store = SqliteStore::open(&db)?;
    let config = ConfigStore::new(store.db());
    println!("Private handheld setup. Run on the node host with its ENDORA_DB.");
    println!("Blank preserves existing values; '-' disables a source. API keys are masked.");
    for service in ["overseerr", "portainer", "metrics"] {
        let origin = prompt(&format!("{service} origin: "))?;
        let id = format!("terminal-{service}");
        if origin == "-" {
            config.set_setting(&id, "origin", "")?;
            config.set_setting(&id, "api_key", "")?;
            continue;
        }
        if !origin.is_empty() {
            endora_capabilities::infrastructure::homelab::validate_origin(&origin)
                .map_err(|_| "Use an HTTP(S) origin without path, query or credentials")?;
            if service != "metrics" {
                let key = rpassword::prompt_password("API key (blank preserves existing): ")?;
                if !key.is_empty() {
                    config.set_setting(&id, "api_key", &key)?;
                }
            }
            config.set_setting(&id, "origin", &origin)?;
        }
        let key = match service {
            "overseerr" => Some((
                "request_user",
                "Overseerr requesting user numeric ID (approval permissions apply; blank preserves): ",
            )),
            "portainer" => Some((
                "endpoint",
                "Portainer environment numeric ID (blank preserves): ",
            )),
            "metrics" => Some((
                "instance",
                "Prometheus Telegraf host label, e.g. ansible-nas (blank preserves): ",
            )),
            _ => None,
        };
        if let Some((key, label)) = key {
            let value = prompt(label)?;
            if !value.is_empty() {
                if matches!(key, "endpoint" | "request_user")
                    && (!value.bytes().all(|b| b.is_ascii_digit()) || value == "0")
                {
                    return Err("Environment ID must be numeric".into());
                }
                config.set_setting(&id, key, &value)?;
            }
        }
    }
    let favorites = prompt(
        "Home Assistant favorite entity IDs, comma separated (blank preserves; '-' clears): ",
    )?;
    if !favorites.is_empty() {
        if favorites != "-"
            && !favorites.split(',').all(|id| {
                let id = id.trim();
                id.contains('.')
                    && id
                        .bytes()
                        .all(|b| b.is_ascii_alphanumeric() || b"_.".contains(&b))
            })
        {
            return Err("Use exact entity IDs, separated by commas".into());
        }
        config.set_setting(
            "terminal-home",
            "favorites",
            if favorites == "-" { "" } else { &favorites },
        )?;
    }
    let key = rpassword::prompt_password("Private Ansible job agent key (blank preserves): ")?;
    if !key.is_empty() {
        if key.len() < 32 {
            return Err("Use a job agent key of at least 32 characters".into());
        }
        config.set_setting("terminal-jobs", "api_key", &key)?;
    }
    println!("Private settings saved. Home Assistant reuses its existing url/token configuration.");
    Ok(())
}
