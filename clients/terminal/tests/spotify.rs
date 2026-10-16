use endora_terminal::spotify::{
    Item, NowPlaying, Tokens, auth_url, new_verifier, parse_callback, parse_items, parse_now,
    pkce_challenge, time_text,
};
use serde_json::json;

#[test]
fn pkce_challenge_matches_the_published_rfc_7636_vector() {
    assert_eq!(
        pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        "E9Melhoa2OwvFrEMTJguCHaoeK1t8URWbuGJSstw-cM"
    );
    let a = new_verifier();
    assert!((43..=128).contains(&a.len()));
    assert!(a.bytes().all(|b| b.is_ascii_alphanumeric()));
    assert_ne!(a, new_verifier(), "verifiers are random");
}

#[test]
fn the_login_url_asks_for_playback_control_with_pkce_and_no_secret() {
    let url = auth_url("fictionalclientid", "CHALLENGE", "state123");
    assert!(url.starts_with("https://accounts.spotify.com/authorize?"));
    for part in [
        "client_id=fictionalclientid",
        "response_type=code",
        "code_challenge_method=S256",
        "code_challenge=CHALLENGE",
        "state=state123",
        "redirect_uri=http%3A%2F%2F127.0.0.1%3A8888%2Fcallback",
        "user-modify-playback-state",
        "user-read-playback-state",
    ] {
        assert!(url.contains(part), "{part}: {url}");
    }
    assert!(!url.contains("client_secret"));
}

#[test]
fn the_pasted_redirect_address_yields_the_code_only_when_the_state_matches() {
    let ok = "http://127.0.0.1:8888/callback?code=abc123&state=state123";
    assert_eq!(parse_callback(ok, "state123").unwrap(), "abc123");
    // Quotes, spaces and a missing scheme from a hasty paste are tolerated.
    assert_eq!(
        parse_callback(
            " '127.0.0.1:8888/callback?code=abc123&state=state123' ",
            "state123"
        )
        .unwrap(),
        "abc123"
    );
    assert!(
        parse_callback(ok, "other").is_err(),
        "wrong state is refused"
    );
    assert!(
        parse_callback(
            "http://127.0.0.1:8888/callback?error=access_denied&state=state123",
            "state123"
        )
        .is_err()
    );
    assert!(parse_callback("http://127.0.0.1:8888/callback?state=state123", "state123").is_err());
    assert!(parse_callback("", "state123").is_err());
}

#[test]
fn tokens_refresh_a_minute_before_they_expire() {
    let t = Tokens {
        access: "a".into(),
        refresh: "r".into(),
        expires_at: 1000,
    };
    assert!(!t.needs_refresh(900));
    assert!(t.needs_refresh(941));
    assert!(t.needs_refresh(5000));
}

#[test]
fn playback_state_becomes_a_short_now_playing_record() {
    let body = json!({
        "is_playing": true, "progress_ms": 42000,
        "device": {"name": "Speaker", "volume_percent": 60, "id": "dev1"},
        "item": {"name": "Fictional Song", "duration_ms": 195000, "uri": "spotify:track:s1",
                 "artists": [{"name": "Artist A", "uri": "spotify:artist:a1"}, {"name": "Artist B"}]}
    });
    assert_eq!(
        parse_now(&body),
        Some(NowPlaying {
            title: "Fictional Song".into(),
            artist: "Artist A, Artist B".into(),
            playing: true,
            device: "Speaker".into(),
            volume: Some(60),
            progress_ms: 42000,
            duration_ms: 195000,
            uri: "spotify:track:s1".into(),
            artist_uri: "spotify:artist:a1".into(),
        })
    );
    assert_eq!(parse_now(&json!({})), None);
    assert_eq!(time_text(42000), "0:42");
    assert_eq!(time_text(195000), "3:15");
    assert_eq!(time_text(3_725_000), "62:05");
}

#[test]
fn search_and_library_results_become_playable_items() {
    let search = json!({
        "tracks": {"items": [{"uri":"spotify:track:t1","name":"Song One","artists":[{"name":"Artist"}]}]},
        "playlists": {"items": [null, {"uri":"spotify:playlist:p1","name":"Mix","owner":{"display_name":"Someone"}}]},
        "albums": {"items": [{"uri":"spotify:album:a1","name":"Album","artists":[{"name":"Band"}]}]}
    });
    let items = parse_items(&search);
    assert_eq!(
        items,
        vec![
            Item {
                uri: "spotify:track:t1".into(),
                label: "Song One - Artist".into(),
                track: true
            },
            Item {
                uri: "spotify:playlist:p1".into(),
                label: "Playlist: Mix".into(),
                track: false
            },
            Item {
                uri: "spotify:album:a1".into(),
                label: "Album: Album - Band".into(),
                track: false
            },
        ]
    );
    // Saved tracks come wrapped in {"track": ...}; a bad uri is dropped.
    let saved = json!({"items": [{"track": {"uri":"spotify:track:t2","name":"Liked","artists":[{"name":"Z"}]}},
                                  {"track": {"uri":"https://evil.example","name":"Bad","artists":[]}}]});
    assert_eq!(parse_items(&saved).len(), 1);
    assert!(parse_items(&json!({"error": "x"})).is_empty());
}

#[test]
fn the_login_link_fits_on_the_small_screen_as_a_qr_code() {
    use endora_terminal::spotify::qr_lines;
    // Built at run time so no key-shaped text sits in the repository.
    let client = "ab12".repeat(8);
    let state = "cd34".repeat(8);
    let url = auth_url(
        &client,
        &pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        &state,
    );
    let lines = qr_lines(&url).unwrap();
    let width = lines.iter().map(|l| l.chars().count()).max().unwrap();
    assert!(width <= 88, "too wide for a 90-column screen: {width}");
    assert!(lines.len() <= 44, "too tall: {}", lines.len());
    assert!(
        lines
            .iter()
            .all(|l| l.chars().all(|c| matches!(c, ' ' | '█' | '▀' | '▄'))),
        "only block characters"
    );
    assert!(lines.iter().any(|l| l.contains('█')));
}

#[test]
fn form_text_is_decoded_and_the_pasted_address_is_found() {
    use endora_terminal::spotify::{form_value, percent_decode};
    assert_eq!(percent_decode("a%3Db%26c+d%2F"), "a=b&c d/");
    assert_eq!(percent_decode("100%"), "100%", "a stray percent is kept");
    assert_eq!(percent_decode("%zz"), "%zz");
    let body =
        "note=x&url=http%3A%2F%2F127.0.0.1%3A8888%2Fcallback%3Fcode%3Dabc%26state%3Ds1&other=1";
    assert_eq!(
        form_value(body, "url").unwrap(),
        "http://127.0.0.1:8888/callback?code=abc&state=s1"
    );
    assert_eq!(form_value(body, "missing"), None);
}

#[test]
fn the_paste_page_accepts_one_answer_only_on_its_secret_path() {
    use endora_terminal::spotify::wait_for_paste;
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let request = move |method: &str, path: &str, body: &str| -> String {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "{method} {path} HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    };
    let client = std::thread::spawn(move || {
        let form = request("GET", "/secret123", "");
        assert!(
            form.starts_with("HTTP/1.1 200") && form.contains("<textarea"),
            "{form}"
        );
        assert!(request("GET", "/wrong", "").starts_with("HTTP/1.1 404"));
        assert!(request("POST", "/wrong", "url=evil").starts_with("HTTP/1.1 404"));
        let ok = request(
            "POST",
            "/secret123",
            "url=http%3A%2F%2F127.0.0.1%3A8888%2Fcallback%3Fcode%3Dabc",
        );
        assert!(
            ok.starts_with("HTTP/1.1 200") && ok.contains("Done"),
            "{ok}"
        );
    });
    let pasted =
        wait_for_paste(&listener, "secret123", std::time::Duration::from_secs(10)).unwrap();
    assert_eq!(pasted, "http://127.0.0.1:8888/callback?code=abc");
    client.join().unwrap();
    // Nobody answers: it gives up instead of waiting forever.
    let quiet = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    assert!(wait_for_paste(&quiet, "secret123", std::time::Duration::from_millis(300)).is_err());
}

#[test]
fn each_sign_in_qr_code_gets_its_own_cleared_screen_that_fits_the_console() {
    use endora_terminal::spotify::{qr_lines, qr_page};
    let client = "ab12".repeat(8);
    let login = auth_url(
        &client,
        &pkce_challenge("dBjftJeZ4CVP-mB92K27uhbUJU1p1r_wW1gFWFOEjXk"),
        &"cd34".repeat(8),
    );
    let paste = format!(
        "http://{}:8899/{}",
        std::net::Ipv4Addr::new(10, 1, 2, 3),
        "t".repeat(32)
    );
    for (url, heading) in [
        (login.as_str(), "Step 1 of 2"),
        (paste.as_str(), "Step 2 of 2"),
    ] {
        let page = qr_page(heading, &qr_lines(url).unwrap(), "Press Enter when done.");
        assert!(
            page.starts_with("\x1b[2J\x1b[H"),
            "clears the screen so nothing scrolls away"
        );
        assert!(page.contains(heading) && page.contains("Press Enter when done."));
        let rows = page.trim_start_matches("\x1b[2J\x1b[H").lines().count();
        assert!(
            rows <= 44,
            "{heading}: {rows} rows leave no room on a 45-row screen"
        );
    }
}

#[test]
fn waiting_for_the_paste_can_be_cancelled_without_waiting_out_the_clock() {
    use endora_terminal::spotify::wait_for_paste_with;
    let quiet = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let started = std::time::Instant::now();
    let mut checks = 0;
    let result = wait_for_paste_with(
        &quiet,
        "secret123",
        std::time::Duration::from_secs(60),
        || {
            checks += 1;
            checks >= 3
        },
    );
    assert!(result.is_err(), "cancelled is not a paste");
    assert!(
        started.elapsed() < std::time::Duration::from_secs(5),
        "returns promptly once cancelled"
    );
    assert_eq!(checks, 3, "asked again on each pass until it said stop");
}

#[test]
fn artists_and_saved_albums_become_items_and_artists_open_instead_of_playing() {
    let top = json!({"items": [
        {"uri":"spotify:artist:a1","name":"Band One","type":"artist"},
        {"uri":"https://evil.example","name":"Bad"}
    ]});
    let items = parse_items(&top);
    assert_eq!(
        items,
        vec![Item {
            uri: "spotify:artist:a1".into(),
            label: "Artist: Band One".into(),
            track: false
        }]
    );
    assert!(items[0].is_artist());
    // Saved albums arrive wrapped in {"album": ...}; search can return artists too.
    let saved = json!({"items": [{"album": {"uri":"spotify:album:b1","name":"Record","artists":[{"name":"Band One"}]}}]});
    let albums = parse_items(&saved);
    assert_eq!(albums[0].label, "Album: Record - Band One");
    assert!(!albums[0].is_artist());
    let search = json!({"artists": {"items": [{"uri":"spotify:artist:a2","name":"Solo"}]}});
    assert_eq!(parse_items(&search)[0].label, "Artist: Solo");
}

#[test]
fn browsing_your_top_artists_and_saved_albums_needs_the_top_read_permission() {
    let url = auth_url("fictionalclientid", "CHALLENGE", "state123");
    assert!(url.contains("user-top-read"), "{url}");
    assert!(url.contains("user-library-read"));
}

#[test]
fn discovery_offers_unsaved_albums_from_artists_you_play_without_repeats() {
    use endora_terminal::spotify::mix_discovery;
    let album = |id: &str, name: &str| Item {
        uri: format!("spotify:album:{id}"),
        label: format!("Album: {name} - X"),
        track: false,
    };
    let artist_one = vec![
        album("a1", "One"),
        album("a2", "Two"),
        album("a3", "Three"),
        album("a4", "Four"),
    ];
    let artist_two = vec![album("b1", "Uno"), album("a1", "One"), album("b2", "Dos")];
    let saved = vec![album("a2", "Two")];
    let mix = mix_discovery(&[artist_one.clone(), artist_two.clone()], &saved, 7);
    let uris: Vec<&str> = mix.iter().map(|i| i.uri.as_str()).collect();
    assert!(
        !uris.contains(&"spotify:album:a2"),
        "already saved albums are left out"
    );
    let mut unique = uris.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), uris.len(), "no repeats");
    assert!(mix.len() <= 20);
    assert!(
        uris.iter()
            .filter(|u| u.starts_with("spotify:album:a"))
            .count()
            <= 3,
        "at most three albums from one artist so the mix is varied"
    );
    // The same seed gives the same mix, another seed may order it differently.
    assert_eq!(
        mix,
        mix_discovery(&[artist_one.clone(), artist_two.clone()], &saved, 7)
    );
    assert!(mix_discovery(&[], &saved, 1).is_empty());
}

#[test]
fn an_artists_albums_are_asked_for_within_spotifys_limit_for_personal_apps() {
    use endora_terminal::spotify::ARTIST_ALBUMS_LIMIT;
    // Since 2026 development-mode apps get "Invalid limit" above 10 (checked against the API).
    assert!((1..=10).contains(&ARTIST_ALBUMS_LIMIT));
}

#[test]
fn discovery_reports_spotifys_error_when_no_artist_could_be_looked_up() {
    use endora_terminal::spotify::discovery_from;
    let album = |id: &str| Item {
        uri: format!("spotify:album:{id}"),
        label: format!("Album: {id} - X"),
        track: false,
    };
    let failed: Vec<endora_terminal::Result<Vec<Item>>> = vec![
        Err("Spotify could not do that".into()),
        Err("Spotify could not do that".into()),
    ];
    assert_eq!(
        discovery_from(failed, &[], 1).unwrap_err().to_string(),
        "Spotify could not do that",
        "not a silent 'Nothing found'"
    );
    let some_failed: Vec<endora_terminal::Result<Vec<Item>>> = vec![
        Err("Spotify is busy".into()),
        Ok(vec![album("a1"), album("a2")]),
    ];
    assert_eq!(
        discovery_from(some_failed, &[], 1).unwrap().len(),
        2,
        "one failure is fine"
    );
    let none_saved: Vec<endora_terminal::Result<Vec<Item>>> = vec![Ok(vec![album("a1")])];
    assert!(
        discovery_from(none_saved, &[album("a1")], 1)
            .unwrap()
            .is_empty()
    );
}

#[test]
fn saving_songs_needs_the_library_modify_permission() {
    let url = auth_url("fictionalclientid", "CHALLENGE", "state123");
    assert!(url.contains("user-library-modify"), "{url}");
}

#[test]
fn a_radio_batch_takes_a_couple_of_new_songs_from_each_album() {
    use endora_terminal::spotify::pick_tracks;
    let track = |id: &str| Item {
        uri: format!("spotify:track:{id}"),
        label: format!("{id} - X"),
        track: true,
    };
    let albums = vec![
        vec![track("a1"), track("a2"), track("a3"), track("a4")],
        vec![track("b1"), track("b2")],
        vec![track("c1"), track("c2"), track("c3")],
    ];
    let played = vec!["spotify:track:b1".to_owned()];
    let batch = pick_tracks(&albums, &played, 2, 7);
    let uris: Vec<&str> = batch.iter().map(|t| t.uri.as_str()).collect();
    assert!(
        !uris.contains(&"spotify:track:b1"),
        "already played songs are left out"
    );
    assert!(
        uris.iter().filter(|u| u.contains(":a")).count() <= 2,
        "two per album"
    );
    assert!(
        uris.iter().filter(|u| u.contains(":c")).count() <= 2,
        "two per album"
    );
    let mut unique = uris.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), uris.len(), "no song twice");
    assert!(batch.iter().all(|t| t.track), "only songs");
    assert_eq!(
        batch,
        pick_tracks(&albums, &played, 2, 7),
        "same seed, same batch"
    );
    let many: Vec<Vec<Item>> = (0..30).map(|i| vec![track(&format!("x{i}"))]).collect();
    assert!(
        pick_tracks(&many, &[], 2, 1).len() <= 20,
        "a batch is at most 20 songs"
    );
}

#[test]
fn the_radio_keeps_going_until_something_else_is_played() {
    use endora_terminal::spotify::Radio;
    let uris = |ids: &[&str]| {
        ids.iter()
            .map(|i| format!("spotify:track:{i}"))
            .collect::<Vec<_>>()
    };
    let mut radio = Radio::new(uris(&["t1", "t2", "t3"]));
    assert!(radio.is_ours("spotify:track:t1"));
    assert!(!radio.needs_more("spotify:track:t1"));
    radio.playing("spotify:track:t1");
    assert!(
        radio.needs_more("spotify:track:t3"),
        "on the last song, fetch the next batch"
    );
    radio.extend(uris(&["t4", "t5"]));
    assert!(!radio.needs_more("spotify:track:t3"), "already topped up");
    assert!(radio.needs_more("spotify:track:t5"));
    assert!(
        !radio.is_ours("spotify:track:other"),
        "something else is playing: the radio stops"
    );
    assert_eq!(radio.played(), ["spotify:track:t1".to_owned()]);
    radio.avoid("spotify:artist:a1");
    radio.avoid("spotify:artist:a1");
    assert_eq!(
        radio.avoided(),
        ["spotify:artist:a1".to_owned()],
        "each artist once"
    );
}

#[test]
fn the_speakers_sign_in_link_is_read_from_the_players_output() {
    use endora_terminal::spotify::speaker_link;
    let line = "Browse to: https://accounts.spotify.com/authorize?response_type=code&client_id=abc&state=s&redirect_uri=http%3A%2F%2F127.0.0.1%3A5599%2Flogin";
    assert_eq!(
        speaker_link(line).as_deref(),
        Some(
            "https://accounts.spotify.com/authorize?response_type=code&client_id=abc&state=s&redirect_uri=http%3A%2F%2F127.0.0.1%3A5599%2Flogin"
        )
    );
    assert_eq!(speaker_link("[INFO librespot] starting"), None);
    assert_eq!(
        speaker_link("Browse to: https://evil.example/authorize?x=1"),
        None,
        "only a Spotify sign-in page is ever shown as a QR code"
    );
}

#[test]
fn the_pasted_speaker_address_is_only_ever_sent_to_the_local_player() {
    use endora_terminal::spotify::speaker_login_path;
    let ok = "http://127.0.0.1:5599/login?code=abc123&state=s1";
    assert_eq!(
        speaker_login_path(ok, 5599).unwrap(),
        "/login?code=abc123&state=s1"
    );
    assert_eq!(
        speaker_login_path(" '127.0.0.1:5599/login?code=abc123&state=s1' ", 5599).unwrap(),
        "/login?code=abc123&state=s1",
        "a hasty paste is tolerated"
    );
    assert!(speaker_login_path("http://127.0.0.1:5599/login?error=access_denied", 5599).is_err());
    assert!(
        speaker_login_path("http://127.0.0.1:8888/callback?code=abc", 5599).is_err(),
        "the other sign-in's address"
    );
    assert!(speaker_login_path("http://evil.example:5599/login?code=abc", 5599).is_err());
    assert!(
        speaker_login_path("http://127.0.0.1:5599/login?code=a b", 5599).is_err(),
        "no spaces or request smuggling"
    );
    assert!(speaker_login_path("", 5599).is_err());
}

#[test]
fn sign_in_steps_are_numbered_for_the_steps_actually_shown() {
    use endora_terminal::spotify::step_heading;
    assert_eq!(step_heading(1, 2, "scan this"), "Step 1 of 2: scan this");
    assert_eq!(step_heading(3, 4, "scan this"), "Step 3 of 4: scan this");
}

#[test]
fn a_missing_speaker_says_how_to_sign_it_in_on_this_device() {
    use endora_terminal::spotify::SPEAKER_MISSING;
    assert!(
        SPEAKER_MISSING.contains("Settings > Accounts"),
        "{SPEAKER_MISSING}"
    );
    assert!(
        !SPEAKER_MISSING.to_lowercase().contains("phone"),
        "no other device needed"
    );
}

#[test]
fn the_sign_in_page_offers_the_long_spotify_link_as_a_button_above_the_paste_box() {
    use endora_terminal::spotify::paste_page;
    let link = "https://accounts.spotify.com/authorize?a=1&b=2";
    let page = paste_page(Some(link));
    assert!(
        page.contains("href=\"https://accounts.spotify.com/authorize?a=1&amp;b=2\""),
        "the link is escaped for HTML: {page}"
    );
    assert!(
        page.contains("target=_blank"),
        "opens in a new tab so the paste box stays: {page}"
    );
    assert!(
        page.find("href=").unwrap() < page.find("<textarea").unwrap(),
        "button first, then paste"
    );
    let plain = paste_page(None);
    assert!(
        plain.contains("<textarea") && !plain.contains("href="),
        "{plain}"
    );
    // A link that is not a Spotify sign-in page is never offered.
    assert!(!paste_page(Some("https://evil.example/x")).contains("href="));
    assert!(
        !paste_page(Some("https://accounts.spotify.com/authorize?\"><script>"))
            .contains("<script>")
    );
}

#[test]
fn the_sign_in_page_is_served_with_its_button_on_the_secret_path() {
    use endora_terminal::spotify::wait_for_sign_in;
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    let request = move |method: &str, body: &str| -> String {
        let mut s = std::net::TcpStream::connect(("127.0.0.1", port)).unwrap();
        write!(s, "{method} /tok HTTP/1.1\r\nHost: x\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        let mut out = String::new();
        s.read_to_string(&mut out).unwrap();
        out
    };
    let client = std::thread::spawn(move || {
        let page = request("GET", "");
        assert!(
            page.contains("href=\"https://accounts.spotify.com/authorize?x=1\""),
            "{page}"
        );
        request(
            "POST",
            "url=http%3A%2F%2F127.0.0.1%3A5599%2Flogin%3Fcode%3Dz",
        );
    });
    let pasted = wait_for_sign_in(
        &listener,
        "tok",
        "https://accounts.spotify.com/authorize?x=1",
        std::time::Duration::from_secs(10),
        || false,
    )
    .unwrap();
    assert_eq!(pasted, "http://127.0.0.1:5599/login?code=z");
    client.join().unwrap();
}

#[test]
fn playlist_songs_wrapped_as_item_are_read() {
    // Since 2026 a playlist's entries wrap the song as {"item": ...} instead of {"track": ...}.
    let page = json!({"items": [
        {"added_at": "x", "item": {"uri":"spotify:track:p1","name":"In A Playlist","artists":[{"name":"Band"}]}},
        {"item": null}
    ]});
    let items = parse_items(&page);
    assert_eq!(items.len(), 1);
    assert_eq!(items[0].uri, "spotify:track:p1");
    assert_eq!(items[0].label, "In A Playlist - Band");
}

#[test]
fn smart_shuffle_mixes_one_new_song_after_every_few_of_yours() {
    use endora_terminal::spotify::smart_mix;
    let track = |id: &str| Item {
        uri: format!("spotify:track:{id}"),
        label: id.into(),
        track: true,
    };
    let mine: Vec<Item> = (0..9).map(|i| track(&format!("m{i}"))).collect();
    let fresh: Vec<Item> = (0..5).map(|i| track(&format!("n{i}"))).collect();
    let mix = smart_mix(&mine, &fresh, 3, 11);
    let labels: Vec<&str> = mix.iter().map(|t| t.label.as_str()).collect();
    assert_eq!(
        mix.len(),
        12,
        "nine of yours and three new ones: {labels:?}"
    );
    for (i, label) in labels.iter().enumerate() {
        let is_new = label.starts_with('n');
        assert_eq!(
            is_new,
            (i + 1) % 4 == 0,
            "a new song after every three: {labels:?}"
        );
    }
    let mut unique = labels.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), labels.len(), "no song twice");
    assert_ne!(
        labels
            .iter()
            .filter(|l| l.starts_with('m'))
            .copied()
            .collect::<Vec<_>>(),
        (0..9)
            .map(|i| format!("m{i}"))
            .collect::<Vec<_>>()
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        "your songs are shuffled"
    );
    assert_eq!(
        mix,
        smart_mix(&mine, &fresh, 3, 11),
        "same seed, same order"
    );
    // With nothing new to mix in, it is a plain shuffle; a long list is capped.
    assert_eq!(smart_mix(&mine, &[], 3, 1).len(), 9);
    let many: Vec<Item> = (0..200).map(|i| track(&format!("m{i}"))).collect();
    assert!(smart_mix(&many, &fresh, 3, 1).len() <= 50);
}

#[test]
fn a_big_library_is_sampled_from_a_few_random_pages() {
    use endora_terminal::spotify::sample_offsets;
    let offsets = sample_offsets(1154, 50, 4, 9);
    assert_eq!(offsets.len(), 4);
    assert!(
        offsets.iter().all(|o| o % 50 == 0 && *o < 1154),
        "{offsets:?}"
    );
    let mut unique = offsets.clone();
    unique.sort();
    unique.dedup();
    assert_eq!(unique.len(), 4, "different pages: {offsets:?}");
    assert_eq!(
        sample_offsets(80, 50, 4, 9).len(),
        2,
        "only as many pages as exist"
    );
    assert_eq!(sample_offsets(0, 50, 4, 9), Vec::<usize>::new());
    assert_eq!(offsets, sample_offsets(1154, 50, 4, 9));
}

#[test]
fn an_ok_reply_with_no_body_is_success_not_an_unexpected_reply() {
    use endora_terminal::spotify::reply_value;
    assert_eq!(
        reply_value("").unwrap(),
        serde_json::Value::Null,
        "saving a song answers 200 with nothing"
    );
    assert_eq!(reply_value("  \n").unwrap(), serde_json::Value::Null);
    assert_eq!(reply_value("{\"a\":1}").unwrap()["a"], 1);
    assert!(reply_value("<html>oops").is_err());
}

#[test]
fn a_command_that_succeeds_is_success_whatever_its_reply_says() {
    use endora_terminal::spotify::command_reply;
    // Next track answers 200 with an opaque token, not JSON.
    assert_eq!(
        command_reply("mLM68sEzI8-fictional"),
        serde_json::Value::Null
    );
    assert_eq!(command_reply(""), serde_json::Value::Null);
    assert_eq!(command_reply("{\"snapshot_id\":\"x\"}")["snapshot_id"], "x");
}

#[test]
fn the_accounts_devices_become_a_play_on_list() {
    use endora_terminal::spotify::{device_items, parse_devices};
    let value = json!({"devices": [
        {"id": "abc123", "name": "Speaker", "type": "Speaker", "is_active": false},
        {"id": "def456", "name": "Fictional MacBook", "type": "Computer", "is_active": true},
        {"id": "bad id/../x", "name": "Broken", "type": "Smartphone", "is_active": false},
        {"name": "No id", "type": "TV"}
    ]});
    let devices = parse_devices(&value);
    assert_eq!(devices.len(), 2, "devices without a usable id are left out");
    let items = device_items(&devices);
    assert_eq!(items[0].label, "Speaker (this handheld)");
    assert_eq!(items[1].label, "Fictional MacBook (Computer) - playing now");
    assert_eq!(items[1].device_id(), Some("def456"));
    assert_eq!(items[0].device_id(), Some("abc123"));
    assert!(!items[0].track && !items[0].is_artist());
    let song = Item {
        uri: "spotify:track:t1".into(),
        label: "x".into(),
        track: true,
    };
    assert_eq!(song.device_id(), None, "a song is not a device");
}

#[test]
fn playback_goes_to_the_chosen_device_or_else_this_handheld() {
    use endora_terminal::spotify::{choose_device, parse_devices};
    let devices = parse_devices(&json!({"devices": [
        {"id": "abc123", "name": "Speaker", "type": "Speaker"},
        {"id": "def456", "name": "Fictional MacBook", "type": "Computer"}
    ]}));
    assert_eq!(
        choose_device(&devices, Some("Fictional MacBook"))
            .unwrap()
            .id,
        "def456"
    );
    assert_eq!(
        choose_device(&devices, None).unwrap().id,
        "abc123",
        "default: this handheld"
    );
    assert_eq!(
        choose_device(&devices, Some("Gone Away")).unwrap().id,
        "abc123",
        "the chosen one is off: back to this handheld"
    );
    assert!(choose_device(&[], None).is_none());
}
