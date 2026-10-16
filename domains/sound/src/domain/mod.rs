//! Sound's domain: the patchbay and the mix, as pure rules.

pub mod mix;
pub mod patchbay;

pub use mix::{Input, Mixer};
pub use patchbay::{
    Group, Level, Patchbay, Refusal, Source, SourceId, SourceKind, Speaker, SpeakerId, SpeakerKind,
    Wire,
};
