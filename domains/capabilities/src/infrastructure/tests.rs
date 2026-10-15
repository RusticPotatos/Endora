use super::*;
#[test]
fn search_wire_sends_key_in_header_and_reports_provider_failure() {
    use std::io::{Read, Write};
    let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
    let address = listener.local_addr().unwrap();
    let server = std::thread::spawn(move || {
        for status in [200, 429] {
            let (mut stream, _) = listener.accept().unwrap();
            stream
                .set_read_timeout(Some(Duration::from_secs(3)))
                .unwrap();
            let mut request = Vec::new();
            loop {
                let mut bytes = [0; 1024];
                let read = stream.read(&mut bytes).unwrap();
                assert!(read > 0);
                request.extend_from_slice(&bytes[..read]);
                if request.windows(4).any(|w| w == b"\r\n\r\n") {
                    break;
                }
            }
            let request = String::from_utf8(request).unwrap().to_ascii_lowercase();
            assert!(request.starts_with("get /search?q=fictional%20schedule&count=2 "));
            assert!(request.contains("x-subscription-token: fixture-key\r\n"));
            assert!(request.contains("api-version: 2023-01-01\r\n"));
            assert!(!request.lines().next().unwrap().contains("fixture-key"));
            let body = r#"{"web":{"results":[]}}"#;
            write!(stream, "HTTP/1.1 {status} Fixture\r\nContent-Length: {}\r\nContent-Type: application/json\r\nConnection: close\r\n\r\n{body}", body.len()).unwrap();
        }
    });
    let url = format!("http://{address}/search?q=fictional%20schedule&count=2");
    assert!(search_request(&url, "fixture-key").is_ok());
    let error = search_request(&url, "fixture-key").unwrap_err().to_string();
    assert!(error.contains("429") && !error.contains("fixture-key"));
    server.join().unwrap();
}
#[test]
fn internet_search_returns_sources_and_requires_a_local_key() {
    assert!(
        WebSearchCapability
            .invoke(
                &json!({"query":"fictional schedule"}),
                &CapabilitySettings::new()
            )
            .is_err()
    );
    let raw = json!({"web":{"results":[{"title":"Calendar","url":"https://example.com/schedule","description":"Upcoming games"},{"title":"Unsafe","url":"javascript:alert(1)"}]}});
    let results = search_results(&raw, 5).unwrap();
    assert_eq!(results["results"].as_array().unwrap().len(), 1);
    let text = WebSearchCapability.summarize(&results);
    assert!(text.contains("https://example.com/schedule") && text.contains("Upcoming games"));
    assert!(search_results(&json!({"error":{"status":429}}), 5).is_err());
}

#[test]
fn schedule_reads_exclude_started_games_and_sort_before_selecting() {
    let event = |id: &str, date: &str| json!({"id":id,"date":date,"competitions":[{"competitors":[{"id":"fixture-team","homeAway":"home"},{"id":"other","team":{"displayName":"Bears"}}],"venue":{"fullName":"Example Stadium"}}]});
    let events = vec![
        event("later", "2030-09-08T17:00:00Z"),
        event("started", "2030-09-07T12:00:00Z"),
        event("next", "2030-09-07T17:00:00Z"),
        event("invalid", "2030-02-30T17:00:00Z"),
    ];
    let cutoff = crate::application::schedule::utc_millis("2030-09-07T14:00:00Z").unwrap();
    let games = next_schedule_games(&events, "fixture-team", cutoff, 2);
    assert_eq!(games.len(), 2);
    assert_eq!(games[0]["id"], "next");
    assert_eq!(games[1]["id"], "later");
}

/// A `CapabilityInfo` with a given band and reach; other fields are inert.
fn info(reversibility: Reversibility, reaches_external: bool) -> CapabilityInfo {
    CapabilityInfo {
        id: "x",
        name: "X",
        description: "",
        category: "",
        reaches_external,
        reversibility,
        configured: true,
        needs: "",
        settings: &[],
        wants_place: false,
        borrows_from: None,
        third_party: false,
        input_schema: None,
    }
}

#[test]
fn a_place_the_person_did_not_name_is_filled_in() {
    let filled = super::place_filled_in("{}", true, "Springfield", "the weather please");
    assert!(filled.contains("Springfield"), "{filled}");
    assert!(filled.contains("location"), "{filled}");
}

#[test]
fn a_place_the_person_did_name_is_left_alone() {
    // "the weather in Boston" is a real request, and only the person can mean it.
    for named in [
        r#"{"location":"Boston"}"#,
        r#"{"city":"Boston"}"#,
        r#"{"q":"Boston"}"#,
        r#"{"lat":42.36,"lon":-71.06}"#,
    ] {
        assert_eq!(
            super::place_filled_in(named, true, "Springfield", "the weather in Boston at 42.36"),
            named,
            "overwrote a place the person asked about: {named}"
        );
    }
}

#[test]
fn a_place_nobody_named_is_put_back() {
    // The amended rule (ADR 0065): measured by `select:no-invented-place`, the model
    // asked for weather with no place named anywhere invented a city on its first
    // run. A place counts as named only if the person's own words contain it.
    let filled = super::place_filled_in(
        r#"{"location":"New York"}"#,
        true,
        "Springfield",
        "give me my morning brief",
    );
    assert!(
        filled.contains("Springfield") && !filled.contains("New York"),
        "an invented city survived: {filled}"
    );
}

#[test]
fn a_place_named_earlier_in_the_conversation_is_kept() {
    // "what about tomorrow?" after "weather in Boston" — the earlier message travels
    // in the person's words, so Boston is theirs to mean.
    let kept = super::place_filled_in(
        r#"{"location":"Boston"}"#,
        true,
        "Springfield",
        "what is the weather in Boston\nwhat about tomorrow?",
    );
    assert_eq!(kept, r#"{"location":"Boston"}"#);
}

#[test]
fn coordinates_are_left_to_the_model() {
    // Nobody types a latitude, so the person's words cannot arbitrate one — and a
    // coordinate-shaped invention has never been observed.
    let coords = r#"{"lat":42.36,"lon":-71.06}"#;
    assert_eq!(
        super::place_filled_in(coords, true, "Springfield", "morning brief"),
        coords
    );
}

#[test]
fn a_key_present_but_empty_is_not_an_answer() {
    // The failure mode that looks like an answer: the model emits the field and
    // leaves it blank. Treating that as "they named a place" is how a blank reaches
    // the geocoder, which then resolves it to somewhere famous.
    for blank in [
        r#"{"location":""}"#,
        r#"{"location":"  "}"#,
        r#"{"location":null}"#,
    ] {
        assert!(
            super::place_filled_in(blank, true, "Springfield", "the weather please")
                .contains("Springfield"),
            "an empty field counted as a place: {blank}"
        );
    }
}

#[test]
fn a_skill_that_does_not_want_a_place_is_untouched() {
    assert_eq!(
        super::place_filled_in("{}", false, "Springfield", "hi"),
        "{}"
    );
    // And nothing is invented when Endora has not been told where they are.
    assert_eq!(super::place_filled_in("{}", true, "", "hi"), "{}");
}

#[test]
fn a_call_that_will_not_parse_fails_the_way_it_already_would() {
    // Not this function's job to rescue a malformed call, and changing how it fails
    // would move the error somewhere harder to read.
    assert_eq!(
        super::place_filled_in("not json", true, "Springfield", "hi"),
        "not json"
    );
    assert_eq!(
        super::place_filled_in("[1,2]", true, "Springfield", "hi"),
        "[1,2]"
    );
}

#[test]
fn other_arguments_survive_the_fill() {
    let filled = super::place_filled_in(r#"{"days":3}"#, true, "Springfield", "forecast please");
    assert!(filled.contains("\"days\":3"), "{filled}");
    assert!(filled.contains("Springfield"), "{filled}");
}

#[test]
fn one_stance_the_band_the_record_and_the_envelope_decide() {
    use crate::application::{AutonomyEnvelope, Stance};
    use Reversibility::{Irreversible, OutwardReversible, Reversible};
    let default_env = AutonomyEnvelope::default(); // external ok, consequential no
    let no_external = AutonomyEnvelope {
        auto_external: false,
        auto_consequential: false,
    };
    let widened = AutonomyEnvelope {
        auto_external: true,
        auto_consequential: true,
    };

    // The band's defaults, where the person has said nothing.
    assert_eq!(default_stance(Reversibility::Observe), Stance::Auto);
    assert_eq!(default_stance(Reversible), Stance::Auto);
    assert_eq!(default_stance(OutwardReversible), Stance::Ask);
    assert_eq!(default_stance(Irreversible), Stance::Off);

    // Auto acts — narrowed to confirm when an external action is kept in hand.
    assert_eq!(
        classify(&info(Reversible, false), &default_env, Stance::Auto, false),
        Decision::Act
    );
    assert_eq!(
        classify(&info(Reversible, true), &no_external, Stance::Auto, false),
        Decision::Confirm
    );

    // Ask confirms each use, whatever the envelope says...
    assert_eq!(
        classify(&info(OutwardReversible, true), &widened, Stance::Ask, false),
        Decision::Confirm
    );
    // ...until the record proves the tool AND the envelope allows consequential
    // actions — then it acts. Graduation (ADR 0062).
    assert_eq!(
        classify(&info(OutwardReversible, true), &widened, Stance::Ask, true),
        Decision::Act
    );
    // Narrow the envelope and every graduate asks again.
    assert_eq!(
        classify(
            &info(OutwardReversible, true),
            &default_env,
            Stance::Ask,
            true
        ),
        Decision::Confirm
    );

    // Off blocks, whoever set it and whyever — proof and widening change nothing.
    assert_eq!(
        classify(&info(Irreversible, true), &widened, Stance::Off, true),
        Decision::Block
    );

    // `may_run_autonomously` is exactly "the verdict is Act".
    assert!(may_run_autonomously(
        &info(Reversible, false),
        &default_env,
        Stance::Auto,
        false
    ));
    assert!(!may_run_autonomously(
        &info(OutwardReversible, true),
        &widened,
        Stance::Ask,
        false
    ));
}

#[test]
fn run_refuses_an_irreversible_skill_deny_by_default() {
    // A skill whose effect can't be undone must be refused by the execution
    // path itself, not just excluded from autonomous runs (ADR 0051).
    struct IrreversibleSkill;
    impl Capability for IrreversibleSkill {
        fn info(&self) -> CapabilityInfo {
            CapabilityInfo {
                id: "wire_transfer",
                name: "Wire transfer",
                description: "",
                category: "",
                reaches_external: true,
                reversibility: Reversibility::Irreversible,
                configured: true,
                needs: "",
                settings: &[],
                wants_place: false,
                borrows_from: None,
                third_party: false,
                input_schema: None,
            }
        }
        fn invoke(
            &self,
            _input: &Value,
            _settings: &CapabilitySettings,
        ) -> Result<Value, CapabilityError> {
            panic!("an irreversible skill must never be invoked");
        }
    }
    let runner = RegistryRunner::new(Arc::new(vec![
        Arc::new(IrreversibleSkill) as Arc<dyn Capability>
    ]));
    let err = runner.run("wire_transfer", "{}").unwrap_err();
    assert!(err.contains("can't be undone"), "unexpected error: {err}");
}

#[test]
fn run_allows_an_opened_irreversible_skill_but_never_autonomously() {
    // Once the person opens a capability's irreversible band, the execution path
    // runs it (reached only via an explicit confirmation) — but it is still
    // never cleared to act on its own (ADR 0051).
    struct BookingSkill;
    impl Capability for BookingSkill {
        fn info(&self) -> CapabilityInfo {
            CapabilityInfo {
                id: "booking",
                name: "Booking",
                description: "",
                category: "",
                reaches_external: true,
                reversibility: Reversibility::Irreversible,
                configured: true,
                needs: "",
                settings: &[],
                wants_place: false,
                borrows_from: None,
                third_party: false,
                input_schema: None,
            }
        }
        fn invoke(
            &self,
            _input: &Value,
            _settings: &CapabilitySettings,
        ) -> Result<Value, CapabilityError> {
            Ok(json!({ "booked": true }))
        }
    }
    let caps: Arc<Vec<Arc<dyn Capability>>> =
        Arc::new(vec![Arc::new(BookingSkill) as Arc<dyn Capability>]);

    // Moved to `ask` for this capability, fully-widened envelope, nothing proven.
    let runner = RegistryRunner::with_config(
        caps,
        vec![("booking".to_owned(), crate::application::Stance::Ask)],
        std::collections::HashSet::new(),
        crate::application::AutonomyEnvelope {
            auto_external: true,
            auto_consequential: true,
        },
        std::collections::HashMap::new(),
    );

    // It runs now (the person confirmed) — no longer blocked.
    assert_eq!(runner.run("booking", "{}").unwrap(), r#"{"booked":true}"#);
    // But it is never autonomous: it must be confirmed every time.
    let spec = runner
        .available()
        .into_iter()
        .find(|s| s.id == "booking")
        .unwrap();
    assert!(
        !spec.autonomous,
        "an unproven irreversible skill at `ask` must still confirm"
    );
}

#[test]
fn decision_reports_the_full_verdict_including_block() {
    // `decision` exposes the real Act/Confirm/Block verdict (for the audit
    // trail), unlike the coarse `autonomous` bool which collapses the last two.
    struct BookingSkill;
    impl Capability for BookingSkill {
        fn info(&self) -> CapabilityInfo {
            CapabilityInfo {
                id: "booking",
                name: "Booking",
                description: "",
                category: "",
                reaches_external: true,
                reversibility: Reversibility::Irreversible,
                configured: true,
                needs: "",
                settings: &[],
                wants_place: false,
                borrows_from: None,
                third_party: false,
                input_schema: None,
            }
        }
        fn invoke(
            &self,
            _input: &Value,
            _settings: &CapabilitySettings,
        ) -> Result<Value, CapabilityError> {
            Ok(json!({}))
        }
    }
    let caps: Arc<Vec<Arc<dyn Capability>>> =
        Arc::new(vec![Arc::new(BookingSkill) as Arc<dyn Capability>]);

    // Closed: the un-undoable is Blocked (the bool would only say "not autonomous").
    let closed = RegistryRunner::new(caps.clone());
    assert_eq!(closed.decision("booking"), Some(Decision::Block));
    // At `ask`, unproven: Confirm — never Act.
    let opened = RegistryRunner::with_config(
        caps,
        vec![("booking".to_owned(), crate::application::Stance::Ask)],
        std::collections::HashSet::new(),
        crate::application::AutonomyEnvelope::default(),
        std::collections::HashMap::new(),
    );
    assert_eq!(opened.decision("booking"), Some(Decision::Confirm));
    // Unknown skill: no verdict.
    assert_eq!(closed.decision("nope"), None);
}

#[test]
fn registry_has_modules_with_stable_ids() {
    let caps = default_capabilities();
    assert!(caps.len() >= 6);
    let ids: Vec<_> = caps.iter().map(|c| c.info().id).collect();
    assert!(ids.contains(&"weather"));
    assert!(ids.contains(&"web_fetch"));
    assert!(ids.contains(&"flights"));
}

#[test]
fn a_scaffold_reports_unavailable_with_a_reason() {
    let err = FlightSearchCapability
        .invoke(&json!({}), &CapabilitySettings::new())
        .unwrap_err();
    assert!(matches!(err, CapabilityError::Unavailable(_)));
}

#[test]
fn observed_time_formats_the_open_meteo_timestamp() {
    assert_eq!(
        observed_time("2026-07-23T07:00").as_deref(),
        Some("7:00 AM")
    );
    assert_eq!(
        observed_time("2026-07-23T00:15").as_deref(),
        Some("12:15 AM")
    );
    assert_eq!(
        observed_time("2026-07-23T13:30").as_deref(),
        Some("1:30 PM")
    );
    assert_eq!(
        observed_time("2026-07-23T12:00").as_deref(),
        Some("12:00 PM")
    );
    // Unrecognized shapes fall back to no tag (rather than a wrong one).
    assert_eq!(observed_time("garbage"), None);
    assert_eq!(observed_time("2026-07-23T99:00"), None);
}

#[test]
fn a_built_in_keeps_the_flags_it_declared() {
    // The seam that shipped broken: `available()` hardcoded both flags false,
    // so the place guard (ADR 0065) and the stranger taint (ADR 0064) silently
    // never applied to any built-in. Weather declares wants_place; news and
    // mail declare third_party; the specs must say so.
    let runner = RegistryRunner::new(Arc::new(default_capabilities()));
    let spec = |id: &str| {
        runner
            .available()
            .into_iter()
            .find(|s| s.id == id)
            .unwrap_or_else(|| panic!("{id} is registered"))
    };
    assert!(spec("weather").wants_place, "weather declared wants_place");
    assert!(spec("news").third_party, "news is other people's words");
    assert!(spec("mail").third_party, "mail is other people's words");
    assert!(
        !spec("home_assistant").third_party,
        "the house is not a stranger"
    );
}

#[test]
fn a_city_with_a_state_code_is_split_for_the_listing_service() {
    assert_eq!(
        city_and_state("Springfield IL"),
        ("Springfield".to_owned(), Some("IL".to_owned()))
    );
    assert_eq!(
        city_and_state("Springfield, IL"),
        ("Springfield".to_owned(), Some("IL".to_owned()))
    );
    assert_eq!(
        city_and_state("New York ny"),
        ("New York".to_owned(), Some("NY".to_owned()))
    );
    // A bare city stays whole, and a lone two-letter word is a city, not a state.
    assert_eq!(city_and_state("Boston"), ("Boston".to_owned(), None));
    assert_eq!(city_and_state("NY"), ("NY".to_owned(), None));
}

#[test]
fn traffic_reads_as_drives_and_absence_names_the_fix() {
    let s = TrafficCapability.summarize(&json!({"drives": [
        {"name": "Home to Work", "minutes": 18.4, "route": "I-77 S"},
        {"name": "School run", "minutes": 12.0, "route": null},
    ]}));
    assert_eq!(
        s,
        "Traffic: Home to Work — 18 min via I-77 S · School run — 12 min"
    );
    // No sensors is an honest absence that says what to add, never a guess.
    let none = TrafficCapability.summarize(&json!({"drives": []}));
    assert!(none.contains("Waze Travel Time"), "{none}");
}

#[test]
fn a_news_call_says_how_many_headlines_it_wants() {
    assert_eq!(wanted_headlines(&json!({"count": 3})), 3);
    assert_eq!(
        wanted_headlines(&json!({})),
        6,
        "the historical default holds"
    );
    assert_eq!(
        wanted_headlines(&json!({"count": 0})),
        1,
        "zero is not a reading"
    );
    assert_eq!(
        wanted_headlines(&json!({"count": 50})),
        6,
        "fifty is a feed"
    );
}

#[test]
fn the_weather_summary_says_when_an_umbrella_is_worth_taking() {
    // The person asked for exactly this: "weather, and if you need an umbrella
    // or jacket". Whether you need one is a fact of the numbers, so it is said
    // by code, not left to the model's mood.
    let reading = json!({
        "place": "New York", "condition": "light rain",
        "temperature_c": 18.0, "feels_like_c": 17.0,
        "high_c": 21.0, "low_c": 14.0,
        "rain_chance_pct": 65,
        "observed_at": "2026-08-07T07:00", "warning": "",
    });
    let s = WeatherCapability.summarize(&reading);
    assert!(s.contains("umbrella"), "{s}");
    assert!(s.contains("65%"), "{s}");
    assert!(!s.contains("jacket"), "17°C is not jacket weather: {s}");
}

#[test]
fn a_cold_morning_calls_for_a_jacket() {
    let reading = json!({
        "place": "New York", "condition": "clear sky",
        "temperature_c": 9.0, "feels_like_c": 6.0,
        "high_c": 15.0, "low_c": 5.0,
        "rain_chance_pct": 5,
        "observed_at": "2026-08-07T07:00", "warning": "",
    });
    let s = WeatherCapability.summarize(&reading);
    assert!(s.contains("jacket"), "{s}");
    assert!(!s.contains("umbrella"), "5% is not umbrella weather: {s}");
}

#[test]
fn a_dry_mild_day_gets_no_wardrobe_advice() {
    // Advice that fires every day is noise, and noise teaches the person to
    // stop reading.
    let reading = json!({
        "place": "New York", "condition": "partly cloudy",
        "temperature_c": 22.0, "feels_like_c": 23.0,
        "high_c": 27.0, "low_c": 18.0,
        "rain_chance_pct": 10,
        "observed_at": "2026-08-07T07:00", "warning": "",
    });
    let s = WeatherCapability.summarize(&reading);
    assert!(!s.contains("umbrella"), "{s}");
    assert!(!s.contains("jacket"), "{s}");
}

#[test]
fn a_reading_without_the_rain_field_stays_quiet_about_umbrellas() {
    // An older reading (or an API that omitted the field) must not produce
    // advice from a value that is not there.
    let reading = json!({
        "place": "New York", "condition": "partly cloudy",
        "temperature_c": 22.0, "feels_like_c": 23.0,
        "high_c": 27.0, "low_c": 18.0,
        "observed_at": "2026-08-07T07:00", "warning": "",
    });
    let s = WeatherCapability.summarize(&reading);
    assert!(!s.contains("umbrella"), "{s}");
}

#[test]
fn geocode_candidates_simplify_city_state() {
    assert_eq!(
        geocode_candidates("New York NY"),
        vec!["New York NY", "New York"]
    );
    assert_eq!(
        geocode_candidates("New York, NY"),
        vec!["New York, NY", "New York"]
    );
    assert_eq!(geocode_candidates("Boston"), vec!["Boston"]);
    assert_eq!(geocode_candidates("San Francisco"), vec!["San Francisco"]);
}

#[test]
fn zip_detector_ignores_non_zip_input_without_a_network_call() {
    // Non-5-digit or non-numeric input must fall through (Ok(None)), so a place
    // name never triggers the ZIP lookup.
    assert!(resolve_us_zip("New York").unwrap().is_none());
    assert!(resolve_us_zip("2827").unwrap().is_none());
    assert!(resolve_us_zip("abcde").unwrap().is_none());
    assert!(resolve_us_zip("Boston, MA").unwrap().is_none());
}

#[test]
fn outbound_tripwire_flags_secrets_but_not_ordinary_text() {
    // Ordinary queries and URLs are NOT flagged (no false positives).
    assert_eq!(scan_outbound_secret("what's the weather in New York"), None);
    assert_eq!(
        scan_outbound_secret("https://example.com/articles/2026/summer-festival"),
        None
    );
    assert_eq!(scan_outbound_secret("{\"location\":\"10001\"}"), None);
    // Known credential shapes ARE flagged.
    assert_eq!(
        scan_outbound_secret("{\"q\":\"my key is sk-abcdefabcdefabcdefabcdef\"}"),
        Some("an API key")
    );
    assert_eq!(
        scan_outbound_secret("AKIAIOSFODNN7EXAMPLE and more"),
        Some("an AWS access key")
    );
    assert_eq!(
        scan_outbound_secret("ghp_1234567890abcdefghijklmnopqrstuv"),
        Some("a GitHub token")
    );
    assert!(scan_outbound_secret("-----BEGIN RSA PRIVATE KEY-----").is_some());
    // A secret embedded in a URL query string is caught too.
    assert_eq!(
        scan_outbound_secret("https://x.com/?token=sk-abcdef1234567890abcdef"),
        Some("an API key")
    );
    assert_eq!(
        scan_outbound_secret("token eyJhbGciOi.eyJzdWIiOi.SflKxwRJSM"),
        Some("a token (JWT)")
    );
}

#[test]
fn query_minimization_redacts_emails_but_not_urls_or_plain_text() {
    // A standalone email in a query is redacted.
    assert_eq!(
        redact_emails_in_text("email john.doe@example.com about the trip"),
        "email [redacted-email] about the trip"
    );
    // Trailing punctuation is handled; the rest of the word survives.
    assert_eq!(
        redact_emails_in_text("(contact a@b.com)"),
        "(contact [redacted-email])"
    );
    // A URL is one word and not an email — left intact.
    assert_eq!(
        redact_emails_in_text("https://site.com/path?x=1"),
        "https://site.com/path?x=1"
    );
    // Ordinary text is untouched.
    assert_eq!(
        redact_emails_in_text("weather in New York tomorrow"),
        "weather in New York tomorrow"
    );
    assert!(!looks_like_email("https://a.com/x@y"));
    assert!(looks_like_email("a@b.co"));
}

#[test]
fn query_minimization_walks_json_values() {
    let mut v = json!({ "query": "reach me at me@x.org", "url": "https://ok.com/" });
    redact_pii_in_value(&mut v);
    assert_eq!(v["query"], json!("reach me at [redacted-email]"));
    assert_eq!(v["url"], json!("https://ok.com/")); // url untouched
}

#[test]
fn egress_guard_blocks_internal_ip_literals() {
    // Loopback, RFC1918, link-local (incl. cloud metadata), and IPv6 loopback.
    for url in [
        "http://127.0.0.1/x",
        "http://192.168.1.10:8787/data",
        "http://10.0.0.5/",
        "http://169.254.169.254/latest/meta-data",
        "https://[::1]/",
        "http://[fd00::1]/",
    ] {
        assert!(
            matches!(guard_egress(url), Err(CapabilityError::BadInput(_))),
            "should have blocked {url}"
        );
    }
    // A non-http scheme is refused too.
    assert!(guard_egress("ftp://8.8.8.8/").is_err());
}

#[test]
fn egress_guard_allows_public_ip_literals() {
    assert!(guard_egress("https://8.8.8.8/").is_ok());
    assert!(guard_egress("http://1.1.1.1:8080/x").is_ok());
}

#[test]
fn host_and_port_parses_forms() {
    assert_eq!(
        host_and_port("https://example.com/path"),
        Some(("example.com".to_owned(), 443))
    );
    assert_eq!(
        host_and_port("http://example.com:8080/x?y=1"),
        Some(("example.com".to_owned(), 8080))
    );
    assert_eq!(
        host_and_port("http://user:pass@10.0.0.1/x"),
        Some(("10.0.0.1".to_owned(), 80))
    );
    assert_eq!(
        host_and_port("https://[::1]:9000/"),
        Some(("::1".to_owned(), 9000))
    );
}

#[test]
fn redirect_resolution_handles_absolute_and_relative() {
    assert_eq!(
        resolve_redirect("https://a.com/x", "https://b.com/y"),
        "https://b.com/y"
    );
    assert_eq!(
        resolve_redirect("https://a.com/x/y", "/z"),
        "https://a.com/z"
    );
}

#[test]
fn web_fetch_rejects_non_http() {
    let err = WebFetchCapability
        .invoke(
            &json!({ "url": "file:///etc/passwd" }),
            &CapabilitySettings::new(),
        )
        .unwrap_err();
    assert!(matches!(err, CapabilityError::BadInput(_)));
}

#[test]
fn rss_items_are_extracted_with_titles_links_and_publisher() {
    let xml = "<rss><channel>\
        <item><title>Storms hit New York &amp; the region</title><link>https://ex.com/a</link>\
          <source url=\"https://www.wcnc.com\">WCNC</source></item>\
        <item><title><![CDATA[City council votes tonight]]></title></item>\
        <item><title>Third &#39;big&#39; story</title></item>\
        </channel></rss>";
    let items = extract_rss_items(xml, 6);
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].0, "Storms hit New York & the region");
    assert_eq!(items[0].1, "https://ex.com/a"); // the source link is kept
    assert_eq!(items[0].2, "WCNC"); // publisher name from <source>
    assert_eq!(items[1].0, "City council votes tonight");
    assert_eq!(items[1].1, ""); // no link in the feed ⇒ empty, not fabricated
    assert_eq!(items[1].2, ""); // no <source> ⇒ empty, not fabricated
    assert_eq!(items[2].0, "Third 'big' story");
}

#[test]
fn news_without_a_place_or_query_is_bad_input() {
    let err = LocalNewsCapability
        .invoke(&json!({}), &CapabilitySettings::new())
        .unwrap_err();
    assert!(matches!(err, CapabilityError::BadInput(_)));
}

#[test]
fn knowledge_summarize_reads_the_extract_or_says_nothing_found() {
    let hit = json!({ "query": "Ada Lovelace", "found": true, "title": "Ada Lovelace",
        "extract": "Ada Lovelace was an English mathematician." });
    assert_eq!(
        KnowledgeCapability.summarize(&hit),
        "Ada Lovelace: Ada Lovelace was an English mathematician."
    );
    let miss = json!({ "query": "asdfqwer", "found": false, "extract": "" });
    assert!(
        KnowledgeCapability
            .summarize(&miss)
            .contains("couldn't find")
    );
}

#[test]
fn internet_search_is_declared_with_required_secret_configuration() {
    let searching: Vec<&str> = default_capabilities()
        .iter()
        .map(|c| c.info().id)
        .filter(|id| *id == "web_search")
        .collect();
    assert_eq!(searching, vec!["web_search"]);
    let info = WebSearchCapability.info();
    assert!(info.settings[0].secret && !info.settings[0].optional);
    assert_eq!(info.reversibility, Reversibility::Observe);
}

#[test]
fn home_assistant_needs_config_and_summarizes_state() {
    // Without url/token it's unavailable (never reaches the home).
    assert!(
        HomeAssistantCapability
            .invoke(&json!({}), &CapabilitySettings::new())
            .is_err()
    );
    // Summary reads entities as name: state.
    let out = json!({ "domain": "light", "count": 2, "entities": [
        { "entity": "light.kitchen", "name": "Kitchen", "state": "on" },
        { "entity": "light.desk", "name": "Desk", "state": "off" },
    ] });
    let s = HomeAssistantCapability.summarize(&out);
    assert!(s.contains("Kitchen: on"));
    assert!(s.contains("Desk: off"));
}

#[test]
fn news_summarize_gives_a_readable_numbered_list() {
    let out = json!({
        "query": "New York news",
        "count": 2,
        "headlines": ["Council meets tonight", "Road closures downtown"],
        "note": "",
    });
    let text = LocalNewsCapability.summarize(&out);
    assert!(text.contains("New York news"));
    assert!(text.contains("1. Council meets tonight"));
    assert!(text.contains("2. Road closures downtown"));
    // Empty results read as a plain "none found", never as raw JSON.
    let empty = json!({ "query": "Nowhere news", "count": 0, "headlines": [] });
    assert!(
        LocalNewsCapability
            .summarize(&empty)
            .contains("No recent news")
    );
}

#[test]
fn strip_html_drops_tags_and_scripts() {
    let t = strip_html("<html><script>bad()</script><p>Hello <b>world</b></p></html>");
    assert_eq!(t, "Hello world");
}

#[test]
fn strip_html_survives_and_keeps_text_that_is_not_ascii() {
    // Found by a live run: a page with an arrow in its navigation panicked the fetch
    // ("byte index is not a char boundary"), because the scan stepped a byte at a time and
    // sliced the string at each step. Before that it would have mangled every accented
    // letter, by treating each byte of a character as a character of its own.
    let t = strip_html("<nav>← Back</nav><p>Café — naïve 日本語</p>");
    assert_eq!(t, "← Back Café — naïve 日本語");
}

#[test]
fn strip_html_skips_a_script_or_style_whatever_it_contains_and_however_it_is_cased() {
    let t = strip_html(
        "<P>a</P><SCRIPT>var s = 'é←';</SCRIPT><p>b</p><Style>p::before{content:'→'}</Style><p>c</p>",
    );
    assert_eq!(t, "a b c");
}

#[test]
fn strip_html_is_not_thrown_off_by_letters_that_change_length_when_lowercased() {
    // 'İ' lowercases to two characters, so a lowercased copy no longer lines up with the
    // original byte for byte; every position after it was then read from the wrong place.
    let t = strip_html("<p>İstanbul</p><script>x</script><p>after</p>");
    assert_eq!(t, "İstanbul after");
}

#[test]
fn image_review_declares_a_model_setting_and_needs_it() {
    let cap = ImageReviewCapability {
        ollama_base: "http://localhost:11434".to_owned(),
    };
    // It declares a required "model" setting...
    assert_eq!(cap.info().settings.len(), 1);
    assert_eq!(cap.info().settings[0].key, "model");
    // ...and without it, running fails (never reaches the vision call).
    assert!(
        cap.invoke(
            &json!({ "image_url": "http://x/y.png" }),
            &CapabilitySettings::new()
        )
        .is_err()
    );
}

#[test]
fn settings_complete_requires_every_declared_setting() {
    let cap = ImageReviewCapability {
        ollama_base: "http://localhost:11434".to_owned(),
    };
    let info = cap.info();
    assert!(!settings_complete(&info, &CapabilitySettings::new()));
    let mut s = CapabilitySettings::new();
    s.insert("model".to_owned(), "moondream".to_owned());
    assert!(settings_complete(&info, &s));
    // A keyless skill is always complete.
    assert!(settings_complete(
        &WeatherCapability.info(),
        &CapabilitySettings::new()
    ));
}

// ---- MCP host: adapter + composite -------------------------------------

use crate::application::CapabilitySpec;

/// A stand-in MCP server. `call` echoes `tool(input)` so a test can prove which
/// tool was reached; an unhealthy server fails `list_tools`. No interior
/// mutability, so it stays `Send + Sync` for `Box<dyn McpClient>`.
struct FakeTransport {
    tools: Vec<McpToolInfo>,
    healthy: bool,
}

impl McpClient for FakeTransport {
    fn list_tools(&self) -> Result<Vec<McpToolInfo>, String> {
        if self.healthy {
            Ok(self.tools.clone())
        } else {
            Err("server down".to_owned())
        }
    }
    fn call(&self, tool: &str, input_json: &str) -> Result<String, String> {
        Ok(format!("{tool}({input_json})"))
    }
}

/// A stand-in MCP server that also offers **resources** — the standard half of the
/// protocol Endora did not speak (ADR 0058).
struct FakeWithResources {
    resources: Vec<McpResource>,
    unreadable: bool,
}

impl McpClient for FakeWithResources {
    fn list_tools(&self) -> Result<Vec<McpToolInfo>, String> {
        Ok(vec![tool("do_something")])
    }
    fn call(&self, tool: &str, input: &str) -> Result<String, String> {
        Ok(format!("{tool}({input})"))
    }
    fn list_resources(&self) -> Result<Vec<McpResource>, String> {
        Ok(self.resources.clone())
    }
    fn read_resource(&self, uri: &str) -> Result<String, String> {
        if self.unreadable {
            return Err("gone".to_owned());
        }
        Ok(format!("state of {uri}"))
    }
}

fn resource(uri: &str) -> McpResource {
    McpResource {
        uri: uri.to_owned(),
        name: uri.to_owned(),
        description: String::new(),
    }
}

#[test]
fn an_mcp_servers_resources_become_states_the_watch_loop_can_see() {
    // The payoff of speaking the other half of MCP: a third-party server can now feed the
    // watch loop, the transition log and notions with no Rust in this repository at all.
    let runner = McpRunner::connect_with_readers(vec![(
        "house".to_owned(),
        Box::new(FakeWithResources {
            resources: vec![resource("house://light.kitchen"), resource("house://door")],
            unreadable: false,
        }) as Box<dyn McpClient>,
        String::new(),
    )]);

    let mut states = runner.current_states();
    states.sort();
    assert_eq!(
        states,
        vec![
            (
                "house::house://door".to_owned(),
                "state of house://door".to_owned()
            ),
            (
                "house::house://light.kitchen".to_owned(),
                "state of house://light.kitchen".to_owned()
            ),
        ],
        "states must say which server they came from"
    );
}

#[test]
fn two_servers_publishing_the_same_name_do_not_collide() {
    // A flat key would have one silently overwrite the other, and both would still look
    // present — the kind of loss nothing downstream could detect.
    let runner = McpRunner::connect_with_readers(vec![
        (
            "house".to_owned(),
            Box::new(FakeWithResources {
                resources: vec![resource("status")],
                unreadable: false,
            }) as Box<dyn McpClient>,
            String::new(),
        ),
        (
            "network".to_owned(),
            Box::new(FakeWithResources {
                resources: vec![resource("status")],
                unreadable: false,
            }) as Box<dyn McpClient>,
            String::new(),
        ),
    ]);
    let mut keys: Vec<String> = runner
        .current_states()
        .into_iter()
        .map(|(key, _)| key)
        .collect();
    keys.sort();
    assert_eq!(keys, vec!["house::status", "network::status"]);
}

#[test]
fn a_tools_only_server_reports_no_states_rather_than_failing() {
    // The ordinary case. Most MCP servers have no resources, and that must never look
    // like trouble — the watch loop treats absence as something to raise.
    let runner = McpRunner::connect_with_readers(vec![(
        "plain".to_owned(),
        Box::new(FakeTransport {
            tools: vec![tool("search")],
            healthy: true,
        }) as Box<dyn McpClient>,
        String::new(),
    )]);
    assert!(runner.current_states().is_empty());
}

#[test]
fn a_resource_that_will_not_read_is_skipped_not_fatal() {
    // One unreadable resource must not cost the watch loop every other reading it had.
    let runner = McpRunner::connect_with_readers(vec![(
        "house".to_owned(),
        Box::new(FakeWithResources {
            resources: vec![resource("house://gone")],
            unreadable: true,
        }) as Box<dyn McpClient>,
        String::new(),
    )]);
    assert!(runner.current_states().is_empty());
}

fn tool(name: &str) -> McpToolInfo {
    McpToolInfo {
        name: name.to_owned(),
        description: format!("does {name}"),
        input_schema: None,
    }
}

/// A tool that advertises the arguments it takes, so schema-against-schema logic
/// has something real to match.
fn schema_tool(name: &str, fields: &[&str]) -> McpToolInfo {
    let props: serde_json::Map<String, Value> = fields
        .iter()
        .map(|f| ((*f).to_owned(), serde_json::json!({ "type": "string" })))
        .collect();
    McpToolInfo {
        name: name.to_owned(),
        description: format!("does {name}"),
        input_schema: Some(serde_json::json!({ "type": "object", "properties": props })),
    }
}

#[test]
fn mcp_runner_namespaces_tools_is_deny_by_default_and_routes() {
    let transport = FakeTransport {
        tools: vec![tool("create_event"), tool("list_events")],
        healthy: true,
    };
    let runner = McpRunner::connect(vec![("calendar".to_owned(), Box::new(transport))]);

    // Tools appear namespaced, configured, and never autonomous.
    let ids: Vec<String> = runner.available().into_iter().map(|s| s.id).collect();
    assert_eq!(ids, vec!["calendar.create_event", "calendar.list_events"]);
    assert!(
        runner
            .available()
            .iter()
            .all(|s| s.configured && !s.autonomous)
    );

    // Deny-by-default: a hosted tool blocks; an unknown id is None.
    assert_eq!(
        runner.decision("calendar.create_event"),
        Some(Decision::Block)
    );
    assert_eq!(runner.decision("calendar.nope"), None);
    assert_eq!(runner.decision("weather"), None);

    // `run` routes faithfully to the transport (its contract: only called once
    // policy has cleared the tool).
    assert_eq!(
        runner.run("calendar.create_event", "{\"x\":1}").unwrap(),
        "create_event({\"x\":1})"
    );
    assert!(runner.run("calendar.nope", "{}").is_err());
}

#[test]
fn coerce_wraps_a_scalar_where_the_schema_wants_an_array() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "domain": { "type": "array" },
            "name": { "type": "string" }
        }
    });
    // The classic HA slip: domain sent as a string, not an array.
    let out = coerce_args_to_schema(
        r#"{"domain":"light","name":"kitchen"}"#,
        Some(&schema),
        false,
    );
    let v: serde_json::Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["domain"], serde_json::json!(["light"]));
    // A non-array field is left alone; an already-array value is untouched.
    assert_eq!(v["name"], "kitchen");
    let already = coerce_args_to_schema(r#"{"domain":["light"]}"#, Some(&schema), false);
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&already).unwrap()["domain"],
        serde_json::json!(["light"])
    );
    // No schema, or unparseable input → passed through unchanged.
    assert_eq!(coerce_args_to_schema("{bad", Some(&schema), false), "{bad");
    assert_eq!(
        coerce_args_to_schema(r#"{"x":1}"#, None, false),
        r#"{"x":1}"#
    );
}

/// The hint names what a tool **needs**, and counts the rest.
///
/// It used to list every parameter a server declared. A search tool with nine of them
/// read to a small model as nine things to collect, and it asked the person for them —
/// "the count of results you'd like to see, your preferred country, what level of
/// freshness you require". That is somebody else's API surface, put to the person as a
/// question. Optional means the server has a default.
#[test]
fn compact_input_hint_names_what_is_needed_and_counts_the_rest() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "area": { "type": "string" }
        },
        "required": ["name"]
    });
    let hint = compact_input_hint(&schema).unwrap();
    assert!(hint.contains("name (string)"), "got: {hint}");
    assert!(
        !hint.contains("area"),
        "an optional field was named: {hint}"
    );
    assert!(hint.contains("1 more"), "got: {hint}");

    // Everything optional: say so rather than listing a menu to be collected.
    let all_optional = serde_json::json!({
        "type": "object",
        "properties": { "count": { "type": "integer" }, "country": { "type": "string" } }
    });
    let hint = compact_input_hint(&all_optional).unwrap();
    assert!(hint.contains("no arguments"), "got: {hint}");
    assert!(!hint.contains("country"), "got: {hint}");
    // A no-argument tool needs no hint.
    assert!(
        compact_input_hint(&serde_json::json!({ "type": "object", "properties": {} })).is_none()
    );
    assert!(compact_input_hint(&serde_json::json!({})).is_none());
}

#[test]
fn mcp_available_folds_the_input_schema_into_the_description() {
    let mut turn_on = tool("HassTurnOn");
    turn_on.description = "Turns on a device".to_owned();
    turn_on.input_schema = Some(serde_json::json!({
        "type": "object",
        "properties": { "name": { "type": "string" } },
        "required": ["name"]
    }));
    let transport = FakeTransport {
        tools: vec![turn_on],
        healthy: true,
    };
    let runner = McpRunner::connect(vec![("home".to_owned(), Box::new(transport))]);
    let spec = runner
        .available()
        .into_iter()
        .find(|s| s.id == "home.HassTurnOn")
        .unwrap();
    // The model sees both what the tool does AND how to call it.
    assert!(spec.description.contains("Turns on a device"));
    // What it NEEDS, not every parameter it will accept — a menu of optional fields
    // reads to a small model as things to go and collect from the person.
    assert!(
        spec.description.contains("needs: name (string)"),
        "description should carry the input hint, got: {}",
        spec.description
    );
}

#[test]
fn mcp_runner_skips_an_unhealthy_server() {
    let ok = FakeTransport {
        tools: vec![tool("read_file")],
        healthy: true,
    };
    let down = FakeTransport {
        tools: vec![tool("send_mail")],
        healthy: false,
    };
    let runner = McpRunner::connect(vec![
        ("files".to_owned(), Box::new(ok)),
        ("mail".to_owned(), Box::new(down)),
    ]);
    // Only the healthy server contributes tools; the down one drops out silently.
    let ids: Vec<String> = runner.available().into_iter().map(|s| s.id).collect();
    assert_eq!(ids, vec!["files.read_file"]);
}

/// A minimal built-in-like source: an autonomous `weather` skill.
struct FakeBuiltin;
impl CapabilityRunner for FakeBuiltin {
    fn available(&self) -> Vec<CapabilitySpec> {
        vec![CapabilitySpec {
            id: "weather".to_owned(),
            wants_place: false,
            third_party: false,
            description: "the weather".to_owned(),
            configured: true,
            autonomous: true,
            input_schema: None,
            reversibility: endora_kernel::Reversibility::Observe,
        }]
    }
    fn run(&self, id: &str, _input: &str) -> Result<String, String> {
        if id == "weather" {
            Ok("sunny".to_owned())
        } else {
            Err(format!("no such skill '{id}'"))
        }
    }
}

#[test]
fn composite_runner_merges_sources_and_routes_by_owner() {
    let mcp = McpRunner::connect(vec![(
        "calendar".to_owned(),
        Box::new(FakeTransport {
            tools: vec![tool("create_event")],
            healthy: true,
        }) as Box<dyn McpClient>,
    )]);
    let composite = CompositeRunner::new(vec![Arc::new(FakeBuiltin), Arc::new(mcp)]);

    // Both sources' catalogs are merged.
    let ids: Vec<String> = composite.available().into_iter().map(|s| s.id).collect();
    assert_eq!(ids, vec!["weather", "calendar.create_event"]);

    // Each source keeps its own policy verdict.
    assert_eq!(composite.decision("weather"), Some(Decision::Act));
    assert_eq!(
        composite.decision("calendar.create_event"),
        Some(Decision::Block)
    );
    assert_eq!(composite.decision("missing"), None);

    // `run` dispatches to whichever source owns the id.
    assert_eq!(composite.run("weather", "{}").unwrap(), "sunny");
    assert_eq!(
        composite.run("calendar.create_event", "{}").unwrap(),
        "create_event({})"
    );
    assert!(composite.run("missing", "{}").is_err());
}

#[test]
fn a_confirmed_alias_recovers_a_call_that_failed_on_the_name() {
    // The live case, after hours of it: the entity is `Kitchen Table`, the model kept
    // sending `name: "table"`, and Home Assistant kept answering
    // no_match_reason=NAME. With the alias in its prompt it STILL sent "table" —
    // grounding is advice, and this model takes advice about a third of the time.
    struct OnlyKnowsTheRealName;
    impl CapabilityRunner for OnlyKnowsTheRealName {
        fn available(&self) -> Vec<crate::application::CapabilitySpec> {
            vec![crate::application::CapabilitySpec {
                id: "home-assistant.HassTurnOn".to_owned(),
                wants_place: false,
                third_party: false,
                description: String::new(),
                configured: true,
                autonomous: true,
                input_schema: None,
                reversibility: Reversibility::Irreversible,
            }]
        }
        fn run(&self, _id: &str, input: &str) -> Result<String, String> {
            if input.contains("Kitchen Table") {
                return Ok("action_done".to_owned());
            }
            Err("no_match_reason=NAME".to_owned())
        }
    }

    let runner = AliasRunner::new(
        Arc::new(OnlyKnowsTheRealName),
        vec![(
            "home-assistant".to_owned(),
            "table".to_owned(),
            "Kitchen Table".to_owned(),
        )],
    );

    let out = runner
        .run("home-assistant.HassTurnOn", r#"{"name":"table"}"#)
        .expect("the alias recovers it");
    assert!(out.contains("action_done"));
    // And it says what it did — the substitution is visible to the model, the outcome
    // record and the person. Recovering silently would hide the model's mistake.
    assert!(
        out.contains("retried") && out.contains("Kitchen Table"),
        "the retry was silent: {out}"
    );
}

#[test]
fn an_alias_never_touches_a_call_that_worked() {
    // Recovery, never pre-emption: it fires only after a failure, so it can never
    // redirect a call that was about to hit the right thing.
    struct AlwaysWorks;
    impl CapabilityRunner for AlwaysWorks {
        fn available(&self) -> Vec<crate::application::CapabilitySpec> {
            Vec::new()
        }
        fn run(&self, _id: &str, input: &str) -> Result<String, String> {
            Ok(format!("ran with {input}"))
        }
    }
    let runner = AliasRunner::new(
        Arc::new(AlwaysWorks),
        vec![(
            "home-assistant".to_owned(),
            "table".to_owned(),
            "Kitchen Table".to_owned(),
        )],
    );
    let out = runner
        .run("home-assistant.HassTurnOn", r#"{"name":"table"}"#)
        .unwrap();
    assert!(
        out.contains(r#""table""#),
        "a working call was rewritten: {out}"
    );
    assert!(!out.contains("Kitchen Table"));
}

#[test]
fn an_alias_matches_a_whole_value_not_a_fragment() {
    // "table" must not rewrite "comfortable". Saying what one thing is called is not
    // licence to edit every value containing that word.
    struct Fails;
    impl CapabilityRunner for Fails {
        fn available(&self) -> Vec<crate::application::CapabilitySpec> {
            Vec::new()
        }
        fn run(&self, _id: &str, _input: &str) -> Result<String, String> {
            Err("nope".to_owned())
        }
    }
    let runner = AliasRunner::new(
        Arc::new(Fails),
        vec![(
            "home-assistant".to_owned(),
            "table".to_owned(),
            "Kitchen Table".to_owned(),
        )],
    );
    // No whole-value match, so no retry input exists and the original error stands.
    assert_eq!(
        runner.run(
            "home-assistant.HassTurnOn",
            r#"{"name":"comfortable chair"}"#
        ),
        Err("nope".to_owned())
    );
}

#[test]
fn the_nominated_reader_is_an_observation_not_a_receipt() {
    // Observed live: GetLiveContext's own result came back stamped
    //   "[unverified] This is what the tool reported about its own work."
    // A read has no work of its own, and that text tells the model not to trust the
    // very reading ADR 0053 uses as confirmation.
    let mcp = McpRunner::connect_with_readers(vec![(
        "home-assistant".to_owned(),
        Box::new(FakeTransport {
            tools: vec![tool("HassTurnOff"), tool("GetLiveContext")],
            healthy: true,
        }) as Box<dyn McpClient>,
        "GetLiveContext".to_owned(),
    )]);

    let specs = mcp.available();
    let reader = specs
        .iter()
        .find(|s| s.id == "home-assistant.GetLiveContext")
        .expect("the reader is offered");
    assert_eq!(reader.reversibility, Reversibility::Observe);
    assert!(reader.autonomous, "a read may run on its own");
    assert_eq!(
        mcp.decision("home-assistant.GetLiveContext"),
        Some(Decision::Act)
    );

    // Everything else on the server stays deny-by-default, and is verified through
    // the nominated reader.
    let action = specs
        .iter()
        .find(|s| s.id == "home-assistant.HassTurnOff")
        .expect("the action is offered");
    assert_eq!(action.reversibility, Reversibility::Irreversible);
    assert!(!action.autonomous);
    assert_eq!(
        mcp.verifier("home-assistant.HassTurnOff").as_deref(),
        Some("home-assistant.GetLiveContext")
    );
    // A reader never verifies itself.
    assert_eq!(mcp.verifier("home-assistant.GetLiveContext"), None);
}

#[test]
fn arguments_the_schema_forbids_are_dropped_not_sent() {
    // Live failure: device_class:["light"] — not one of that field's permitted
    // values — failed the WHOLE call with a validation error, so a light nobody
    // could name stayed on. The schema says which values are allowed; use it.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "floor": { "type": "string" },
            "device_class": {
                "type": "array",
                "items": { "enum": ["outlet", "switch", "tv"] }
            }
        }
    });
    let out = coerce_args_to_schema(
        r#"{"name":"main","floor":"","device_class":["light"]}"#,
        Some(&schema),
        false,
    );
    let parsed: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(parsed["name"], "main");
    assert!(
        parsed.get("device_class").is_none(),
        "an invalid enum value was sent: {out}"
    );
    assert!(
        parsed.get("floor").is_none(),
        "an empty filter was sent: {out}"
    );
}

#[test]
fn cleaning_a_call_never_turns_one_target_into_all_of_them() {
    // Observed live, and caused by the empty-field hygiene I added:
    //   HassTurnOn{area:null, name:null, domain:["light"]}
    // cleaned to {domain:["light"]} — and Home Assistant turned on every light in
    // the house. The model plainly meant to name something and sent nulls; the
    // honest reading is "it failed to say what", not "it meant all of them".
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "area": { "type": "string" },
            "name": { "type": "string" },
            "domain": { "type": "array", "items": { "type": "string" } }
        }
    });
    let out = coerce_args_to_schema(
        r#"{"area":null,"name":null,"domain":["light"]}"#,
        Some(&schema),
        false,
    );
    let parsed: Value = serde_json::from_str(&out).unwrap();
    assert!(
        parsed.get("area").is_some() || parsed.get("name").is_some(),
        "the empties were cleaned away, leaving a house-wide action: {out}"
    );
}

#[test]
fn a_call_with_one_real_target_is_still_cleaned() {
    // The guard must not undo the fix it sits next to: an empty `floor` alongside a
    // real name still gets dropped, because the call still aims at something.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "name": { "type": "string" },
            "floor": { "type": "string" },
            "domain": { "type": "array", "items": { "type": "string" } }
        }
    });
    let out = coerce_args_to_schema(
        r#"{"name":"Kitchen Table","floor":"","domain":["light"]}"#,
        Some(&schema),
        false,
    );
    let parsed: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(parsed["name"], "Kitchen Table");
    assert!(parsed.get("floor").is_none(), "empty floor survived: {out}");
}

#[test]
fn a_tool_that_takes_no_target_is_left_alone() {
    // Something like "cancel all timers" supplies no targeting field at all. It never
    // aimed at one thing, so there is nothing for the guard to protect.
    let schema = serde_json::json!({ "type": "object", "properties": {} });
    let out = coerce_args_to_schema("{}", Some(&schema), false);
    assert_eq!(out, "{}");
}

#[test]
fn a_reader_call_full_of_nulls_is_widened_not_rejected() {
    // Live, 2026-08-06: GetLiveContext{name:null} — the revert guard below
    // (written for actuators, where cleaning nulls once switched on every light
    // in the house) put the null back, and the server rejected the call:
    // "Input validation error: None is not of type 'string'". The turn then fell
    // into failure recovery and the person got no answer to "what lights are on".
    //
    // For a READ, widening is harmless by construction — the widest read is the
    // reader's whole job — so the guard yields and the nulls go.
    let schema = serde_json::json!({
        "type": "object",
        "properties": { "name": { "type": "string" } }
    });
    let widened = coerce_args_to_schema(r#"{"name":null}"#, Some(&schema), true);
    assert_eq!(widened, "{}");
    // An actuator keeps the guard: the nulls stay, and the server's rejection
    // stands in for the target the model failed to name.
    let kept = coerce_args_to_schema(r#"{"name":null}"#, Some(&schema), false);
    assert_eq!(kept, r#"{"name":null}"#);
}

#[test]
fn a_kind_filter_that_merely_repeats_another_is_dropped() {
    // Live: HassTurnOff{area:"kitchen", domain:["switch"], device_class:["switch"]}
    // — the same word twice. `Kitchen Main Light` is domain `switch` with no
    // matching device_class, so the pair excluded everything and Home Assistant
    // reported the AREA unmatched even though it exists.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "area": { "type": "string" },
            "domain": { "type": "array", "items": { "type": "string" } },
            "device_class": { "type": "array", "items": { "type": "string" } }
        }
    });
    let out = coerce_args_to_schema(
        r#"{"area":"kitchen","domain":["switch"],"device_class":["switch"]}"#,
        Some(&schema),
        false,
    );
    let parsed: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(parsed["area"], "kitchen", "the target is untouched");
    // Exactly one of the duplicated pair survives, so the call constrains the same
    // amount it always did — dropping it cannot widen the blast radius.
    let kept = ["domain", "device_class"]
        .iter()
        .filter(|k| parsed.get(**k).is_some())
        .count();
    assert_eq!(kept, 1, "one filter should remain: {out}");
}

#[test]
fn genuinely_different_kind_filters_both_survive() {
    // domain:["light"] with device_class:["outlet"] is a real narrowing, not a
    // duplication. Dropping either would change what the call can touch.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "domain": { "type": "array", "items": { "type": "string" } },
            "device_class": { "type": "array", "items": { "type": "string" } }
        }
    });
    let out = coerce_args_to_schema(
        r#"{"domain":["light"],"device_class":["outlet"]}"#,
        Some(&schema),
        false,
    );
    let parsed: Value = serde_json::from_str(&out).unwrap();
    assert!(parsed.get("domain").is_some(), "{out}");
    assert!(parsed.get("device_class").is_some(), "{out}");
}

#[test]
fn a_permitted_value_survives_the_hygiene() {
    // The check must not eat valid arguments — that would break every working call.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "device_class": {
                "type": "array",
                "items": { "enum": ["outlet", "switch"] }
            }
        }
    });
    let out = coerce_args_to_schema(r#"{"device_class":"switch"}"#, Some(&schema), false);
    let parsed: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(
        parsed["device_class"],
        serde_json::json!(["switch"]),
        "a valid value was coerced to an array and kept: {out}"
    );
}

#[test]
fn the_read_back_is_scoped_to_what_the_action_targeted() {
    // The live failure: "turn the kitchen main off" read state back with no
    // arguments, got every device in the house, and the butler answered about the
    // GARAGE — the first thing in the dump that happened to be on.
    //
    // Home Assistant's GetLiveContext accepts name/domain/area filters, and the turn
    // was passing none of them.
    let mcp = McpRunner::connect_with_readers(vec![(
        "home-assistant".to_owned(),
        Box::new(FakeTransport {
            tools: vec![
                schema_tool("HassTurnOff", &["name", "area", "domain"]),
                schema_tool("GetLiveContext", &["name", "area", "domain"]),
            ],
            healthy: true,
        }) as Box<dyn McpClient>,
        "GetLiveContext".to_owned(),
    )]);

    let scoped = mcp.read_back_input(
        "home-assistant.HassTurnOff",
        r#"{"area":"kitchen","name":"main"}"#,
    );
    let parsed: Value = serde_json::from_str(&scoped).unwrap();
    assert_eq!(parsed["area"], "kitchen");
    assert_eq!(parsed["name"], "main");
}

#[test]
fn a_kind_filter_never_narrows_the_read_back() {
    // The live case: "kitchen main" is a SWITCH, and the model sent
    // domain: ["light"]. Inheriting that would hide the one entity that explains
    // the failure — after an action that did not land, what is actually there is
    // the whole point of looking.
    let mcp = McpRunner::connect_with_readers(vec![(
        "home-assistant".to_owned(),
        Box::new(FakeTransport {
            tools: vec![
                schema_tool("HassTurnOff", &["name", "area", "domain"]),
                schema_tool("GetLiveContext", &["name", "area", "domain"]),
            ],
            healthy: true,
        }) as Box<dyn McpClient>,
        "GetLiveContext".to_owned(),
    )]);

    let scoped = mcp.read_back_input(
        "home-assistant.HassTurnOff",
        r#"{"area":"kitchen","name":"main","domain":["light"]}"#,
    );
    let parsed: Value = serde_json::from_str(&scoped).unwrap();
    assert_eq!(parsed["area"], "kitchen", "location still narrows");
    assert!(
        parsed.get("domain").is_none(),
        "the kind filter leaked into the reading: {scoped}"
    );
}

#[test]
fn a_reader_only_gets_arguments_it_understands() {
    // Schema against schema: an argument the reader does not accept must not be
    // forwarded, or the read-back fails and the action goes unverified.
    let mcp = McpRunner::connect_with_readers(vec![(
        "home-assistant".to_owned(),
        Box::new(FakeTransport {
            tools: vec![
                schema_tool("HassTurnOff", &["name", "brightness"]),
                schema_tool("GetLiveContext", &["name"]),
            ],
            healthy: true,
        }) as Box<dyn McpClient>,
        "GetLiveContext".to_owned(),
    )]);

    let scoped = mcp.read_back_input(
        "home-assistant.HassTurnOff",
        r#"{"name":"kitchen","brightness":40}"#,
    );
    let parsed: Value = serde_json::from_str(&scoped).unwrap();
    assert_eq!(parsed["name"], "kitchen");
    assert!(
        parsed.get("brightness").is_none(),
        "an argument the reader can't take was forwarded: {scoped}"
    );
}

#[test]
fn an_untargeted_action_still_reads_the_whole_state() {
    // No overlap, no scoping — the whole reading, which is the old behaviour and
    // the right fallback.
    let mcp = McpRunner::connect_with_readers(vec![(
        "home-assistant".to_owned(),
        Box::new(FakeTransport {
            tools: vec![
                schema_tool("HassTurnOff", &["name"]),
                schema_tool("GetLiveContext", &["name"]),
            ],
            healthy: true,
        }) as Box<dyn McpClient>,
        "GetLiveContext".to_owned(),
    )]);
    assert_eq!(
        mcp.read_back_input("home-assistant.HassTurnOff", "{}"),
        "{}"
    );
}

#[test]
fn any_server_gets_read_back_once_someone_nominates_its_reader() {
    // ADR 0054's whole point. Nothing here is Home Assistant, and nothing in the
    // runner knows what this server is — a calendar gets verification on exactly
    // the same terms, because the mapping is data rather than a name in the source.
    let mcp = McpRunner::connect_with_readers(vec![(
        "calendar".to_owned(),
        Box::new(FakeTransport {
            tools: vec![tool("create_event"), tool("list_events")],
            healthy: true,
        }) as Box<dyn McpClient>,
        "list_events".to_owned(),
    )]);

    assert_eq!(
        mcp.verifier("calendar.create_event").as_deref(),
        Some("calendar.list_events"),
        "an action is verified through the nominated reader"
    );
    let reader = mcp
        .available()
        .into_iter()
        .find(|s| s.id == "calendar.list_events")
        .expect("offered");
    assert_eq!(reader.reversibility, Reversibility::Observe);
    assert!(reader.autonomous);
}

#[test]
fn a_server_with_no_nomination_gets_no_reader_and_no_read_back() {
    // The honest default, unchanged: Endora does not guess which tool reads, and a
    // server's own say-so is not evidence (ADR 0051/0038). Note this holds even for
    // a tool literally named GetLiveContext — the old hardcode is gone.
    let mcp = McpRunner::connect(vec![(
        "home-assistant".to_owned(),
        Box::new(FakeTransport {
            tools: vec![tool("HassTurnOff"), tool("GetLiveContext")],
            healthy: true,
        }) as Box<dyn McpClient>,
    )]);
    assert_eq!(mcp.verifier("home-assistant.HassTurnOff"), None);
    for spec in mcp.available() {
        assert_eq!(spec.reversibility, Reversibility::Irreversible);
        assert!(!spec.autonomous);
    }
}

#[test]
fn opener_runner_lifts_block_to_confirm_only_for_opened_tools() {
    let mcp = Arc::new(McpRunner::connect(vec![(
        "fs".to_owned(),
        Box::new(FakeTransport {
            tools: vec![tool("read_file"), tool("write_file")],
            healthy: true,
        }) as Box<dyn McpClient>,
    )]));
    // The person has opened only fs.write_file, but has NOT allowed acting on its
    // own (auto_consequential = false).
    let stances: std::collections::HashMap<String, Stance> =
        [("fs.write_file".to_owned(), Stance::Ask)].into();
    let overlay = OpenerRunner::new(
        mcp,
        std::collections::HashSet::new(),
        stances,
        std::collections::HashSet::new(),
        false,
    );

    // Deny-by-default holds for the un-opened tool; the opened one becomes
    // confirm-each-use (never autonomous — its spec stays non-autonomous).
    assert_eq!(overlay.decision("fs.read_file"), Some(Decision::Block));
    assert_eq!(overlay.decision("fs.write_file"), Some(Decision::Confirm));
    assert!(overlay.available().iter().all(|s| !s.autonomous));

    // Only the opened tool may run; the blocked one is refused even on a direct
    // call. The opened tool routes through to the transport.
    assert!(overlay.run("fs.read_file", "{}").is_err());
    assert_eq!(
        overlay.run("fs.write_file", "{\"p\":1}").unwrap(),
        "write_file({\"p\":1})"
    );
}

#[test]
fn an_ask_stance_does_not_take_a_readers_autonomy() {
    // Seen live: `trust_all` opened every undecided tool on a server with `ask` —
    // including the nominated reader. Ask's graduation gate is proven-by-read-back
    // confirmed CHANGES (ADR 0062), which only an actuator can ever satisfy, so the
    // reader was left permanently non-autonomous and every chat-turn read was
    // refused with "needs their go-ahead". The model, unable to look, answered
    // about the house from imagination. A read has no work of its own to confirm;
    // asking cannot make it safer than it already is.
    let mcp = Arc::new(McpRunner::connect_with_readers(vec![(
        "home".to_owned(),
        Box::new(FakeTransport {
            tools: vec![tool("GetLiveContext"), tool("HassTurnOff")],
            healthy: true,
        }) as Box<dyn McpClient>,
        "GetLiveContext".to_owned(),
    )]));
    let stances: std::collections::HashMap<String, Stance> = [
        ("home.GetLiveContext".to_owned(), Stance::Ask),
        ("home.HassTurnOff".to_owned(), Stance::Ask),
    ]
    .into();
    let overlay = OpenerRunner::new(
        mcp,
        std::collections::HashSet::new(),
        stances,
        std::collections::HashSet::new(),
        false,
    );

    let reader = overlay
        .available()
        .into_iter()
        .find(|s| s.id == "home.GetLiveContext")
        .expect("offered");
    assert!(
        reader.autonomous,
        "an ask stance on the observe band must not strip autonomy"
    );
    assert_eq!(overlay.decision("home.GetLiveContext"), Some(Decision::Act));

    // The actuator's gate is untouched: confirm-each-use until proven.
    let actuator = overlay
        .available()
        .into_iter()
        .find(|s| s.id == "home.HassTurnOff")
        .expect("offered");
    assert!(!actuator.autonomous);
    assert_eq!(
        overlay.decision("home.HassTurnOff"),
        Some(Decision::Confirm)
    );
}

#[test]
fn off_still_silences_a_reader() {
    // `off` is the person's decision, not a default — the observe band must not
    // resurrect a reader they switched off.
    let mcp = Arc::new(McpRunner::connect_with_readers(vec![(
        "home".to_owned(),
        Box::new(FakeTransport {
            tools: vec![tool("GetLiveContext")],
            healthy: true,
        }) as Box<dyn McpClient>,
        "GetLiveContext".to_owned(),
    )]));
    let stances: std::collections::HashMap<String, Stance> =
        [("home.GetLiveContext".to_owned(), Stance::Off)].into();
    let overlay = OpenerRunner::new(
        mcp,
        std::collections::HashSet::new(),
        stances,
        std::collections::HashSet::new(),
        false,
    );
    let reader = overlay
        .available()
        .into_iter()
        .find(|s| s.id == "home.GetLiveContext")
        .expect("listed");
    assert!(!reader.autonomous);
    assert_eq!(
        overlay.decision("home.GetLiveContext"),
        Some(Decision::Block)
    );
}

#[test]
fn unattended_runner_keeps_an_opened_actuator_from_running_with_nobody_there() {
    // The exact configuration the nightly loop's safety claim has to survive: the
    // person opened a Home Assistant actuator AND widened the envelope, so in a
    // chat turn `OpenerRunner` clears it to act on its own. Unattended, it must not.
    let mcp = Arc::new(McpRunner::connect(vec![(
        "home".to_owned(),
        Box::new(FakeTransport {
            tools: vec![tool("HassTurnOff")],
            healthy: true,
        }) as Box<dyn McpClient>,
    )]));
    let stances: std::collections::HashMap<String, Stance> =
        [("home.HassTurnOff".to_owned(), Stance::Ask)].into();
    let proven: std::collections::HashSet<String> = ["home.HassTurnOff".to_owned()].into();
    let attended = Arc::new(OpenerRunner::new(
        mcp,
        std::collections::HashSet::new(),
        stances,
        proven,
        true,
    ));
    // Precondition: attended, it really is cleared to act.
    assert_eq!(attended.decision("home.HassTurnOff"), Some(Decision::Act));

    let unattended = ReversibleOnlyRunner::new(attended);
    // Unattended, the same tool falls back to needing a person.
    assert_eq!(
        unattended.decision("home.HassTurnOff"),
        Some(Decision::Confirm)
    );
    assert!(unattended.available().iter().all(|s| !s.autonomous));
    // And it is refused at the run layer too, not merely un-offered.
    assert!(unattended.run("home.HassTurnOff", "{}").is_err());
}

#[test]
fn unattended_runner_leaves_reversible_skills_alone() {
    // The nightly loop still has to be able to research: an `Observe`-band skill
    // keeps acting on its own, which is the whole point of the loop.
    let unattended = ReversibleOnlyRunner::new(Arc::new(FakeBuiltin));
    assert_eq!(unattended.decision("weather"), Some(Decision::Act));
    assert!(unattended.available().iter().all(|s| s.autonomous));
    assert_eq!(unattended.run("weather", "{}").unwrap(), "sunny");
}

#[test]
fn unattended_runner_treats_an_unknown_band_as_an_actuator() {
    // Deny-by-default, same rule ADR 0053 applies to verification: a capability
    // whose band we cannot see is assumed to change something.
    let unattended = ReversibleOnlyRunner::new(Arc::new(FakeBuiltin));
    assert!(unattended.run("mystery", "{}").is_err());
}

#[test]
fn opener_runner_lets_opened_tools_act_when_autonomy_is_widened() {
    let mcp = Arc::new(McpRunner::connect(vec![(
        "home".to_owned(),
        Box::new(FakeTransport {
            tools: vec![tool("HassTurnOn"), tool("HassTurnOff")],
            healthy: true,
        }) as Box<dyn McpClient>,
    )]));
    let stances: std::collections::HashMap<String, Stance> =
        [("home.HassTurnOn".to_owned(), Stance::Ask)].into();
    let proven: std::collections::HashSet<String> = ["home.HassTurnOn".to_owned()].into();
    // Both gates open (ADR 0062): the record has proven this tool AND the person
    // allowed acting on consequential things on its own.
    let overlay = OpenerRunner::new(mcp, std::collections::HashSet::new(), stances, proven, true);

    // The opened tool may now run in the loop (Act + autonomous); the un-opened
    // one is still blocked, and never autonomous.
    assert_eq!(overlay.decision("home.HassTurnOn"), Some(Decision::Act));
    assert_eq!(overlay.decision("home.HassTurnOff"), Some(Decision::Block));
    let specs = overlay.available();
    assert!(
        specs
            .iter()
            .find(|s| s.id == "home.HassTurnOn")
            .unwrap()
            .autonomous
    );
    assert!(
        !specs
            .iter()
            .find(|s| s.id == "home.HassTurnOff")
            .unwrap()
            .autonomous
    );
}

/// A runner offering two tools, one of which the person turned off.
fn two_tools() -> Arc<dyn CapabilityRunner + Send + Sync> {
    struct Two;
    impl CapabilityRunner for Two {
        fn available(&self) -> Vec<crate::application::CapabilitySpec> {
            ["home.HassTurnOn", "home.HassLightSet"]
                .into_iter()
                .map(|id| crate::application::CapabilitySpec {
                    id: id.to_owned(),
                    wants_place: false,
                    third_party: false,
                    description: String::new(),
                    configured: true,
                    autonomous: false,
                    reversibility: Reversibility::Reversible,
                    input_schema: None,
                })
                .collect()
        }
        fn decision(&self, _id: &str) -> Option<Decision> {
            Some(Decision::Confirm)
        }
        fn run(&self, id: &str, _input: &str) -> Result<String, String> {
            Ok(format!("ran {id}"))
        }
        fn verifier(&self, _id: &str) -> Option<String> {
            Some("home.HassLightSet".to_owned())
        }
        fn read_back_input(&self, _id: &str, _input: &str) -> String {
            "{}".to_owned()
        }
    }
    Arc::new(Two)
}

fn withdrawing(id: &str) -> WithdrawnRunner {
    WithdrawnRunner::new(two_tools(), [id.to_owned()].into_iter().collect())
}

#[test]
fn a_withdrawn_capability_is_not_on_the_menu_at_all() {
    // The measured failure is the model CHOOSING the wrong tool, so refusing it more
    // loudly does not help — it has to stop being offered.
    let runner = withdrawing("home.HassLightSet");
    let ids: Vec<String> = runner.available().into_iter().map(|s| s.id).collect();
    assert_eq!(ids, vec!["home.HassTurnOn".to_owned()]);
}

#[test]
fn a_withdrawn_capability_cannot_be_run_by_name() {
    // The model may name a tool it saw earlier in the conversation, and a direct
    // call must not route around the narrowed catalogue either.
    let err = withdrawing("home.HassLightSet")
        .run("home.HassLightSet", "{}")
        .expect_err("a withdrawn tool ran");
    assert!(err.contains("turned off"), "{err}");
    assert!(
        withdrawing("home.HassLightSet")
            .decision("home.HassLightSet")
            .is_none(),
        "a withdrawn tool still had a policy decision"
    );
}

#[test]
fn withdrawing_a_reader_leaves_actions_unverified_rather_than_broken() {
    // Read-back must not name a tool that can no longer be run: the turn would ask
    // for a reading it cannot get. Unverified is the honest fallback (ADR 0053).
    let runner = withdrawing("home.HassLightSet");
    assert_eq!(runner.verifier("home.HassTurnOn"), None);
}

#[test]
fn everything_else_is_untouched() {
    let runner = withdrawing("home.HassLightSet");
    assert_eq!(
        runner.run("home.HassTurnOn", "{}").unwrap(),
        "ran home.HassTurnOn"
    );
    assert_eq!(runner.decision("home.HassTurnOn"), Some(Decision::Confirm));
}

/// A server that behaves like the live one: it matches a name only when it is exactly
/// right, and it has a reader that reports what exists.
struct House {
    calls: std::sync::Mutex<Vec<String>>,
}

impl CapabilityRunner for House {
    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        Vec::new()
    }
    fn decision(&self, _id: &str) -> Option<Decision> {
        Some(Decision::Act)
    }
    fn verifier(&self, id: &str) -> Option<String> {
        (id != "home.GetLiveContext").then(|| "home.GetLiveContext".to_owned())
    }
    fn read_back_input(&self, _id: &str, _input: &str) -> String {
        "{}".to_owned()
    }
    fn run(&self, id: &str, input: &str) -> Result<String, String> {
        if id == "home.GetLiveContext" {
            return Ok(
                "{\"result\": \"- names: Kitchen Main Light\\n  domain: light\\n\
                       - names: Kitchen Table\\n  domain: light\\n\
                       - names: Guest Bedroom Left\\n  domain: light\\n\
                       - names: Guest Bedroom\\n  domain: light\\n\
                       - names: Garage Main\\n  domain: light\\n\"}"
                    .to_owned(),
            );
        }
        self.calls.lock().unwrap().push(input.to_owned());
        let v: Value = serde_json::from_str(input).unwrap();
        // Matches by name, exactly — like the real thing. An entity name in the AREA
        // field is refused, which is the live INVALID_AREA failure.
        let named = v.get("name").and_then(Value::as_str).unwrap_or_default();
        if ["Kitchen Table", "Guest Bedroom Left"].contains(&named) {
            return Ok(format!("turned on {named}"));
        }
        Err("MatchFailedError no_match_reason=NAME".to_owned())
    }
}

fn house() -> (Arc<House>, TargetSearchRunner) {
    let inner = Arc::new(House {
        calls: std::sync::Mutex::new(Vec::new()),
    });
    let runner =
        TargetSearchRunner::new(Arc::clone(&inner) as Arc<dyn CapabilityRunner + Send + Sync>);
    (inner, runner)
}

#[test]
fn it_finds_the_name_the_model_spent_fourteen_attempts_failing_to_guess() {
    // The live failure, end to end: the model asks for "table" in the kitchen, the
    // server refuses, and the name it needed was in the reading all along.
    let (inner, runner) = house();
    let out = runner
        .run(
            "home.HassTurnOn",
            r#"{"name":"table","area":"kitchen","domain":["light"]}"#,
        )
        .expect("the search did not recover the call");
    assert!(out.contains("turned on Kitchen Table"), "{out}");
    assert!(
        out.contains("Kitchen Table"),
        "the substitution is disclosed: {out}"
    );
    let calls = inner.calls.lock().unwrap();
    assert!(
        calls.iter().any(|c| c.contains("Kitchen Table")),
        "never retried with the real name: {calls:?}"
    );
}

#[test]
fn a_working_call_is_never_touched() {
    // Recovery only. It must not be able to hijack a call that was already right.
    let (inner, runner) = house();
    let out = runner
        .run("home.HassTurnOn", r#"{"name":"Kitchen Table"}"#)
        .unwrap();
    assert_eq!(out, "turned on Kitchen Table", "rewrote a working call");
    assert_eq!(
        inner.calls.lock().unwrap().len(),
        1,
        "retried unnecessarily"
    );
}

#[test]
fn an_ambiguous_search_shows_the_names_and_acts_on_none_of_them() {
    // "kitchen" resembles two lights. Picking one would be a coin flip that actuates
    // something, so it hands over the shortlist and stops.
    let (inner, runner) = house();
    let err = runner
        .run("home.HassTurnOn", r#"{"area":"kitchen"}"#)
        .expect_err("acted on an ambiguous match");
    assert!(err.contains("Kitchen Table"), "{err}");
    assert!(err.contains("Kitchen Main Light"), "{err}");
    assert!(err.contains("EXACTLY"), "does not say to copy it: {err}");
    assert_eq!(
        inner.calls.lock().unwrap().len(),
        1,
        "retried on a guess: {:?}",
        inner.calls.lock().unwrap()
    );
}

#[test]
fn nothing_resembling_it_leaves_the_original_failure_alone() {
    let (_, runner) = house();
    let err = runner
        .run("home.HassTurnOn", r#"{"name":"greenhouse"}"#)
        .expect_err("invented a match");
    assert!(err.contains("MatchFailedError"), "{err}");
    assert!(
        !err.contains("[candidates]"),
        "offered nothing as something: {err}"
    );
}

#[test]
fn a_server_with_no_nominated_reader_searches_nothing() {
    // No reader means no reading, which means no candidates — the same honest silence
    // ADR 0054 chose, and the reason this needs no per-server code.
    struct Blind;
    impl CapabilityRunner for Blind {
        fn available(&self) -> Vec<crate::application::CapabilitySpec> {
            Vec::new()
        }
        fn decision(&self, _id: &str) -> Option<Decision> {
            Some(Decision::Act)
        }
        fn verifier(&self, _id: &str) -> Option<String> {
            None
        }
        fn read_back_input(&self, _id: &str, _input: &str) -> String {
            "{}".to_owned()
        }
        fn run(&self, _id: &str, _input: &str) -> Result<String, String> {
            Err("nope".to_owned())
        }
    }
    let runner = TargetSearchRunner::new(Arc::new(Blind));
    assert_eq!(
        runner.run("x.Y", r#"{"name":"table"}"#).unwrap_err(),
        "nope"
    );
}

#[test]
fn the_retry_is_bounded_however_many_fields_the_call_carried() {
    let (inner, runner) = house();
    let _ = runner.run(
        "home.HassTurnOn",
        r#"{"name":"table","area":"kitchen","floor":"kitchen table","zone":"table"}"#,
    );
    assert!(
        inner.calls.lock().unwrap().len() <= 1 + MAX_PLACEMENTS,
        "unbounded retrying: {:?}",
        inner.calls.lock().unwrap()
    );
}

#[test]
fn it_moves_a_real_name_into_a_field_that_can_hold_it() {
    // Live, and the search found the answer then had nowhere to put it:
    //   {area: "guest bedroom left", name: "lamp"} -> INVALID_AREA
    // `Guest Bedroom Left` is an entity, not an area. The field already held the
    // whole name and the name field held "lamp", so restricting placements to
    // fragment-holders left no placement at all.
    let (inner, runner) = house();
    let out = runner
        .run(
            "home.HassTurnOn",
            r#"{"area":"guest bedroom left","name":"lamp","domain":["light"]}"#,
        )
        .expect("found the name and could not use it");
    assert!(out.contains("turned on Guest Bedroom Left"), "{out}");
    let calls = inner.calls.lock().unwrap();
    assert!(
        calls
            .iter()
            .any(|c| c.contains(r#""name":"Guest Bedroom Left""#)),
        "never tried the name in a field that could hold it: {calls:?}"
    );
}

/// A service with its own interface: things have ids, and acting by id cannot miss.
struct Direct {
    acted: std::sync::Mutex<Vec<String>>,
}

impl NativeChannel for Direct {
    fn known(&self) -> Result<Vec<(String, String)>, String> {
        Ok(vec![
            ("light.kitchen_table".to_owned(), "Kitchen Table".to_owned()),
            (
                "switch.kitchen_main".to_owned(),
                "Kitchen Main Light".to_owned(),
            ),
            (
                "light.hidden_from_assist".to_owned(),
                "Pantry Strip".to_owned(),
            ),
        ])
    }
    fn reading(&self) -> Result<String, String> {
        // Ids are deliberately absent: `light.kitchen_table` shares its words with
        // `Kitchen Table` and would compete with it as a candidate.
        Ok("names: Kitchen Table\n\
            names: Kitchen Main Light\n\
            names: Kitchen Main Light LED\n\
            names: Kitchen Main Light Cloud connection\n\
            names: Pantry Strip"
            .to_owned())
    }
    fn act(&self, tool: &str, id: &str) -> Option<Result<String, String>> {
        if !tool.ends_with("HassTurnOn") {
            return None;
        }
        self.acted.lock().unwrap().push(id.to_owned());
        Some(Ok(format!("called turn_on on {id}")))
    }

    fn about_the_person(&self) -> Option<String> {
        Some("john is not home".to_owned())
    }

    fn states(&self) -> Result<Vec<(String, String)>, String> {
        Ok(vec![
            ("Kitchen Table".to_owned(), "off".to_owned()),
            ("Kitchen Main Light".to_owned(), "on".to_owned()),
        ])
    }

    fn actionable(&self, _tool: &str, id: &str) -> bool {
        id.starts_with("light.") || id.starts_with("switch.")
    }
}

fn with_direct() -> (Arc<Direct>, TargetSearchRunner) {
    let direct = Arc::new(Direct {
        acted: std::sync::Mutex::new(Vec::new()),
    });
    // Wrapping the server itself, not the plain search runner — nesting two of them
    // would let the inner one recover first, and the outer would never see a failure.
    let server = Arc::new(House {
        calls: std::sync::Mutex::new(Vec::new()),
    });
    let runner = TargetSearchRunner::with_channels(
        server as Arc<dyn CapabilityRunner + Send + Sync>,
        vec![(
            "home".to_owned(),
            Arc::clone(&direct) as Arc<dyn NativeChannel>,
        )],
    );
    (direct, runner)
}

#[test]
fn direct_reach_acts_by_id_rather_than_retrying_a_name() {
    // The end of the guessing. The model asked for "table"; the service's own
    // interface has `light.kitchen_table`, which cannot be mis-matched.
    let (direct, runner) = with_direct();
    let out = runner
        .run("home.HassTurnOn", r#"{"name":"table","area":"kitchen"}"#)
        .expect("direct reach did not act");
    assert!(out.contains("light.kitchen_table"), "{out}");
    assert_eq!(
        *direct.acted.lock().unwrap(),
        vec!["light.kitchen_table".to_owned()]
    );
}

#[test]
fn it_can_find_what_the_tool_surface_never_exposed() {
    // The reason direct reach is worth having beyond exactness: `Pantry Strip` is not
    // in the tool surface's reading at all, so nothing Endora did before could find
    // it however hard it searched.
    let (direct, runner) = with_direct();
    let out = runner
        .run("home.HassTurnOn", r#"{"name":"pantry strip"}"#)
        .expect("could not reach something hidden from the tool surface");
    assert!(out.contains("light.hidden_from_assist"), "{out}");
    assert_eq!(direct.acted.lock().unwrap().len(), 1);
}

#[test]
fn a_tool_the_channel_cannot_express_falls_back_to_the_retry() {
    // Direct reach is an improvement, never a new way to fail: the fake channel only
    // expresses turning on, so turning off goes back through the tool.
    let (direct, runner) = with_direct();
    let out = runner.run("home.HassTurnOff", r#"{"name":"table","area":"kitchen"}"#);
    assert!(direct.acted.lock().unwrap().is_empty(), "acted anyway");
    assert!(out.is_err() || out.unwrap().contains("Kitchen Table"));
}

#[test]
fn direct_reach_still_refuses_an_ambiguous_match() {
    // Exactness does not lower the bar for acting. "kitchen" resembles two things,
    // and having ids available does not make choosing between them safe.
    let (direct, runner) = with_direct();
    let err = runner
        .run("home.HassTurnOn", r#"{"area":"kitchen"}"#)
        .expect_err("acted on an ambiguous match");
    assert!(err.contains("Kitchen Table"), "{err}");
    assert!(direct.acted.lock().unwrap().is_empty(), "acted on a guess");
}

#[test]
fn a_confirmed_name_drops_the_models_other_guesses() {
    // Live, with the alias already in place and still failing:
    //   {name:"table light", area:"Living Room", floor:"1"} -> INVALID_FLOOR
    // A kitchen light placed on an invented floor in the wrong room. The server
    // rejected the call on the floor and never looked at the name, so substituting
    // it changed nothing.
    struct Picky;
    impl CapabilityRunner for Picky {
        fn available(&self) -> Vec<crate::application::CapabilitySpec> {
            Vec::new()
        }
        fn decision(&self, _id: &str) -> Option<Decision> {
            Some(Decision::Act)
        }
        fn verifier(&self, _id: &str) -> Option<String> {
            None
        }
        fn read_back_input(&self, _id: &str, _input: &str) -> String {
            "{}".to_owned()
        }
        fn run(&self, _id: &str, input: &str) -> Result<String, String> {
            let v: Value = serde_json::from_str(input).unwrap();
            if v.get("floor").is_some() {
                return Err("INVALID_FLOOR".to_owned());
            }
            if v.get("name").and_then(Value::as_str) == Some("Kitchen Table") {
                return Ok("turned off Kitchen Table".to_owned());
            }
            Err("no match".to_owned())
        }
    }
    let runner = AliasRunner::new(
        Arc::new(Picky),
        vec![(
            "home".to_owned(),
            "table light".to_owned(),
            "Kitchen Table".to_owned(),
        )],
    );
    let out = runner
        .run(
            "home.HassTurnOff",
            r#"{"name":"table light","area":"Living Room","floor":"1","domain":["light"]}"#,
        )
        .expect("the confirmed name never got a chance");
    assert!(out.contains("turned off Kitchen Table"), "{out}");
}

#[test]
fn a_confirmed_name_still_never_widens_the_call() {
    // Kind filters restrict which sorts of thing count rather than claiming which one
    // it is. Dropping them would widen the call, which is how one action became every
    // light in the house.
    let runner = AliasRunner::new(
        Arc::new(House {
            calls: std::sync::Mutex::new(Vec::new()),
        }),
        vec![(
            "home".to_owned(),
            "table".to_owned(),
            "Kitchen Table".to_owned(),
        )],
    );
    let applied = runner
        .apply(
            "home.HassTurnOn",
            r#"{"name":"table","area":"kitchen","domain":["light"]}"#,
        )
        .expect("the alias did not apply");
    let v: Value = serde_json::from_str(&applied).unwrap();
    assert_eq!(v["name"], "Kitchen Table");
    assert_eq!(
        v["domain"],
        serde_json::json!(["light"]),
        "widened: {applied}"
    );
    assert!(
        v.get("area").is_none(),
        "kept a guess about the same target: {applied}"
    );
}

#[test]
fn a_thing_and_its_own_diagnostics_are_not_a_real_ambiguity() {
    // Live: "turn off the kitchen main light" tied three ways — the light, its LED
    // configuration entry, and its cloud-connection indicator. All three carry the
    // device's name; two of them are not controls at all. Reading that as ambiguity
    // meant refusing a request with exactly one sensible answer.
    let (direct, runner) = with_direct();
    let out = runner
        .run("home.HassTurnOn", r#"{"name":"kitchen main light"}"#)
        .expect("refused a request with one operable answer");
    assert!(out.contains("switch.kitchen_main"), "{out}");
    assert_eq!(
        *direct.acted.lock().unwrap(),
        vec!["switch.kitchen_main".to_owned()],
        "acted on a diagnostic"
    );
}

#[test]
fn a_tie_between_two_real_things_still_acts_on_neither() {
    // Narrowing a tie must not become a way to break one. Two operable candidates is
    // the ambiguity this whole path exists to refuse.
    let (direct, runner) = with_direct();
    let err = runner
        .run("home.HassTurnOn", r#"{"area":"kitchen"}"#)
        .expect_err("acted on a genuine ambiguity");
    assert!(err.contains("Kitchen Table"), "{err}");
    assert!(direct.acted.lock().unwrap().is_empty(), "acted on a guess");
}

#[test]
fn direct_reach_verifies_against_the_whole_service() {
    // Live: a call aimed at "living room" was corrected to `light.kitchen_table` and
    // acted on it, while the read-back kept looking at the living room. Both readings
    // matched, so a light the person watched come on was recorded as no change.
    let (_, runner) = with_direct();
    assert_eq!(
        runner.read_back_input("home.HassTurnOn", r#"{"area":"living room"}"#),
        "{}",
        "verification still aimed where the model pointed"
    );
}

#[test]
fn without_direct_reach_the_scoped_read_back_is_untouched() {
    let (_, runner) = house();
    assert_eq!(
        runner.read_back_input("home.HassTurnOn", r#"{"area":"kitchen"}"#),
        "{}",
        "the inner runner decides when there is no channel"
    );
}

#[test]
fn what_a_service_knows_about_the_person_reaches_the_turn() {
    // The house was already reporting `person.john -> not_home` in a reading Endora
    // fetches for other reasons, and nothing ever looked at it. A butler that does not
    // know whether anyone is in is guessing every time it decides whether to speak.
    let (_, runner) = with_direct();
    assert_eq!(
        runner.about_the_person(),
        vec!["john is not home".to_owned()]
    );
}

#[test]
fn a_service_with_nothing_to_say_about_them_says_nothing() {
    let (_, plain) = house();
    assert!(plain.about_the_person().is_empty());
}

#[test]
fn an_enum_nested_in_any_of_is_still_a_list_of_permitted_values() {
    // The shape that got through. A generated schema usually says "a string from this
    // list, or null" as an anyOf, and looking only at the top two levels missed it —
    // so `device_class: ["light"]` was sent, and Home Assistant answered
    // `'light' is not one of ['awning', 'blind', ...]`, failing the whole call.
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "device_class": {
                "anyOf": [
                    { "type": "array", "items": { "enum": ["door", "garage"] } },
                    { "type": "null" }
                ]
            }
        }
    });
    let out = coerce_args_to_schema(
        r#"{"device_class":["light"],"name":"front"}"#,
        Some(&schema),
        false,
    );
    let v: Value = serde_json::from_str(&out).unwrap();
    assert!(
        v.get("device_class").is_none(),
        "sent a value the schema rejects: {out}"
    );
    assert_eq!(v["name"], "front", "dropped something valid: {out}");
}

#[test]
fn a_permitted_value_nested_the_same_way_survives() {
    let schema = serde_json::json!({
        "type": "object",
        "properties": {
            "device_class": {
                "anyOf": [
                    { "type": "array", "items": { "enum": ["door", "garage"] } },
                    { "type": "null" }
                ]
            }
        }
    });
    let out = coerce_args_to_schema(r#"{"device_class":["garage"]}"#, Some(&schema), false);
    let v: Value = serde_json::from_str(&out).unwrap();
    assert_eq!(v["device_class"], serde_json::json!(["garage"]));
}

/// A runner at the bottom of the stack that records which port methods reach it.
///
/// The backstop for [`forwards_to_inner!`]. The macro stops a decorator forgetting to
/// pass a method along; this proves the whole layered chain is actually transparent,
/// including the runners that aggregate rather than wrap and so cannot use the macro.
#[derive(Default)]
struct RecordsWhatReachesIt {
    reached: std::sync::Mutex<std::collections::BTreeSet<&'static str>>,
}

impl RecordsWhatReachesIt {
    fn note(&self, method: &'static str) {
        self.reached.lock().unwrap().insert(method);
    }
}

/// The one skill the recorder claims. `CompositeRunner` routes by **which child offers
/// the id**, so a recorder that offers nothing is never asked anything — the first run
/// of this test failed that way and the setup was at fault, not the code.
const A_SKILL_IT_OFFERS: &str = "recorder.thing";

impl CapabilityRunner for RecordsWhatReachesIt {
    fn available(&self) -> Vec<crate::application::CapabilitySpec> {
        vec![crate::application::CapabilitySpec {
            id: A_SKILL_IT_OFFERS.to_owned(),
            wants_place: false,
            third_party: false,
            description: String::new(),
            configured: true,
            autonomous: true,
            reversibility: Reversibility::Observe,
            input_schema: None,
        }]
    }

    fn run(&self, _id: &str, _input_json: &str) -> Result<String, String> {
        Ok(String::new())
    }

    fn about_the_person(&self) -> Vec<String> {
        self.note("about_the_person");
        Vec::new()
    }

    fn current_states(&self) -> Vec<(String, String)> {
        self.note("current_states");
        Vec::new()
    }

    fn decision(&self, _id: &str) -> Option<Decision> {
        self.note("decision");
        Some(Decision::Act)
    }

    fn verifier(&self, _id: &str) -> Option<String> {
        self.note("verifier");
        None
    }

    fn read_back_input(&self, _action_id: &str, _action_input: &str) -> String {
        self.note("read_back_input");
        String::new()
    }
}

/// Every method of the port that has a **default**, and so can be dropped silently.
///
/// `available` and `run` are deliberately absent: they are required methods, so the
/// compiler already refuses to let a decorator forget them. These five are the whole
/// risk surface, and both real occurrences of this bug were among them.
const CAN_BE_DROPPED_SILENTLY: [&str; 5] = [
    "about_the_person",
    "current_states",
    "decision",
    "read_back_input",
    "verifier",
];

#[test]
fn every_defaultable_method_survives_the_wrapper_chain() {
    // The mechanism version of the test below. That one names two methods, which means
    // it only ever catches the two bugs that already happened; this one asserts the
    // chain is transparent for the whole risk surface, so a sixth defaultable method
    // added to the port is covered the moment it is listed in one place.
    let bottom = Arc::new(RecordsWhatReachesIt::default());
    let chain: Arc<dyn CapabilityRunner + Send + Sync> =
        Arc::new(ReversibleOnlyRunner::new(Arc::new(WithdrawnRunner::new(
            Arc::new(CompositeRunner::new(vec![Arc::new(AliasRunner::new(
                Arc::new(OpenerRunner::new(
                    Arc::clone(&bottom) as Arc<dyn CapabilityRunner + Send + Sync>,
                    std::collections::HashSet::new(),
                    std::collections::HashMap::new(),
                    std::collections::HashSet::new(),
                    false,
                )),
                Vec::new(),
            ))
                as Arc<dyn CapabilityRunner + Send + Sync>]))
                as Arc<dyn CapabilityRunner + Send + Sync>,
            std::collections::HashSet::new(),
        ))));

    // Call each one at the TOP, the only place production ever calls them.
    let _ = chain.about_the_person();
    let _ = chain.current_states();
    let _ = chain.decision(A_SKILL_IT_OFFERS);
    let _ = chain.verifier(A_SKILL_IT_OFFERS);
    let _ = chain.read_back_input(A_SKILL_IT_OFFERS, "{}");

    let reached = bottom.reached.lock().unwrap().clone();
    let missing: Vec<&str> = CAN_BE_DROPPED_SILENTLY
        .iter()
        .copied()
        .filter(|m| !reached.contains(m))
        .collect();
    assert!(
        missing.is_empty(),
        "these never reached the bottom of the stack, so a service answering them \
         would be silently ignored: {missing:?}"
    );
}

#[test]
fn what_the_services_know_survives_the_whole_stack() {
    // The bug this exists for: `about_the_person` and `current_states` are answered by
    // the channel runner in the middle of the stack, and every wrapper above it has to
    // pass them through. Two did not — so presence never reached a turn and neither
    // did the facts behind an answer, while the unit tests passed because they poked
    // the inner runner directly.
    //
    // A defaulted port method that returns nothing is indistinguishable from a service
    // having nothing to say, which is what made it silent. This asserts through the
    // composed stack, the way production builds it.
    let (_, inner) = with_direct();
    let composed = ReversibleOnlyRunner::new(Arc::new(WithdrawnRunner::new(
        Arc::new(CompositeRunner::new(vec![
            Arc::new(inner) as Arc<dyn CapabilityRunner + Send + Sync>
        ])) as Arc<dyn CapabilityRunner + Send + Sync>,
        std::collections::HashSet::new(),
    )));
    assert_eq!(
        composed.about_the_person(),
        vec!["john is not home".to_owned()],
        "presence was dropped somewhere in the stack"
    );
    assert!(
        composed
            .current_states()
            .iter()
            .any(|(name, _)| name == "Kitchen Table"),
        "the facts behind an answer were dropped somewhere in the stack"
    );
}
