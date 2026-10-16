use endora_terminal::telemetry::{
    Battery, History, Sample, battery_text, estimate_minutes_left, parse_loadavg, parse_meminfo,
    read_battery, report,
};

fn sample(t: u64, stage: &str, bat: Option<u8>, charging: Option<bool>) -> Sample {
    Sample {
        t,
        stage: stage.into(),
        mode: "travel".into(),
        page: 0,
        load1: 0.1,
        mem: 40,
        temp: Some(45.0),
        undervolt: Some(false),
        bat,
        charging,
    }
}

fn scratch(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("endora-telemetry-{name}-{}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir
}

#[test]
fn system_readings_parse_and_bad_input_is_none() {
    assert_eq!(parse_loadavg("0.52 0.40 0.30 1/200 999\n"), Some(0.52));
    assert_eq!(parse_loadavg("garbage"), None);
    assert_eq!(
        parse_meminfo("MemTotal: 1000 kB\nMemFree: 1 kB\nMemAvailable: 250 kB\n"),
        Some(75)
    );
    assert_eq!(parse_meminfo("MemTotal: 0 kB\nMemAvailable: 0 kB\n"), None);
}

#[test]
fn a_power_supply_battery_is_read_when_the_kernel_exposes_one() {
    let dir = scratch("supply");
    assert_eq!(
        read_battery(&dir),
        None,
        "no battery directory means no reading"
    );
    let ac = dir.join("AC");
    std::fs::create_dir_all(&ac).unwrap();
    std::fs::write(ac.join("type"), "Mains\n").unwrap();
    assert_eq!(read_battery(&dir), None, "a mains supply is not a battery");
    let bat = dir.join("BAT0");
    std::fs::create_dir_all(&bat).unwrap();
    std::fs::write(bat.join("type"), "Battery\n").unwrap();
    std::fs::write(bat.join("capacity"), "78\n").unwrap();
    std::fs::write(bat.join("status"), "Discharging\n").unwrap();
    assert_eq!(
        read_battery(&dir),
        Some(Battery {
            pct: 78,
            charging: false
        })
    );
    std::fs::write(bat.join("status"), "Charging\n").unwrap();
    assert!(read_battery(&dir).unwrap().charging);
    std::fs::write(bat.join("capacity"), "250\n").unwrap();
    assert_eq!(
        read_battery(&dir),
        None,
        "out-of-range capacity is rejected"
    );
}

#[test]
fn time_left_comes_from_the_recent_drain_rate_and_is_withheld_when_unsound() {
    // 10 percentage points over 2 hours on battery: 5 %/h, so 50 % left lasts 10 hours.
    let now = 10_000;
    let samples: Vec<_> = (0..=8)
        .map(|i| {
            sample(
                now - 7200 + i * 900,
                "awake",
                Some(60 - (i as u8) * 10 / 8),
                Some(false),
            )
        })
        .collect();
    assert_eq!(estimate_minutes_left(&samples, now), Some(600));
    // Charging, too little history, or no drop means no estimate.
    let mut charging = samples.clone();
    charging.last_mut().unwrap().charging = Some(true);
    assert_eq!(estimate_minutes_left(&charging, now), None);
    assert_eq!(estimate_minutes_left(&samples[..2], now), None);
    let flat: Vec<_> = (0..=8)
        .map(|i| sample(now - 7200 + i * 900, "awake", Some(60), Some(false)))
        .collect();
    assert_eq!(estimate_minutes_left(&flat, now), None);
    assert_eq!(estimate_minutes_left(&[], now), None);
}

#[test]
fn battery_text_never_invents_a_number() {
    assert_eq!(battery_text(None, None), "", "no reading: nothing is shown");
    assert_eq!(
        battery_text(
            Some(Battery {
                pct: 78,
                charging: true
            }),
            Some(600)
        ),
        "Bat 78% charging"
    );
    assert_eq!(
        battery_text(
            Some(Battery {
                pct: 78,
                charging: false
            }),
            Some(310)
        ),
        "Bat 78% ~5h10m"
    );
    assert_eq!(
        battery_text(
            Some(Battery {
                pct: 78,
                charging: false
            }),
            None
        ),
        "Bat 78%"
    );
    assert_eq!(
        battery_text(
            Some(Battery {
                pct: 5,
                charging: false
            }),
            Some(25)
        ),
        "Bat 5% ~25m"
    );
}

#[test]
fn history_is_private_bounded_and_survives_reload() {
    let dir = scratch("history");
    let history = History::new(dir.join("power-history.jsonl"), 600);
    assert!(history.load().is_empty());
    for i in 0..20 {
        history.append(&sample(i, "awake", None, None)).unwrap();
    }
    let loaded = history.load();
    assert!(
        !loaded.is_empty() && loaded.len() < 20,
        "rotation keeps the file bounded: {}",
        loaded.len()
    );
    assert_eq!(loaded.last().unwrap().t, 19);
    use std::os::unix::fs::PermissionsExt;
    assert_eq!(
        std::fs::metadata(dir.join("power-history.jsonl"))
            .unwrap()
            .permissions()
            .mode()
            & 0o777,
        0o600
    );
    // A damaged line is skipped, not fatal.
    std::fs::OpenOptions::new()
        .append(true)
        .open(dir.join("power-history.jsonl"))
        .unwrap();
    std::fs::write(dir.join("power-history.jsonl"), "not json\n").unwrap();
    assert!(history.load().is_empty());
    // A symlinked history path is refused.
    let target = dir.join("elsewhere");
    std::fs::write(&target, "").unwrap();
    let link = dir.join("link.jsonl");
    std::os::unix::fs::symlink(&target, &link).unwrap();
    assert!(
        History::new(link, 600)
            .append(&sample(1, "awake", None, None))
            .is_err()
    );
}

#[test]
fn the_report_attributes_activity_by_screen_state_and_says_what_is_missing() {
    let mut samples = Vec::new();
    for i in 0..6 {
        samples.push(sample(i * 300, "awake", Some(80 - i as u8), Some(false)));
    }
    for i in 6..12 {
        let mut s = sample(i * 300, "pocket", Some(80 - i as u8 / 2), Some(false));
        s.load1 = 0.9;
        samples.push(s);
    }
    samples[3].undervolt = Some(true);
    let text = report(&samples);
    assert!(text.contains("awake") && text.contains("pocket"), "{text}");
    assert!(text.contains("%/h"), "{text}");
    assert!(text.contains("undervoltage"), "{text}");
    assert!(text.contains("busy while the screen was off"), "{text}");
    let none = report(&[sample(0, "awake", None, None)]);
    assert!(none.contains("No battery sensor"), "{none}");
    assert!(report(&[]).contains("No history"));
}
