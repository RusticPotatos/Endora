//! endora-sound (ADR 0104): the sound hub. It holds the patchbay (sources, speakers and the
//! wires between them) in its own state file and answers only those holding its token: the
//! Endora node, which is its remote. Its player mixes every group in real time and streams
//! to the phones listening; the AirPlay senders come next.

use std::sync::Arc;

mod api;
mod listen;
mod player;
mod senders;

use endora_sound::application::hub::Hub;
use endora_sound::infrastructure::{FileStore, Places};

/// The shortest token the hub accepts. Anything shorter is refused at start, so a node and
/// a hub can never be set up with a guessable one.
const SHORTEST_TOKEN: usize = 32;

/// The token the node signs with: `SOUND_TOKEN` if set, else the one in `SOUND_TOKEN_FILE`,
/// made there (random, readable by its owner only) the first time. The node reads the same
/// file through a volume only the two of them mount. The same scheme as HQ's.
fn the_token(env: Option<String>, file: Option<String>) -> Result<String, String> {
    if let Some(token) = env.map(|t| t.trim().to_owned()).filter(|t| !t.is_empty()) {
        if token.len() < SHORTEST_TOKEN {
            return Err(format!(
                "SOUND_TOKEN is shorter than {SHORTEST_TOKEN} characters"
            ));
        }
        return Ok(token);
    }
    let Some(file) = file.filter(|f| !f.trim().is_empty()) else {
        return Err("neither SOUND_TOKEN nor SOUND_TOKEN_FILE is set".to_owned());
    };
    if let Ok(existing) = std::fs::read_to_string(&file) {
        let existing = existing.trim().to_owned();
        if existing.len() >= SHORTEST_TOKEN {
            return Ok(existing);
        }
    }
    let mut bytes = [0u8; 24];
    std::io::Read::read_exact(
        &mut std::fs::File::open("/dev/urandom").map_err(|e| format!("no randomness: {e}"))?,
        &mut bytes,
    )
    .map_err(|e| format!("no randomness: {e}"))?;
    let token: String = bytes.iter().map(|b| format!("{b:02x}")).collect();
    {
        use std::io::Write as _;
        use std::os::unix::fs::OpenOptionsExt as _;
        let mut out = std::fs::OpenOptions::new()
            .create(true)
            .write(true)
            .truncate(true)
            .mode(0o600)
            .open(&file)
            .map_err(|e| format!("cannot write {file}: {e}"))?;
        out.write_all(token.as_bytes())
            .map_err(|e| format!("cannot write {file}: {e}"))?;
    }
    Ok(token)
}

/// Makes the Spotify source if there is none. Whether it made one.
fn spotify_source(hub: &Hub<FileStore>) -> Result<bool, endora_sound::application::hub::HubError> {
    hub.change(|bay| {
        if bay
            .sources()
            .iter()
            .any(|s| s.kind == endora_sound::domain::SourceKind::Spotify)
        {
            return Ok(false);
        }
        bay.add_source("Spotify", endora_sound::domain::SourceKind::Spotify, None)
            .map(|_| true)
    })
}

/// Settles the AirPlay doors at start: the one general door made if it is missing, and any
/// old per-device door merged into it. Says how many device doors it merged and whether it
/// made the general one.
fn settle_doors(
    hub: &Hub<FileStore>,
) -> Result<(usize, bool), endora_sound::application::hub::HubError> {
    hub.change(|bay| {
        let had = bay
            .sources()
            .iter()
            .any(|s| s.kind == endora_sound::domain::SourceKind::AirPlay && s.device.is_none());
        let merged = bay.merge_device_doors()?;
        bay.ensure_general_door()?;
        Ok((merged, !had))
    })
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn std::error::Error>> {
    let token = match the_token(
        std::env::var("SOUND_TOKEN").ok(),
        std::env::var("SOUND_TOKEN_FILE").ok(),
    ) {
        Ok(token) => token,
        Err(why) => {
            eprintln!("sound: {why}. The hub does not start without a token for the node.");
            std::process::exit(2);
        }
    };
    let state = std::env::var("SOUND_STATE").unwrap_or_else(|_| "sound.json".to_owned());
    let addr = std::env::var("SOUND_ADDR").unwrap_or_else(|_| "127.0.0.1:8791".to_owned());
    let hub = match Hub::open(FileStore::new(&state)) {
        Ok(hub) => hub,
        Err(e) => {
            // Never start empty over a state file it cannot read: the next save would throw
            // the owner's setups away.
            eprintln!("sound: {e}. The hub does not start over a state it cannot read.");
            std::process::exit(2);
        }
    };
    let ffmpeg = std::env::var("SOUND_FFMPEG").unwrap_or_else(|_| "ffmpeg".to_owned());
    let pipes = std::env::var("SOUND_PIPES").unwrap_or_else(|_| "pipes".to_owned());
    let hub = Arc::new(hub);
    // Spotify, when its receiver runs beside the hub (SOUND_SPOTIFY=on): one source for it,
    // made here the first time, read from the pipe the receiver writes.
    if std::env::var("SOUND_SPOTIFY").is_ok_and(|v| v.trim() == "on") {
        match spotify_source(&hub) {
            Ok(true) => println!("sound: Spotify is a source now"),
            Ok(false) => {}
            Err(e) => eprintln!("sound: cannot make the Spotify source: {e}"),
        }
    }
    match settle_doors(&hub) {
        Ok((removed, made)) => {
            if removed > 0 {
                println!("sound: merged {removed} per-device AirPlay doors into the general one");
            }
            if made {
                println!("sound: the general AirPlay door is a source now");
            }
        }
        Err(e) => eprintln!("sound: cannot settle the AirPlay doors: {e}"),
    }
    let listeners = Arc::new(listen::Listeners::new(ffmpeg.clone().into()));
    // The AirPlay senders, when there are any: SOUND_SENDERS lists their OwnTone APIs, in
    // order; each plays the pipe `sender-<n>/group.pcm` in the pipes folder, which the hub
    // makes. SOUND_SENDER_PASSWORD_FILE holds their admin password, when they want one.
    let password = std::env::var("SOUND_SENDER_PASSWORD_FILE")
        .ok()
        .filter(|f| !f.trim().is_empty())
        .map(std::path::PathBuf::from);
    let apis: Vec<String> = std::env::var("SOUND_SENDERS")
        .unwrap_or_default()
        .split(',')
        .map(|a| a.trim().to_owned())
        .filter(|a| !a.is_empty())
        .take(endora_sound::domain::patchbay::SENDERS)
        .collect();
    let mut found_from = None;
    let senders: Box<dyn player::Senders> = if apis.is_empty() {
        println!("sound: no AirPlay senders (SOUND_SENDERS is empty); streams only");
        Box::new(player::NoSenders)
    } else {
        if apis.len() < endora_sound::domain::patchbay::SENDERS {
            eprintln!(
                "sound: {} senders given, but the patchbay allows {} AirPlay groups; the rest will not play",
                apis.len(),
                endora_sound::domain::patchbay::SENDERS
            );
        }
        let places: Vec<senders::SenderPlace> = apis
            .iter()
            .enumerate()
            .map(|(i, api)| senders::SenderPlace {
                api: api.clone(),
                pipe: std::path::Path::new(&pipes)
                    .join(format!("sender-{}", i + 1))
                    .join("group.pcm"),
            })
            .collect();
        for place in &places {
            if let Err(e) = senders::make_pipe(&place.pipe) {
                eprintln!("sound: cannot make {}: {e}", place.pipe.display());
            }
        }
        let instances: Vec<Box<dyn senders::Instance>> = places
            .iter()
            .map(|p| {
                Box::new(senders::OwnTone::new(&p.api, password.clone()))
                    as Box<dyn senders::Instance>
            })
            .collect();
        found_from = Some(Arc::new(senders::OwnTone::new(
            &places[0].api,
            password.clone(),
        )));
        println!("sound: {} AirPlay senders", places.len());
        Box::new(senders::OwnToneSenders::start(
            hub.clone(),
            &places,
            instances,
        ))
    };
    player::start(
        hub.clone(),
        listeners.clone(),
        Places {
            ffmpeg: ffmpeg.into(),
            pipes: pipes.into(),
        },
        senders,
    );
    let app = api::app(api::State {
        hub,
        token: Arc::new(token),
        listeners,
        found_from,
    });
    let listener = tokio::net::TcpListener::bind(&addr).await?;
    println!("sound: listening on {addr}, keeping its state in {state}");
    axum::serve(listener, app).await?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::the_token;

    #[test]
    fn a_token_is_given_made_once_kept_and_never_short() {
        let dir = std::env::temp_dir().join(format!("endora-sound-token-{}", std::process::id()));
        let _ = std::fs::create_dir_all(&dir);
        let file = dir.join("token").to_string_lossy().into_owned();
        let _ = std::fs::remove_file(&file);
        let made = the_token(None, Some(file.clone())).unwrap();
        assert_eq!(made.len(), 48);
        assert_eq!(the_token(None, Some(file.clone())).unwrap(), made, "kept");
        let mode = std::os::unix::fs::PermissionsExt::mode(
            &std::fs::metadata(&file).unwrap().permissions(),
        );
        assert_eq!(mode & 0o777, 0o600, "its owner's only");
        let given = "a-token-of-at-least-thirty-two-characters".to_owned();
        assert_eq!(the_token(Some(given.clone()), Some(file)).unwrap(), given);
        assert!(the_token(Some("short".to_owned()), None).is_err());
        assert!(the_token(None, None).is_err());
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn starting_makes_the_general_door_once_and_merges_old_device_doors() {
        use endora_sound::domain::{Level, SourceKind, SpeakerKind};
        let dir = std::env::temp_dir().join(format!("endora-sound-doors-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let hub = super::Hub::open(super::FileStore::new(dir.join("sound.json"))).unwrap();
        hub.change(|bay| {
            let station = SourceKind::Radio("https://radio.example.com/test".to_owned());
            let station = bay.add_source("Test station", station, None)?;
            let door = bay.add_source("Endora Mixer (Pad)", SourceKind::AirPlay, Some("pad"))?;
            let pad = bay.add_speaker("Pad", SpeakerKind::Stream, Some("pad"))?;
            let kitchen = bay.add_speaker("Kitchen", SpeakerKind::AirPlay, None)?;
            bay.wire(station, pad, Level::new(50).unwrap())?;
            bay.wire(door, kitchen, Level::new(65).unwrap())?;
            Ok(())
        })
        .unwrap();
        assert_eq!(super::settle_doors(&hub), Ok((1, true)));
        assert_eq!(super::settle_doors(&hub), Ok((0, false)), "done twice");
        hub.read(|bay| {
            let mut names: Vec<_> = bay.sources().iter().map(|s| s.name.clone()).collect();
            names.sort();
            assert_eq!(names, vec!["Endora Mixer", "Test station"]);
            let levels: Vec<u8> = bay.wires().iter().map(|w| w.level.get()).collect();
            assert_eq!(levels.len(), 2);
            assert!(levels.contains(&50) && levels.contains(&65));
        });
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn spotify_is_made_a_source_once() {
        let dir = std::env::temp_dir().join(format!(
            "endora-sound-spotify-source-{}",
            std::process::id()
        ));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        let hub = super::Hub::open(super::FileStore::new(dir.join("sound.json"))).unwrap();
        assert_eq!(super::spotify_source(&hub), Ok(true));
        assert_eq!(super::spotify_source(&hub), Ok(false), "made twice");
        assert_eq!(hub.read(|bay| bay.sources().len()), 1);
        let _ = std::fs::remove_dir_all(&dir);
    }
}
