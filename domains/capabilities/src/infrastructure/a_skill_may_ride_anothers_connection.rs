//! Live, within an hour of shipping: the mail skill reported itself unconfigured while
//! the Home Assistant connection it reads through was right there and working. Settings
//! are keyed by a capability's own id, and a skill that borrows a connection and
//! declares nothing gets an empty map.
//!
//! Declaring the same settings again would have worked and would have made the person
//! paste the same token twice, which is how two copies of a credential start.

use crate::application::CapabilityRunner;
use std::sync::Arc;

#[test]
fn a_borrower_reads_the_settings_of_what_it_borrows() {
    let mut settings = std::collections::HashMap::new();
    let mut house = super::CapabilitySettings::new();
    house.insert("url".to_owned(), "https://192.168.1.10:8123".to_owned());
    house.insert("token".to_owned(), "a-token".to_owned());
    settings.insert("home_assistant".to_owned(), house);

    let runner = super::RegistryRunner::with_config(
        Arc::new(super::default_capabilities()),
        Vec::new(),
        std::collections::HashSet::new(),
        crate::application::AutonomyEnvelope::default(),
        settings,
    );
    let mail = runner
        .available()
        .into_iter()
        .find(|c| c.id == "mail")
        .expect("the skill is registered");
    assert!(
        mail.configured,
        "a borrower is ready when the connection it rides is ready — it reported \
         unconfigured while the house was connected and working"
    );
}

#[test]
fn a_borrower_with_nothing_to_borrow_is_honestly_unconfigured() {
    let runner = super::RegistryRunner::new(Arc::new(super::default_capabilities()));
    let mail = runner
        .available()
        .into_iter()
        .find(|c| c.id == "mail")
        .expect("the skill is registered");
    assert!(!mail.configured);
}
