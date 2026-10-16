use endora_terminal::font::{MAX, MIN, font_for, label, parse_level, step};

#[test]
fn text_size_steps_stay_within_the_available_sizes() {
    assert_eq!(step(0, 1), 1);
    assert_eq!(step(0, -1), -1);
    assert_eq!(step(MAX, 1), MAX, "cannot grow past the largest size");
    assert_eq!(step(MIN, -1), MIN, "cannot shrink past the smallest size");
}

#[test]
fn every_size_names_a_font_installed_on_the_handheld() {
    use endora_terminal::font::QR_FONT;
    // These files exist in /usr/share/consolefonts on Raspberry Pi OS (checked on the device).
    // The console's boot font is built into the kernel and cannot be reloaded from a file, so
    // Normal is an ordinary 8x16 font file.
    assert_eq!(font_for(-1), "Lat15-Terminus12x6");
    assert_eq!(font_for(0), "Uni2-VGA16");
    assert_eq!(font_for(1), "Lat15-TerminusBold20x10");
    assert_eq!(font_for(2), "Lat15-TerminusBold24x12");
    assert_eq!(font_for(3), "Lat15-TerminusBold28x14");
    for level in MIN..=MAX {
        assert!(
            font_for(level)
                .bytes()
                .all(|b| b.is_ascii_alphanumeric() || b == b'-'),
            "a plain name: it is passed to setfont"
        );
    }
    // QR codes need the half-block characters; this 8x14 font has them and fits a code.
    assert_eq!(QR_FONT, "Uni2-VGA14");
}

#[test]
fn a_saved_size_is_read_back_safely() {
    assert_eq!(parse_level("2\n"), 2);
    assert_eq!(parse_level("-1"), -1);
    assert_eq!(parse_level("99"), MAX, "too large is clamped");
    assert_eq!(parse_level("-99"), MIN);
    assert_eq!(parse_level(""), 0);
    assert_eq!(parse_level("big"), 0);
}

#[test]
fn every_size_has_a_plain_label() {
    for level in MIN..=MAX {
        assert!(!label(level).is_empty());
    }
    assert_eq!(label(0), "Normal");
}
