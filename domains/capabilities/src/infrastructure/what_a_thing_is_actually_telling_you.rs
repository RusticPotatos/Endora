use super::facts_worth_reading;
use serde_json::json;

#[test]
fn a_calendars_meaning_is_entirely_in_its_attributes() {
    // Captured from a live house. Connecting a calendar achieved nothing until this
    // existed: the state is `off`, and the evening is in the attributes.
    let family = json!({
        "message": "Jane Doe & John Doe",
        "all_day": false,
        "start_time": "2026-07-31 18:30:00",
        "end_time": "2026-07-31 19:30:00",
        "location": "",
        "description": "-::~:~::~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~:~::~:~::-",
        "friendly_name": "Family",
        "supported_features": 1
    });
    let facts = facts_worth_reading(&family);
    assert_eq!(facts["message"], "Jane Doe & John Doe");
    assert_eq!(facts["start_time"], "2026-07-31 18:30:00");
    // An empty location is not a fact about the evening.
    assert!(!facts.contains_key("location"));
    // Boilerplate the service attaches to every reading.
    assert!(
        !facts.contains_key("description"),
        "500 characters of tildes"
    );
    assert!(!facts.contains_key("friendly_name"), "already the name");
    assert!(!facts.contains_key("supported_features"), "a bitmask");
}

#[test]
fn the_weather_is_a_temperature_not_a_word() {
    // `clear-night` is the whole state. The temperature is an attribute, which is why
    // a butler with a working weather entity still could not say how warm it was.
    let forecast = json!({
        "temperature": 73, "humidity": 72, "temperature_unit": "°F",
        "attribution": "Weather forecast from met.no, delivered by the Norwegian Meteorological Institute",
        "friendly_name": "Forecast Home", "supported_features": 3
    });
    let facts = facts_worth_reading(&forecast);
    assert_eq!(facts["temperature"], 73);
    assert_eq!(facts["temperature_unit"], "°F");
    assert!(!facts.contains_key("attribution"), "boilerplate, and long");
}

#[test]
fn a_list_is_never_a_fact_about_the_thing() {
    // An Apple TV's `source_list` is forty app names. Including it would put a page of
    // noise beside every media player, on every turn that reads the house.
    let apple_tv = json!({
        "source_list": ["App Store", "Arcade", "Channels", "Computers", "Crunchyroll"],
        "friendly_name": "Apple TV", "supported_features": 450_487
    });
    assert!(
        facts_worth_reading(&apple_tv).is_empty(),
        "nothing to say while it is off"
    );
}

#[test]
fn nothing_discursive_gets_in_even_bounded() {
    // Dropped rather than truncated: sixty characters of a paragraph is still sixty
    // characters of nothing, and it costs the same as sixty useful ones.
    let wordy = json!({ "note": "x".repeat(400) });
    assert!(facts_worth_reading(&wordy).is_empty());
}
