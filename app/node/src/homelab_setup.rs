use endora_capabilities::{CapabilitySettingsRepository, ConfigStore};
use std::io::{self, Write};

pub fn run() -> Result<(), Box<dyn std::error::Error>> {
    let db = std::env::var("ENDORA_DB").unwrap_or_else(|_| "endora.db".into());
    // The same key file as the node, or the keys entered here could not be read by it.
    let store = crate::secrets::open_store(&db)?;
    let config = ConfigStore::new(store.db());
    println!("Host-only dashboard setup. Use the same ENDORA_DB and OS user as the running node.");
    println!("Docker must point at the restricted dashboard proxy, never the raw daemon.");
    for service in ["docker", "sonarr", "radarr"] {
        print!("{service} origin (blank preserves current; '-' disables): ");
        io::stdout().flush()?;
        let mut origin = String::new();
        if io::stdin().read_line(&mut origin)? == 0 {
            return Err("Setup cancelled".into());
        }
        let origin = origin.trim();
        if origin.is_empty() {
            continue;
        }
        let id = format!("homelab-{service}");
        if origin == "-" {
            config.set_setting(&id, "origin", "")?;
            config.set_setting(&id, "api_key", "")?;
            continue;
        }
        endora_capabilities::infrastructure::homelab::validate_origin(origin)
            .map_err(|_| "Invalid service origin; no path, query or credentials allowed")?;
        if service != "docker" {
            let key = rpassword::prompt_password("API key (masked): ")?;
            if key.is_empty() {
                return Err("API key is required".into());
            }
            config.set_setting(&id, "api_key", &key)?;
        }
        config.set_setting(&id, "origin", origin)?;
    }
    println!("Private settings saved. Dashboard refreshes within the next polling cycle.");
    Ok(())
}
