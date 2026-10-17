//! Which tier each skill's result is (ADR 0105), in one place to review. A skill that brings
//! back a stranger's words (`third_party`) is marked as from outside instead, and nothing it
//! says leaves the house; this table is for the skills that read the house itself. A skill
//! not listed is Restricted, so a new one fails safe until it is classified here.

use endora_kernel::Tier;

/// The tier of a skill's result, when this table names it.
#[must_use]
pub fn known_tier(id: &str) -> Option<Tier> {
    Some(match id {
        // What anyone may know, about where the owner lives, and what the world's sources say
        // (these are marked as from outside as well, so nothing they say leaves).
        "weather" | "traffic" | "flights" | "safety_alerts" | "incident_scanner" | "web_search"
        | "web_fetch" | "knowledge" | "news" | "sports_schedule" | "city_meetings"
        | "ticketed_events" => Tier::Public,
        // The house's workings.
        "terminal-homepage"
        | "terminal-portainer"
        | "terminal-metrics"
        | "homelab-docker"
        | "containers"
        | "portainer_containers"
        | "nas_status" => Tier::Internal,
        // The owner's life: what they watch, who is home, what the network sees them do, what
        // Endora knows and did, the image they sent.
        "terminal-overseerr" | "homelab-sonarr" | "homelab-radarr" | "network-pihole"
        | "network-unifi" | "home_assistant" | "image_review" | "own_activity" | "read_page"
        | "mail" | "sonarr_queue" | "radarr_queue" => Tier::Confidential,
        // A movement history: no disguise hides a trail of places.
        "location" => Tier::Restricted,
        // A tool of a tool server the house has paired (`home.HassTurnOn`): only those reach a
        // turn without the outsider's mark, and they are the house speaking about itself.
        _ if id.contains('.') => Tier::Confidential,
        _ => return None,
    })
}

/// The tier of a skill's result: as listed, else Restricted.
#[must_use]
pub fn tier_of(id: &str) -> Tier {
    known_tier(id).unwrap_or(Tier::Restricted)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_built_in_skill_that_reads_the_house_is_classified() {
        let unlisted: Vec<&str> = crate::infrastructure::default_capabilities()
            .iter()
            .map(|c| c.info())
            .filter(|i| !i.third_party && known_tier(i.id).is_none())
            .map(|i| i.id)
            .collect();
        assert!(
            unlisted.is_empty(),
            "classify these in tiers.rs: {unlisted:?}"
        );
        assert_eq!(
            tier_of("a-skill-added-tomorrow"),
            Tier::Restricted,
            "fails safe"
        );
        assert_eq!(tier_of("location"), Tier::Restricted);
        assert_eq!(tier_of("homelab-docker"), Tier::Internal);
        assert_eq!(tier_of("home.HassTurnOn"), Tier::Confidential);
    }
}
