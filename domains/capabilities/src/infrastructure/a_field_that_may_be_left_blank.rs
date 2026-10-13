use super::*;

#[test]
fn an_optional_setting_does_not_hold_a_skill_back() {
    // Live: the Home Assistant skill read "needs setup" and was left out of the model's
    // catalogue entirely, with a URL and a token both set — because `mcp_server` is
    // documented as "blank = home-assistant" and had never been filled in. A field the
    // interface itself describes as optional cannot also be the reason a skill is off.
    let info = HomeAssistantCapability.info();
    assert!(
        info.settings.iter().any(|s| s.optional),
        "this skill is the reason the flag exists"
    );

    let mut only_the_required: CapabilitySettings = CapabilitySettings::new();
    for spec in info.settings.iter().filter(|s| !s.optional) {
        only_the_required.insert(spec.key.to_owned(), "something".to_owned());
    }
    assert!(
        settings_complete(&info, &only_the_required),
        "filling in every REQUIRED field must be enough"
    );
}

#[test]
fn a_required_setting_still_holds_it_back() {
    // The flag must not become a way for a skill to claim it is ready with nothing set.
    let info = HomeAssistantCapability.info();
    assert!(
        !settings_complete(&info, &CapabilitySettings::new()),
        "no URL and no token is not configured"
    );
}
