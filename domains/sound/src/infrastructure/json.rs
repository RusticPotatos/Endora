//! The patchbay as JSON: what the file store keeps, and the shape the API answers with.

use serde_json::{Value, json};

use crate::domain::patchbay::{SYNC_MAX, SYNC_MIN, web_address};
use crate::domain::{
    Level, Patchbay, Source, SourceId, SourceKind, Speaker, SpeakerId, SpeakerKind, Wire,
};

/// The version of the shape below. A file of another version is refused, not guessed at.
pub const VERSION: u64 = 1;

fn wire_json(w: &Wire) -> Value {
    json!({ "source": w.source.0, "speaker": w.speaker.0, "level": w.level.get() })
}

/// One source, as JSON.
#[must_use]
pub fn source_json(s: &Source) -> Value {
    let mut v = json!({ "id": s.id.0, "name": s.name, "kind": s.kind.name(), "device": s.device });
    if let SourceKind::Radio(address) = &s.kind {
        v["address"] = json!(address);
    }
    v
}

/// One speaker, as JSON.
#[must_use]
pub fn speaker_json(s: &Speaker) -> Value {
    json!({
        "id": s.id.0, "name": s.name, "kind": s.kind.name(), "device": s.device,
        "volume": s.volume.get(), "sync_ms": s.sync_ms, "borrowed": s.borrowed,
    })
}

/// The whole patchbay, as JSON.
#[must_use]
pub fn to_json(bay: &Patchbay) -> Value {
    json!({
        "version": VERSION,
        "revision": bay.revision(),
        "last_id": bay.last_id(),
        "sources": bay.sources().into_iter().map(source_json).collect::<Vec<_>>(),
        "speakers": bay.speakers().into_iter().map(speaker_json).collect::<Vec<_>>(),
        "wires": bay.wires().iter().map(wire_json).collect::<Vec<_>>(),
        "setups": bay.setups().into_iter().map(|(name, wires)| json!({
            "name": name, "wires": wires.iter().map(wire_json).collect::<Vec<_>>(),
        })).collect::<Vec<_>>(),
    })
}

fn field<'a>(v: &'a Value, name: &str) -> Result<&'a Value, String> {
    v.get(name).ok_or_else(|| format!("missing {name}"))
}

fn number(v: &Value, name: &str) -> Result<u64, String> {
    field(v, name)?
        .as_u64()
        .ok_or_else(|| format!("{name} is not a whole number"))
}

fn id(v: &Value, name: &str) -> Result<u32, String> {
    u32::try_from(number(v, name)?).map_err(|_| format!("{name} is too large"))
}

fn text<'a>(v: &'a Value, name: &str) -> Result<&'a str, String> {
    field(v, name)?
        .as_str()
        .ok_or_else(|| format!("{name} is not text"))
}

fn level(v: &Value, name: &str) -> Result<Level, String> {
    u8::try_from(number(v, name)?)
        .ok()
        .and_then(Level::new)
        .ok_or_else(|| format!("{name} is not 0 to 100"))
}

/// A speaker's lip-sync; a file from before it existed has none, which is zero.
fn sync(v: &Value) -> Result<i16, String> {
    let Some(n) = v.get("sync_ms") else {
        return Ok(0);
    };
    n.as_i64()
        .and_then(|n| i16::try_from(n).ok())
        .filter(|n| (SYNC_MIN..=SYNC_MAX).contains(n))
        .ok_or_else(|| "sync_ms is not -500 to 1500".to_owned())
}

fn device(v: &Value) -> Result<Option<String>, String> {
    match v.get("device") {
        None | Some(Value::Null) => Ok(None),
        Some(Value::String(d)) => Ok(Some(d.clone())),
        Some(_) => Err("device is not text".to_owned()),
    }
}

fn list<'a>(v: &'a Value, name: &str) -> Result<&'a Vec<Value>, String> {
    field(v, name)?
        .as_array()
        .ok_or_else(|| format!("{name} is not a list"))
}

fn wire_from(v: &Value) -> Result<Wire, String> {
    Ok(Wire {
        source: SourceId(id(v, "source")?),
        speaker: SpeakerId(id(v, "speaker")?),
        level: level(v, "level")?,
    })
}

/// A patchbay from JSON that [`to_json`] made.
///
/// # Errors
/// Anything that is not that shape, of this version. A radio address that is not a web
/// address is refused here too: the file can be edited by hand.
pub fn from_json(v: &Value) -> Result<Patchbay, String> {
    let version = number(v, "version")?;
    if version != VERSION {
        return Err(format!(
            "version {version}, but this hub reads version {VERSION}"
        ));
    }
    let mut bay = Patchbay::default();
    for s in list(v, "sources")? {
        let kind = match text(s, "kind")? {
            "airplay" => SourceKind::AirPlay,
            "spotify" => SourceKind::Spotify,
            "voice" => SourceKind::Voice,
            "radio" => {
                let address = text(s, "address")?;
                if !web_address(address) {
                    return Err(format!("radio address {address:?} is not a web address"));
                }
                SourceKind::Radio(address.to_owned())
            }
            other => return Err(format!("unknown source kind {other:?}")),
        };
        bay.restore_source(Source {
            id: SourceId(id(s, "id")?),
            name: text(s, "name")?.to_owned(),
            kind,
            device: device(s)?,
        });
    }
    for s in list(v, "speakers")? {
        let kind = text(s, "kind")?;
        bay.restore_speaker(Speaker {
            id: SpeakerId(id(s, "id")?),
            name: text(s, "name")?.to_owned(),
            kind: SpeakerKind::from_name(kind)
                .ok_or_else(|| format!("unknown speaker kind {kind:?}"))?,
            device: device(s)?,
            volume: level(s, "volume")?,
            sync_ms: sync(s)?,
            borrowed: field(s, "borrowed")?
                .as_bool()
                .ok_or("borrowed is not true or false")?,
        });
    }
    for w in list(v, "wires")? {
        bay.restore_wire(wire_from(w)?);
    }
    for s in list(v, "setups")? {
        let wires = list(s, "wires")?
            .iter()
            .map(wire_from)
            .collect::<Result<Vec<_>, _>>()?;
        bay.restore_setup(text(s, "name")?, wires);
    }
    bay.restore_last_id(id(v, "last_id")?);
    bay.restore_revision(number(v, "revision")?);
    Ok(bay)
}
