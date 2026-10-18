//! The live stream (ADR 0104, "live mode"): what a stream speaker plays, as raw PCM in small
//! frames, for a browser that must hear it with as little delay as it can.
//!
//! The MP3 stream is for listening: the browser buffers it for seconds, which is what lets a
//! locked phone keep playing, and what makes it useless beside a television. A live frame
//! carries 20 ms of 16-bit little-endian stereo, a sequence number and the hub's clock at the
//! moment it was mixed, so the page can tell a late frame from a missing one and size its own
//! buffer to the network it is on. No encoder is in the path: at 44.1 kHz it is 1.4 Mbit/s,
//! which a home network does not notice, and a browser needs no decoder for it.
//!
//! A frame is a 24-byte header and the samples:
//!
//! | bytes | what |
//! |-------|------|
//! | 0..2  | `EL` |
//! | 2     | flags: bit 0 is set on the frame where a delay probe begins |
//! | 3     | channels (2) |
//! | 4..8  | sequence number, wrapping, one more for each frame |
//! | 8..16 | the hub's clock at the frame's first sample, microseconds since 1970 |
//! | 16..20| sample rate, hertz |
//! | 20..24| bytes of samples that follow |

use axum::body::Bytes;
use endora_sound::domain::mix::{CHANNELS, RATE};

/// The two bytes every frame begins with.
pub const MAGIC: [u8; 2] = *b"EL";

/// The header's length, in bytes.
pub const HEADER_LEN: usize = 24;

/// Set on the frame where a delay probe begins.
pub const FLAG_PROBE: u8 = 1;

/// Frames (one sample per channel) in a full live frame: 20 ms.
pub const FRAME_FRAMES: usize = RATE as usize / 50;

/// The probe's tone, in hertz. The page listens for it with a filter tuned to it.
pub const PROBE_HZ: f32 = 2_000.0;

/// The probe's loudness, as a share of full scale: quiet, but far above any noise floor.
pub const PROBE_AMPLITUDE: f32 = 0.1;

/// How long the probe sounds, in milliseconds.
pub const PROBE_MS: usize = 80;

/// The probe fades in and out over this long, so it does not click.
const PROBE_RAMP_MS: usize = 3;

/// One frame, ready to send.
#[must_use]
pub fn frame(seq: u32, at_us: u64, flags: u8, samples: &[i16]) -> Bytes {
    let payload = samples.len() * 2;
    let mut out = Vec::with_capacity(HEADER_LEN + payload);
    out.extend_from_slice(&MAGIC);
    out.push(flags);
    out.push(u8::try_from(CHANNELS).unwrap_or(2));
    out.extend_from_slice(&seq.to_le_bytes());
    out.extend_from_slice(&at_us.to_le_bytes());
    out.extend_from_slice(&RATE.to_le_bytes());
    out.extend_from_slice(&u32::try_from(payload).unwrap_or(u32::MAX).to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    Bytes::from(out)
}

/// A block of samples cut into live frames of 20 ms; the last may be shorter.
pub fn cut(samples: &[i16]) -> impl Iterator<Item = &[i16]> {
    samples.chunks(FRAME_FRAMES * CHANNELS)
}

/// How long `frames` samples per channel last, in microseconds.
#[must_use]
pub fn micros(frames: usize) -> u64 {
    (frames as u64) * 1_000_000 / u64::from(RATE)
}

/// A delay probe on its way into one speaker's mix: for [`PROBE_MS`] the mix is replaced by a
/// tone the page knows, so it can tell exactly when it comes out. The mix is replaced, not
/// added to, so that music cannot hide the tone; the owner asked for the measurement and hears
/// a short gap and a beep.
#[derive(Debug, Default)]
pub struct Probe {
    done: usize,
}

impl Probe {
    /// A probe that has not begun.
    #[must_use]
    pub const fn new() -> Self {
        Self { done: 0 }
    }

    /// Whether it has not put a sample into the mix yet.
    #[must_use]
    pub const fn fresh(&self) -> bool {
        self.done == 0
    }

    /// Whether the whole tone has been played.
    #[must_use]
    pub const fn finished(&self) -> bool {
        self.done >= RATE as usize * PROBE_MS / 1000
    }

    /// Puts as much of the tone as is left over the start of `block`.
    pub fn play_over(&mut self, block: &mut [i16]) {
        let total = RATE as usize * PROBE_MS / 1000;
        let ramp = (RATE as usize * PROBE_RAMP_MS / 1000).max(1);
        for frame in block.chunks_exact_mut(CHANNELS) {
            if self.done >= total {
                break;
            }
            let i = self.done;
            let envelope = (i.min(total - 1 - i).min(ramp) as f32 / ramp as f32).min(1.0);
            let phase = 2.0 * std::f32::consts::PI * PROBE_HZ * i as f32 / RATE as f32;
            let s = (phase.sin() * envelope * PROBE_AMPLITUDE * f32::from(i16::MAX)).round();
            for c in frame.iter_mut() {
                *c = s as i16;
            }
            self.done += 1;
        }
    }
}

/// What a frame says, read back. Only the tests read frames; the page does that in a browser.
#[cfg(test)]
#[derive(Debug, PartialEq, Eq)]
pub struct Header {
    pub flags: u8,
    pub channels: u8,
    pub seq: u32,
    pub at_us: u64,
    pub rate: u32,
    pub samples: Vec<i16>,
}

/// Reads one frame from the front of `bytes`, and what is left after it.
#[cfg(test)]
#[must_use]
pub fn parse(bytes: &[u8]) -> Option<(Header, &[u8])> {
    let head = bytes.get(..HEADER_LEN)?;
    if head[..2] != MAGIC {
        return None;
    }
    let payload = u32::from_le_bytes(head[20..24].try_into().ok()?) as usize;
    let body = bytes.get(HEADER_LEN..HEADER_LEN + payload)?;
    let samples = body
        .chunks_exact(2)
        .map(|b| i16::from_le_bytes([b[0], b[1]]))
        .collect();
    Some((
        Header {
            flags: head[2],
            channels: head[3],
            seq: u32::from_le_bytes(head[4..8].try_into().ok()?),
            at_us: u64::from_le_bytes(head[8..16].try_into().ok()?),
            rate: u32::from_le_bytes(head[16..20].try_into().ok()?),
            samples,
        },
        &bytes[HEADER_LEN + payload..],
    ))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_frame_carries_its_number_its_time_and_its_samples() {
        let bytes = frame(7, 1_700_000_000_123_456, 0, &[1, -2, 300, -400]);
        assert_eq!(bytes.len(), HEADER_LEN + 8);
        let (h, rest) = parse(&bytes).unwrap();
        assert!(rest.is_empty());
        assert_eq!(h.seq, 7);
        assert_eq!(h.at_us, 1_700_000_000_123_456);
        assert_eq!(h.rate, RATE);
        assert_eq!(h.channels, 2);
        assert_eq!(h.flags, 0);
        assert_eq!(h.samples, vec![1, -2, 300, -400]);
    }

    #[test]
    fn frames_follow_each_other_and_read_back_one_by_one() {
        let mut stream = frame(1, 10, 0, &[1, 2]).to_vec();
        stream.extend_from_slice(&frame(2, 20, FLAG_PROBE, &[3, 4]));
        let (a, rest) = parse(&stream).unwrap();
        let (b, rest) = parse(rest).unwrap();
        assert!(rest.is_empty());
        assert_eq!((a.seq, a.flags), (1, 0));
        assert_eq!((b.seq, b.flags), (2, FLAG_PROBE));
        // A frame cut short is not a frame yet, and bytes that are not a frame are refused.
        let (_, rest) = parse(&stream[..stream.len() - 1]).unwrap();
        assert!(parse(rest).is_none());
        assert!(parse(b"nonsense that is long enough to hold a header ........").is_none());
    }

    #[test]
    fn a_block_is_cut_into_frames_of_twenty_milliseconds() {
        let block = vec![0i16; FRAME_FRAMES * CHANNELS * 2 + 10];
        let sizes: Vec<usize> = cut(&block).map(<[i16]>::len).collect();
        assert_eq!(
            sizes,
            vec![FRAME_FRAMES * CHANNELS, FRAME_FRAMES * CHANNELS, 10]
        );
        assert_eq!(micros(FRAME_FRAMES), 20_000);
    }

    #[test]
    fn the_probe_replaces_the_mix_with_a_tone_that_fades_in_and_out() {
        let mut probe = Probe::new();
        assert!(probe.fresh());
        let mut block = vec![12_345i16; FRAME_FRAMES * CHANNELS];
        probe.play_over(&mut block);
        assert!(!probe.fresh());
        // The music under it is gone, the left and right are the same, and it starts quietly.
        assert!(block.iter().all(|s| *s != 12_345));
        assert!(block.chunks(2).all(|f| f[0] == f[1]));
        assert!(block[..4].iter().all(|s| s.abs() < 200));
        let peak = block.iter().map(|s| i32::from(s.abs())).max().unwrap();
        let wanted = (PROBE_AMPLITUDE * f32::from(i16::MAX)) as i32;
        assert!((peak - wanted).abs() < 200, "peak {peak}, wanted {wanted}");
        // The rest of it goes into the blocks after, and what is past its end is left alone.
        assert!(!probe.finished());
        let mut next = vec![5i16; FRAME_FRAMES * CHANNELS * 3];
        probe.play_over(&mut next);
        assert!(probe.finished());
        let tone_frames = RATE as usize * PROBE_MS / 1000 - FRAME_FRAMES;
        assert!(next[tone_frames * CHANNELS..].iter().all(|s| *s == 5));
        assert!(
            next[(tone_frames - 2) * CHANNELS..tone_frames * CHANNELS]
                .iter()
                .all(|s| s.abs() < 200)
        );
    }
}
