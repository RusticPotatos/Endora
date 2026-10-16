use endora_terminal::network::{NetDetails, net_lines, parse_device_show};
use endora_terminal::vpn::{Facts, HomeMatch, Vpn, decide, parse_trace};
use std::net::Ipv4Addr;

// Fictional, documentation-range addresses, built at run time.
fn addr(last: u8) -> String {
    Ipv4Addr::new(198, 51, 100, last).to_string()
}

#[test]
fn the_wifi_devices_addresses_are_read_from_nmcli() {
    let text = format!(
        "GENERAL.HWADDR:02:00:00:AA:BB:CC\nGENERAL.CONNECTION:Fictional Net\nIP4.ADDRESS[1]:{}/24\nIP4.GATEWAY:{}\nIP4.DNS[1]:{}\nIP4.DNS[2]:{}\nIP6.ADDRESS[1]:fe80::1/64\n",
        addr(7),
        addr(1),
        addr(2),
        addr(3)
    );
    let d = parse_device_show(&text);
    assert_eq!(d.mac, "02:00:00:AA:BB:CC");
    assert_eq!(d.network, "Fictional Net");
    assert_eq!(d.ip, format!("{}/24", addr(7)));
    assert_eq!(d.gateway, addr(1));
    assert_eq!(d.dns, vec![addr(2), addr(3)]);
    assert_eq!(parse_device_show(""), NetDetails::default());
}

#[test]
fn the_network_screen_lists_each_detail_on_its_own_line() {
    let d = NetDetails {
        network: "Fictional Net".into(),
        signal: Some(50),
        ip: format!("{}/24", addr(7)),
        mac: "02:00:00:AA:BB:CC".into(),
        gateway: addr(1),
        dns: vec![addr(2), addr(3)],
    };
    let lines = net_lines(&d, "Off", Some(12));
    let text = lines.join("\n");
    for wanted in [
        "Fictional Net",
        "50%",
        &format!("{}/24", addr(7)),
        "02:00:00:AA:BB:CC",
        &addr(1),
        &addr(3),
        "VPN",
        "Off",
        "12 min",
    ] {
        assert!(text.contains(wanted), "{wanted}: {text}");
    }
    assert!(lines.iter().all(|l| l.chars().count() <= 56), "{text}");
    let nothing = net_lines(&NetDetails::default(), "Off", None).join("\n");
    assert!(nothing.contains("Not connected"), "{nothing}");
    assert!(
        !nothing.contains("min"),
        "no hold shown when none: {nothing}"
    );
}

#[test]
fn cloudflares_trace_says_whether_traffic_goes_through_warp() {
    let text = format!("fl=x\nip={}\nts=1\ncolo=ATL\nwarp=on\n", addr(9));
    let t = parse_trace(&text);
    assert_eq!(t.warp.as_deref(), Some("on"));
    assert_eq!(t.ip, Some(addr(9)));
    assert_eq!(parse_trace("garbage").warp, None);
}

#[test]
fn a_manual_vpn_choice_is_not_undone_by_the_automatic_one() {
    let facts = |vpn, home, manual_hold| Facts {
        vpn,
        home,
        internet: true,
        node_reachable: false,
        unreachable_ticks: 5,
        manual_hold,
    };
    use endora_terminal::vpn::Action;
    // Normally: at home the tunnel is dropped; away it is connected.
    assert_eq!(
        decide(&facts(Vpn::Connected, HomeMatch::Home, false)),
        Action::Disconnect
    );
    assert_eq!(
        decide(&facts(Vpn::Disconnected, HomeMatch::Away, false)),
        Action::Connect
    );
    // Connected or disconnected by hand: left alone while the hold lasts.
    assert_eq!(
        decide(&facts(Vpn::Connected, HomeMatch::Home, true)),
        Action::Nothing
    );
    assert_eq!(
        decide(&facts(Vpn::Disconnected, HomeMatch::Away, true)),
        Action::Nothing
    );
}
