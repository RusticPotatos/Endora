use crossterm::event::KeyCode;
use endora_terminal::nav::{
    Action, MENU, Section, SettingsStatus, Target, menu_key, menu_view, rows, sections,
    settings_key, settings_view,
};

fn status() -> SettingsStatus {
    SettingsStatus {
        wifi: "Fictional Net".into(),
        bluetooth: "On".into(),
        text_size: "Normal".into(),
        screen: "Travel".into(),
        profile: "Home".into(),
        vpn: "Off".into(),
        version: "1.2.3".into(),
        start: "Home".into(),
        network: Vec::new(),
    }
}

#[test]
fn every_menu_letter_is_the_first_letter_of_what_it_opens() {
    let mut keys: Vec<char> = MENU.iter().map(|m| m.key).collect();
    for item in MENU {
        assert!(
            item.label.to_ascii_lowercase().starts_with(item.key),
            "{} should open with {}",
            item.label,
            item.key
        );
    }
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), MENU.len(), "no two menu items share a letter");
}

#[test]
fn bluetooth_wifi_settings_and_music_are_one_key_from_the_menu() {
    let target = |c: char| menu_key(c).map(|i| MENU[i].target);
    assert_eq!(target('b'), Some(Target::Page(8)), "B is Bluetooth");
    assert_eq!(target('B'), Some(Target::Page(8)), "either case");
    assert_eq!(target('w'), Some(Target::Settings(Some(Section::WiFi))));
    assert_eq!(target('s'), Some(Target::Settings(None)));
    assert_eq!(target('m'), Some(Target::Page(7)));
    assert_eq!(target('h'), Some(Target::Page(0)));
    assert_eq!(target('l'), Some(Target::Lock));
    assert_eq!(target('p'), Some(Target::Pocket));
    assert_eq!(target('x'), None);
}

#[test]
fn the_menu_lists_every_item_with_its_letter_and_a_cursor() {
    let text = menu_view(2);
    for item in MENU {
        assert!(text.contains(item.label), "{}", item.label);
    }
    let cursor_line = text.lines().find(|l| l.contains('>')).unwrap();
    assert!(cursor_line.contains(MENU[2].label), "{cursor_line}");
    assert!(text.contains("Esc"), "says how to close it");
    assert!(
        text.lines().all(|l| l.chars().count() <= 56),
        "fits the large text size"
    );
}

#[test]
fn settings_sections_use_first_letters_and_bluetooth_opens_its_page() {
    let mut keys: Vec<char> = sections().iter().map(|s| s.key).collect();
    for section in sections() {
        assert!(
            section.label.to_ascii_lowercase().starts_with(section.key),
            "{}",
            section.label
        );
    }
    keys.sort();
    keys.dedup();
    assert_eq!(keys.len(), sections().len());
    assert_eq!(
        settings_key(None, 0, KeyCode::Char('b')),
        Some(Action::Page(8))
    );
    assert_eq!(
        settings_key(None, 0, KeyCode::Char('w')),
        Some(Action::Open(Section::WiFi))
    );
}

#[test]
fn every_section_row_has_its_own_letter() {
    for section in sections() {
        let list = rows(section.section);
        let mut keys: Vec<char> = list.iter().map(|r| r.key).collect();
        keys.sort();
        keys.dedup();
        assert_eq!(keys.len(), list.len(), "{:?}", section.section);
    }
}

#[test]
fn keys_inside_settings_move_open_and_go_back() {
    use endora_terminal::nav::Action::{Back, Legacy, Open, Select, TextSize};
    // Top list: arrows move, Enter opens the highlighted section, Esc leaves Settings.
    assert_eq!(settings_key(None, 0, KeyCode::Down), Some(Select(1)));
    assert_eq!(
        settings_key(None, 0, KeyCode::Up),
        Some(Select(0)),
        "stays at the top"
    );
    let last = sections().len() - 1;
    assert_eq!(settings_key(None, last, KeyCode::Down), Some(Select(last)));
    assert_eq!(
        settings_key(None, 0, KeyCode::Enter),
        Some(Open(sections()[0].section))
    );
    assert_eq!(settings_key(None, 0, KeyCode::Esc), Some(Back));
    // Inside a section: a letter or Enter runs the row, Esc goes back to the list.
    assert_eq!(
        settings_key(Some(Section::WiFi), 0, KeyCode::Char('c')),
        Some(Legacy('w'))
    );
    assert_eq!(
        settings_key(Some(Section::WiFi), 0, KeyCode::Enter),
        Some(Legacy('w'))
    );
    assert_eq!(
        settings_key(Some(Section::WiFi), 0, KeyCode::Esc),
        Some(Back)
    );
    assert_eq!(
        settings_key(Some(Section::Display), 0, KeyCode::Char('+')),
        Some(TextSize(1))
    );
    assert_eq!(
        settings_key(Some(Section::Display), 0, KeyCode::Char('-')),
        Some(TextSize(-1))
    );
    assert_eq!(
        settings_key(Some(Section::WiFi), 0, KeyCode::Char('z')),
        None
    );
}

#[test]
fn everything_the_old_connect_page_did_is_still_reachable_in_settings() {
    let mut reachable = Vec::new();
    for section in sections() {
        for row in rows(section.section) {
            if let Action::Legacy(c) = row.action {
                reachable.push(c);
            }
        }
    }
    for old in "wnseflxbdtqvhoryzaigp".chars() {
        assert!(
            reachable.contains(&old),
            "lost the old Connect action '{old}'"
        );
    }
    let all: Vec<Action> = sections()
        .iter()
        .flat_map(|s| rows(s.section).iter().map(|r| r.action))
        .collect();
    assert!(all.contains(&Action::Restart));
    assert!(all.contains(&Action::SoundTest));
}

#[test]
fn settings_screens_show_where_you_are_and_fit_the_screen() {
    let top = settings_view(None, 1, &status());
    assert!(top.starts_with("Settings\n"), "{top}");
    assert!(top.contains("Fictional Net") && top.contains("1.2.3"));
    for section in sections() {
        assert!(top.contains(section.label));
        let inner = settings_view(Some(section.section), 0, &status());
        assert!(
            inner.starts_with(&format!("Settings > {}", section.label)),
            "{inner}"
        );
        for row in rows(section.section) {
            assert!(inner.contains(row.label), "{}", row.label);
        }
        assert!(inner.contains("Esc back"));
        assert!(inner.lines().all(|l| l.chars().count() <= 56), "{inner}");
    }
    let cursor = top.lines().find(|l| l.contains('>')).unwrap();
    assert!(cursor.contains(sections()[1].label), "{cursor}");
}

#[test]
fn the_trackpad_moves_like_the_arrow_keys_and_its_click_is_enter() {
    use endora_terminal::nav::trackpad_keys;
    assert_eq!(trackpad_keys(2, false), vec![KeyCode::Up, KeyCode::Up]);
    assert_eq!(trackpad_keys(-1, false), vec![KeyCode::Down]);
    assert_eq!(
        trackpad_keys(-9, false).len(),
        4,
        "a fast flick moves at most four rows"
    );
    assert_eq!(trackpad_keys(0, true), vec![KeyCode::Enter]);
    assert_eq!(
        trackpad_keys(1, true),
        vec![KeyCode::Up, KeyCode::Enter],
        "move first, then choose"
    );
    assert!(trackpad_keys(0, false).is_empty());
}

#[test]
fn the_start_screen_can_be_home_the_menu_or_the_last_page_used() {
    use endora_terminal::nav::{StartScreen, start_view};
    assert_eq!(StartScreen::parse("menu"), StartScreen::Menu);
    assert_eq!(StartScreen::parse("last\n"), StartScreen::Last);
    assert_eq!(
        StartScreen::parse("garbage"),
        StartScreen::Home,
        "unknown means Home"
    );
    assert_eq!(StartScreen::Home.next(), StartScreen::Menu);
    assert_eq!(StartScreen::Menu.next(), StartScreen::Last);
    assert_eq!(StartScreen::Last.next(), StartScreen::Home, "cycles");
    for choice in [StartScreen::Home, StartScreen::Menu, StartScreen::Last] {
        assert_eq!(
            StartScreen::parse(choice.as_str()),
            choice,
            "saved and read back"
        );
    }
    // (page, menu open)
    assert_eq!(start_view(StartScreen::Home, Some(7)), (0, false));
    assert_eq!(start_view(StartScreen::Menu, Some(7)), (0, true));
    assert_eq!(start_view(StartScreen::Last, Some(7)), (7, false));
    assert_eq!(
        start_view(StartScreen::Last, None),
        (0, false),
        "nothing saved yet: Home"
    );
    assert_eq!(
        start_view(StartScreen::Last, Some(42)),
        (0, false),
        "not a page: Home"
    );
}

#[test]
fn the_start_screen_is_set_in_display_settings() {
    assert_eq!(
        settings_key(Some(Section::Display), 0, KeyCode::Char('s')),
        Some(Action::StartScreen)
    );
    let mut text_status = status();
    text_status.start = "Menu".into();
    let display = settings_view(Some(Section::Display), 0, &text_status);
    assert!(display.contains("Start screen"), "{display}");
    assert!(
        display.contains("Menu"),
        "shows the current choice: {display}"
    );
}

#[test]
fn the_network_section_shows_details_and_offers_vpn_controls() {
    assert_eq!(
        settings_key(Some(Section::Network), 0, KeyCode::Char('c')),
        Some(Action::Vpn(true))
    );
    assert_eq!(
        settings_key(Some(Section::Network), 0, KeyCode::Char('d')),
        Some(Action::Vpn(false))
    );
    assert_eq!(
        settings_key(Some(Section::Network), 0, KeyCode::Char('t')),
        Some(Action::VpnTest)
    );
    assert_eq!(
        settings_key(Some(Section::Network), 0, KeyCode::Char('r')),
        Some(Action::NetRefresh)
    );
    let mut s = status();
    s.network = vec!["IP      fictional".into(), "MAC     fictional-mac".into()];
    let view = settings_view(Some(Section::Network), 0, &s);
    assert!(view.contains("fictional-mac"), "{view}");
    assert!(view.lines().all(|l| l.chars().count() <= 56), "{view}");
}
