use endora_terminal::bluetooth::{Device, alias_ok, is_mac, parse_devices, pick};

#[test]
fn bluetoothctl_device_lines_become_named_devices_and_noise_is_dropped() {
    let text = "Device AA:BB:CC:DD:EE:01 Fictional Speaker\n\
                Device AA:BB:CC:DD:EE:02 AA-BB-CC-DD-EE-02\n\
                Device AA:BB:CC:DD:EE:03 Test Phone\n\
                [NEW] Device AA:BB:CC:DD:EE:04 Ignored Prefix\n\
                Device not-a-mac Broken\n\
                Device AA:BB:CC:DD:EE:01 Fictional Speaker\n\
                garbage line\n";
    let devices = parse_devices(text);
    assert_eq!(
        devices,
        vec![
            Device {
                mac: "AA:BB:CC:DD:EE:01".into(),
                name: "Fictional Speaker".into()
            },
            Device {
                mac: "AA:BB:CC:DD:EE:03".into(),
                name: "Test Phone".into()
            },
        ],
        "unnamed (MAC-as-name) entries, bad addresses and duplicates are dropped"
    );
}

#[test]
fn only_well_formed_addresses_and_plain_aliases_are_accepted() {
    assert!(is_mac("AA:BB:CC:DD:EE:0F"));
    assert!(is_mac("aa:bb:cc:dd:ee:0f"));
    for bad in [
        "",
        "AA:BB:CC:DD:EE",
        "AA:BB:CC:DD:EE:0G",
        "AA:BB:CC:DD:EE:01 --help",
        "../etc",
        "AA-BB-CC-DD-EE-01",
    ] {
        assert!(!is_mac(bad), "{bad}");
    }
    assert!(alias_ok("Speaker"));
    assert!(alias_ok("Office audio 2"));
    for bad in [
        "",
        "   ",
        "name\nwith newline",
        "x".repeat(40).as_str(),
        "semi;colon",
        "quote\"d",
    ] {
        assert!(!alias_ok(bad), "{bad:?}");
    }
}

#[test]
fn a_menu_number_picks_a_device_and_anything_else_picks_nothing() {
    let devices = vec![
        Device {
            mac: "AA:BB:CC:DD:EE:01".into(),
            name: "One".into(),
        },
        Device {
            mac: "AA:BB:CC:DD:EE:02".into(),
            name: "Two".into(),
        },
    ];
    assert_eq!(pick(&devices, "2").unwrap().name, "Two");
    assert_eq!(pick(&devices, " 1 ").unwrap().name, "One");
    for bad in ["", "0", "3", "-1", "x", "1 2"] {
        assert!(pick(&devices, bad).is_none(), "{bad:?}");
    }
}

#[test]
fn the_pairing_script_waits_for_startup_then_replaces_the_agent_in_order() {
    use endora_terminal::bluetooth::pairing_steps;
    let steps = pairing_steps(120);
    let text: String = steps.iter().map(|s| s.text.as_str()).collect();
    let at = |needle: &str| {
        text.find(needle)
            .unwrap_or_else(|| panic!("{needle} missing"))
    };
    assert!(at("power on") < at("agent off"));
    assert!(at("agent off") < at("agent NoInputNoOutput"));
    assert!(at("agent NoInputNoOutput") < at("default-agent"));
    assert!(at("default-agent") < at("discoverable on"));
    // bluetoothctl registers its own agent a moment after it starts: wait before replacing it,
    // and give each step time to finish (without this the Mac shows a code nothing can confirm).
    let wait_before = |needle: &str| {
        steps
            .iter()
            .find(|s| s.text.contains(needle))
            .map(|s| s.wait_ms)
            .unwrap()
    };
    assert!(wait_before("agent off") >= 2500);
    assert!(wait_before("agent NoInputNoOutput") >= 1000);
    assert!(wait_before("default-agent") >= 1000);
    // Commands this bluetoothctl does not know are not sent, and the safety timeout outlives the window.
    assert!(!text.contains("pairable-timeout"));
    assert!(text.contains("discoverable-timeout 150"));
    assert!(
        text.contains("system-alias \"Pi Speaker\"\n"),
        "multi-word names must be quoted"
    );
}

#[test]
fn a_new_device_is_trusted_at_once_and_the_window_closes_a_few_seconds_later() {
    use endora_terminal::bluetooth::Settle;
    let mut settle = Settle::default();
    // Nothing new: keep waiting and trust nothing.
    assert_eq!(settle.tick(1, 1), (false, false));
    assert_eq!(settle.tick(1, 1), (false, false));
    // A device appears: trust it immediately, but do not close yet.
    assert_eq!(settle.tick(2, 1), (true, false));
    // Trust only once; close after the grace period so the first audio connection can finish.
    let mut closed_at = None;
    for n in 1..=12 {
        let (trust, done) = settle.tick(2, 1);
        assert!(!trust, "trusted again at tick {n}");
        if done && closed_at.is_none() {
            closed_at = Some(n);
        }
    }
    let n = closed_at.expect("the window must close after the grace period");
    assert!((5..=10).contains(&n), "closed after {n} ticks");
}

#[test]
fn bluetooth_is_wanted_only_at_home_and_online() {
    use endora_terminal::{bluetooth::bluetooth_wanted, vpn::HomeMatch};
    assert_eq!(
        bluetooth_wanted(HomeMatch::Home, "Internet available"),
        Some(true)
    );
    assert_eq!(bluetooth_wanted(HomeMatch::Home, "Joining"), Some(true));
    assert_eq!(
        bluetooth_wanted(HomeMatch::Away, "Internet available"),
        Some(false)
    );
    assert_eq!(
        bluetooth_wanted(HomeMatch::Away, "Portal login needed"),
        Some(false)
    );
    assert_eq!(bluetooth_wanted(HomeMatch::Home, "No Wi-Fi"), Some(false));
    assert_eq!(
        bluetooth_wanted(HomeMatch::Unknown, "No Wi-Fi"),
        Some(false)
    );
    // Home is not known yet and Wi-Fi is up: change nothing.
    assert_eq!(
        bluetooth_wanted(HomeMatch::Unknown, "Internet available"),
        None
    );
}

#[test]
fn the_radio_changes_only_after_the_answer_holds_and_never_repeats() {
    use endora_terminal::bluetooth::Policy;
    let mut policy = Policy::default();
    // A change of mind is applied only after three consecutive checks (about 30 s).
    assert_eq!(policy.observe(Some(false)), None);
    assert_eq!(policy.observe(Some(false)), None);
    assert_eq!(policy.observe(Some(false)), Some(false));
    // Already applied: no repeats.
    assert_eq!(policy.observe(Some(false)), None);
    // A one-check flicker back to "on" is ignored.
    assert_eq!(policy.observe(Some(true)), None);
    assert_eq!(policy.observe(Some(false)), None);
    // A lasting change is applied once.
    assert_eq!(policy.observe(Some(true)), None);
    assert_eq!(policy.observe(Some(true)), None);
    assert_eq!(policy.observe(Some(true)), Some(true));
    assert_eq!(policy.observe(Some(true)), None);
    // "No opinion" changes nothing and resets the count.
    assert_eq!(policy.observe(None), None);
    assert_eq!(policy.observe(Some(false)), None);
    assert_eq!(policy.observe(None), None);
    assert_eq!(policy.observe(Some(false)), None);
    assert_eq!(policy.observe(Some(false)), None);
    assert_eq!(policy.observe(Some(false)), Some(false));
}

#[test]
fn only_devices_that_appeared_during_the_window_count_as_new() {
    use endora_terminal::bluetooth::new_devices;
    let d = |mac: &str, name: &str| Device {
        mac: mac.into(),
        name: name.into(),
    };
    let before = vec![d("AA:BB:CC:DD:EE:01", "Old Phone")];
    let now = vec![
        d("AA:BB:CC:DD:EE:01", "Old Phone"),
        d("aa:bb:cc:dd:ee:02", "New Mac"),
    ];
    assert_eq!(
        new_devices(&before, &now),
        vec![d("aa:bb:cc:dd:ee:02", "New Mac")]
    );
    assert!(new_devices(&now, &now).is_empty());
    assert_eq!(new_devices(&[], &now).len(), 2);
    // Case differences in an address are not a new device.
    let upper = vec![d("AA:BB:CC:DD:EE:02", "New Mac")];
    assert!(new_devices(&now[1..], &upper).is_empty());
}

#[test]
fn a_pairing_window_that_cannot_open_escalates_through_resets_and_then_gives_up() {
    use endora_terminal::bluetooth::{Recovery, recovery_ladder};
    // Try again, then restart the service, then reload the driver (resets the chip), then stop.
    assert_eq!(recovery_ladder(0), Some(Recovery::Retry));
    assert_eq!(recovery_ladder(1), Some(Recovery::RestartService));
    assert_eq!(recovery_ladder(2), Some(Recovery::ReloadDriver));
    assert_eq!(recovery_ladder(3), None);
    assert_eq!(recovery_ladder(99), None);
    // Each step says what is happening, so the page can show it.
    for step in [
        Recovery::Retry,
        Recovery::RestartService,
        Recovery::ReloadDriver,
    ] {
        assert!(step.message().len() > 10, "{step:?}");
    }
    assert!(
        Recovery::ReloadDriver.message().contains("drop"),
        "connected devices drop on a reset"
    );
}

#[test]
fn every_adapter_is_listed_in_a_stable_order_and_named_so_you_can_tell_them_apart() {
    use endora_terminal::bluetooth::{adapter_alias, parse_controllers};
    let text = "Controller B8:27:EB:00:00:02 Speaker \n\
                Controller 5C:F3:70:00:00:01 Speaker [default]\n\
                Controller not-a-mac Broken\n\
                Controller 5C:F3:70:00:00:01 Speaker\n\
                Device AA:BB:CC:DD:EE:01 Not A Controller\n";
    assert_eq!(
        parse_controllers(text),
        vec!["5C:F3:70:00:00:01", "B8:27:EB:00:00:02"],
        "sorted, duplicates and bad lines dropped"
    );
    assert_eq!(adapter_alias(0), "Pi Speaker");
    assert_eq!(adapter_alias(1), "Pi Speaker 2");
    assert_eq!(adapter_alias(2), "Pi Speaker 3");
    assert!(alias_ok(&adapter_alias(1)));
}

#[test]
fn terminal_noise_around_bluetoothctl_answers_is_removed() {
    use endora_terminal::bluetooth::clean_output;
    let raw = "Waiting to connect to bluetoothd...\r\x1b[0;94m[bluetooth]\x1b[0m# devices Paired\r\n\
               Device AA:BB:CC:DD:EE:01 Fictional Phone\r\n\
               \x1b[0;94m[bluetooth]\x1b[0m# Device AA:BB:CC:DD:EE:02 Other Speaker\n\
               \x1b[0;92m[CHG]\x1b[0m Device AA:BB:CC:DD:EE:03 Changed\n";
    let devices = parse_devices(&clean_output(raw));
    let macs: Vec<&str> = devices.iter().map(|d| d.mac.as_str()).collect();
    assert_eq!(macs, ["AA:BB:CC:DD:EE:01", "AA:BB:CC:DD:EE:02"]);
}

#[test]
fn the_pairing_script_opens_every_adapter_under_its_own_name_with_one_agent() {
    use endora_terminal::bluetooth::pairing_steps_for;
    let adapters = vec![
        ("5C:F3:70:00:00:01".to_owned(), "Pi Speaker USB".to_owned()),
        (
            "B8:27:EB:00:00:02".to_owned(),
            "Pi Speaker Built-in".to_owned(),
        ),
    ];
    let text: String = pairing_steps_for(120, &adapters)
        .iter()
        .map(|s| s.text.as_str())
        .collect();
    assert_eq!(text.matches("agent NoInputNoOutput").count(), 1);
    assert_eq!(text.matches("default-agent").count(), 1);
    assert_eq!(text.matches("discoverable on").count(), 2);
    let first = text.rfind("select 5C:F3:70:00:00:01").unwrap();
    let second = text.rfind("select B8:27:EB:00:00:02").unwrap();
    assert!(first < second);
    assert!(text[first..second].contains("system-alias \"Pi Speaker USB\"\n"));
    assert!(text[second..].contains("system-alias \"Pi Speaker Built-in\"\n"));
    // The agent is replaced before any adapter becomes visible.
    assert!(text.find("default-agent").unwrap() < text.find("discoverable on").unwrap());
    let one = pairing_steps_for(120, &adapters[..1]);
    assert!(
        one.iter()
            .any(|s| s.text.contains("select 5C:F3:70:00:00:01"))
    );
}

#[test]
fn adapters_are_named_by_how_they_are_attached_so_the_usb_dongle_is_recognisable() {
    use endora_terminal::bluetooth::{Bus, name_adapters, parse_buses};
    let text = "hci1:\tType: Primary  Bus: USB\n\tBD Address: 5C:F3:70:00:00:01  ACL MTU: 1021:8\n\
                hci0:\tType: Primary  Bus: UART\n\tBD Address: B8:27:EB:00:00:02  ACL MTU: 1021:8\n";
    let buses = parse_buses(text);
    assert_eq!(
        buses,
        vec![
            ("5C:F3:70:00:00:01".to_owned(), Bus::Usb),
            ("B8:27:EB:00:00:02".to_owned(), Bus::Builtin)
        ]
    );
    let macs = vec![
        "5C:F3:70:00:00:01".to_owned(),
        "B8:27:EB:00:00:02".to_owned(),
    ];
    let named = name_adapters(&macs, &buses);
    assert_eq!(named[0].1, "Pi Speaker USB");
    assert_eq!(named[1].1, "Pi Speaker Built-in");
    // Two of a kind are numbered; an unknown attachment keeps the plain name.
    let two = vec![
        "AA:00:00:00:00:01".to_owned(),
        "AA:00:00:00:00:02".to_owned(),
    ];
    let both_usb = vec![(two[0].clone(), Bus::Usb), (two[1].clone(), Bus::Usb)];
    let names: Vec<String> = name_adapters(&two, &both_usb)
        .into_iter()
        .map(|a| a.1)
        .collect();
    assert_eq!(names, ["Pi Speaker USB", "Pi Speaker USB 2"]);
    assert_eq!(name_adapters(&two[..1], &[])[0].1, "Pi Speaker");
    assert!(name_adapters(&macs, &buses).iter().all(|a| alias_ok(&a.1)));
}

#[test]
fn an_adapter_that_dropped_out_is_turned_back_on_at_home_unless_it_was_turned_off_by_hand() {
    use endora_terminal::bluetooth::needs_power_on;
    // At home and online, one adapter off (for example after a power glitch): switch it on.
    assert!(needs_power_on(Some(true), false, &[true, false]));
    assert!(needs_power_on(Some(true), false, &[false]));
    // Everything is already on, or there are no adapters to turn on.
    assert!(!needs_power_on(Some(true), false, &[true, true]));
    assert!(!needs_power_on(Some(true), false, &[]));
    // Turned off by hand, away from home, offline, or no opinion yet: leave it alone.
    assert!(!needs_power_on(Some(true), true, &[false]));
    assert!(!needs_power_on(Some(false), false, &[false]));
    assert!(!needs_power_on(None, false, &[false]));
}

#[test]
fn a_connected_device_without_a_sound_device_is_found_so_the_audio_link_can_be_opened() {
    use endora_terminal::bluetooth::missing_audio_cards;
    let mac_book = Device {
        mac: "F0:2F:4B:07:8F:23".into(),
        name: "Fictional Mac".into(),
    };
    let phone = Device {
        mac: "AA:BB:CC:DD:EE:01".into(),
        name: "Fictional Phone".into(),
    };
    let cards = "5\talsa_card.usb-Fictional\talsa\n78\tbluez_card.AA_BB_CC_DD_EE_01\tmodule-bluez5-device.c\n";
    let connected = [mac_book.clone(), phone.clone()];
    assert_eq!(
        missing_audio_cards(&connected, cards),
        vec![mac_book.clone()]
    );
    assert!(missing_audio_cards(&[phone], cards).is_empty());
    assert_eq!(missing_audio_cards(&[mac_book.clone()], ""), vec![mac_book]);
    assert!(missing_audio_cards(&[], cards).is_empty());
}
