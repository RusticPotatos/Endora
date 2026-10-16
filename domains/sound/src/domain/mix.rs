//! The mix: one group's sources summed at their wires' levels, Endora's voice ducking the
//! rest while it talks, and a limiter so the sum never clips. Pure arithmetic on PCM; what
//! reads the sources and sends the result lives elsewhere.
//!
//! Sound here is what OwnTone's pipes and shairport-sync speak: 44.1 kHz, 16-bit, stereo,
//! interleaved.

use super::patchbay::Level;

/// Frames a second.
pub const RATE: u32 = 44_100;

/// Samples a frame: left, then right.
pub const CHANNELS: usize = 2;

/// How loud the other wires play while Endora speaks: a quarter, about 12 dB down.
pub const DUCKED: f32 = 0.25;

/// The loudest the mix may be, as a share of full scale: a little headroom under clipping.
pub const CEILING: f32 = 0.98;

/// A voice sample quieter than this is not speech: the voice source is wired all day, and
/// its silence (or a little hiss) must not duck anything.
const VOICE_FLOOR: u16 = 64;

/// How long the duck holds after the voice was last heard, so it does not lift between
/// sentences.
const HOLD_MS: f32 = 600.0;

/// How quickly the duck goes down, and comes back up.
const DUCK_ATTACK_MS: f32 = 50.0;
const DUCK_RELEASE_MS: f32 = 300.0;

/// How quickly the limiter lets go once the mix is quiet again. It takes hold at once.
const LIMIT_RELEASE_MS: f32 = 100.0;

/// The share of the way a one-pole smoother moves each frame, for a time constant in ms.
fn step(ms: f32) -> f32 {
    1.0 - (-1000.0 / (ms * RATE as f32)).exp()
}

fn frames_in(ms: f32) -> u32 {
    (ms / 1000.0 * RATE as f32) as u32
}

/// A wire's level as a gain: linear, so 10 is a tenth (20 dB down), quiet but there.
#[must_use]
pub fn gain(level: Level) -> f32 {
    f32::from(level.get()) / 100.0
}

/// One source's next block, and how it joins the mix.
#[derive(Debug, Clone, Copy)]
pub struct Input<'a> {
    /// Its samples, interleaved. Shorter than the block means it ran out: silence after.
    pub samples: &'a [i16],
    /// Its wire's level.
    pub level: Level,
    /// Whether this is Endora's voice, which ducks every other input and is never ducked.
    pub voice: bool,
}

/// One group's mixer. It carries the duck and the limiter from one block to the next, so a
/// group keeps one mixer for as long as it plays.
#[derive(Debug, Clone)]
pub struct Mixer {
    duck: f32,
    hold: u32,
    limit: f32,
}

impl Default for Mixer {
    fn default() -> Self {
        Self::new()
    }
}

impl Mixer {
    /// A mixer at rest: nothing ducked, nothing limited.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            duck: 1.0,
            hold: 0,
            limit: 1.0,
        }
    }

    /// Mixes the next block of every input into `out`, which sets the block's length. A
    /// trailing half frame in `out` is left silent.
    pub fn mix(&mut self, inputs: &[Input<'_>], out: &mut [i16]) {
        let (attack, release, let_go) = (
            step(DUCK_ATTACK_MS),
            step(DUCK_RELEASE_MS),
            step(LIMIT_RELEASE_MS),
        );
        let hold = frames_in(HOLD_MS);
        out.fill(0);
        for (f, frame) in out.chunks_exact_mut(CHANNELS).enumerate() {
            let at = f * CHANNELS;
            let speaking = inputs.iter().any(|i| {
                i.voice
                    && i.samples
                        .get(at..at + CHANNELS)
                        .is_some_and(|s| s.iter().any(|x| x.unsigned_abs() > VOICE_FLOOR))
            });
            if speaking {
                self.hold = hold;
            }
            let target = if self.hold > 0 { DUCKED } else { 1.0 };
            self.hold = self.hold.saturating_sub(1);
            self.duck += (target - self.duck) * if target < self.duck { attack } else { release };

            let mut sum = [0.0f32; CHANNELS];
            for input in inputs {
                let g = gain(input.level) * if input.voice { 1.0 } else { self.duck };
                for (c, s) in sum.iter_mut().enumerate() {
                    let x = input.samples.get(at + c).copied().unwrap_or(0);
                    *s += f32::from(x) / 32_768.0 * g;
                }
            }

            let peak = sum.iter().fold(0.0f32, |m, s| m.max(s.abs()));
            let needed = if peak > CEILING { CEILING / peak } else { 1.0 };
            if needed < self.limit {
                self.limit = needed;
            } else {
                self.limit = (self.limit + (1.0 - self.limit) * let_go).min(needed);
            }
            for (o, s) in frame.iter_mut().zip(sum) {
                *o = (s * self.limit * 32_768.0)
                    .round()
                    .clamp(-32_768.0, 32_767.0) as i16;
            }
        }
    }
}

#[cfg(test)]
#[path = "mix_tests.rs"]
mod tests;
