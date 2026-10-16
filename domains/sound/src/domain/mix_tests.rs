use super::*;

fn level(n: u8) -> Level {
    Level::new(n).unwrap()
}

/// `frames` stereo frames of one value on both channels.
fn steady(value: i16, frames: usize) -> Vec<i16> {
    vec![value; frames * CHANNELS]
}

/// A full-scale square wave, the loudest thing a source can send.
fn loudest(frames: usize) -> Vec<i16> {
    (0..frames * CHANNELS)
        .map(|i| {
            if (i / CHANNELS / 50) % 2 == 0 {
                i16::MAX
            } else {
                i16::MIN
            }
        })
        .collect()
}

fn music(samples: &[i16], n: u8) -> Input<'_> {
    Input {
        samples,
        level: level(n),
        voice: false,
    }
}

fn say(samples: &[i16]) -> Input<'_> {
    Input {
        samples,
        level: level(100),
        voice: true,
    }
}

const SECOND: usize = RATE as usize;

#[test]
fn one_source_at_full_level_passes_through_untouched() {
    let input: Vec<i16> = (0..2000)
        .map(|i| i16::try_from(i * 7 - 7000).unwrap())
        .collect();
    let mut out = vec![0; input.len()];
    Mixer::new().mix(&[music(&input, 100)], &mut out);
    assert_eq!(out, input);
}

#[test]
fn a_level_scales_its_source() {
    let input = steady(10_000, 100);
    let mut out = vec![0; input.len()];
    Mixer::new().mix(&[music(&input, 50)], &mut out);
    assert!(out.iter().all(|&s| s == 5_000), "{:?}", &out[..4]);
}

#[test]
fn level_zero_is_silence() {
    let input = steady(20_000, 100);
    let mut out = vec![1; input.len()];
    Mixer::new().mix(&[music(&input, 0)], &mut out);
    assert!(out.iter().all(|&s| s == 0));
}

#[test]
fn sources_are_summed_at_their_own_levels() {
    // Classical at 10 under the phone at 90.
    let phone = steady(10_000, 100);
    let classical = steady(-10_000, 100);
    let mut out = vec![0; phone.len()];
    Mixer::new().mix(&[music(&phone, 90), music(&classical, 10)], &mut out);
    assert!(out.iter().all(|&s| s == 8_000), "{:?}", &out[..4]);
}

#[test]
fn a_source_that_falls_short_is_silence_for_the_rest() {
    let short = steady(1_000, 10);
    let mut out = vec![7; 40];
    Mixer::new().mix(&[music(&short, 100)], &mut out);
    assert!(out[..20].iter().all(|&s| s == 1_000));
    assert!(out[20..].iter().all(|&s| s == 0));
}

#[test]
fn nothing_wired_is_silence() {
    let mut out = vec![9; 40];
    Mixer::new().mix(&[], &mut out);
    assert!(out.iter().all(|&s| s == 0));
}

#[test]
fn the_sum_never_clips_or_wraps() {
    let a = loudest(SECOND / 10);
    let b = loudest(SECOND / 10);
    let c = steady(i16::MAX, SECOND / 10);
    let mut out = vec![0; a.len()];
    Mixer::new().mix(&[music(&a, 100), music(&b, 100), music(&c, 100)], &mut out);
    let ceiling = (CEILING * 32_768.0) as i32 + 1;
    assert!(
        out.iter().all(|&s| i32::from(s).abs() <= ceiling),
        "a sample went over the ceiling"
    );
    // A wrapped sample would flip sign against the inputs; the sum of positives stays positive.
    for (i, &s) in out.iter().enumerate() {
        if a[i] > 0 && b[i] > 0 {
            assert!(s > 0, "sample {i} wrapped to {s}");
        }
    }
}

#[test]
fn the_limiter_lets_go_again_once_it_is_quiet() {
    let mut mixer = Mixer::new();
    let loud = loudest(SECOND / 10);
    let mut out = vec![0; loud.len()];
    mixer.mix(&[music(&loud, 100), music(&loud, 100)], &mut out);
    let quiet = steady(10_000, SECOND);
    let mut out = vec![0; quiet.len()];
    mixer.mix(&[music(&quiet, 100)], &mut out);
    // Held down at first, back to unity within the second.
    assert!(out[0] < 10_000, "the limiter let go at once: {}", out[0]);
    assert!(
        (9_990..=10_000).contains(out.last().unwrap()),
        "{}",
        out.last().unwrap()
    );
}

#[test]
fn endoras_voice_ducks_the_others_and_not_itself() {
    let mut mixer = Mixer::new();
    let frames = SECOND / 2;
    let song = steady(10_000, frames);
    let voice = steady(4_000, frames);
    let mut out = vec![0; song.len()];
    mixer.mix(
        &[
            music(&song, 100),
            Input {
                samples: &voice,
                level: level(100),
                voice: true,
            },
        ],
        &mut out,
    );
    // By the end of half a second the song plays at DUCKED, the voice at its own level.
    let expected = (10_000.0 * DUCKED) as i32 + 4_000;
    let last = i32::from(*out.last().unwrap());
    assert!(
        (last - expected).abs() <= 20,
        "{last} is not about {expected}"
    );
}

#[test]
fn ducking_holds_through_a_pause_and_then_comes_back() {
    let mut mixer = Mixer::new();
    let frames = SECOND / 2;
    let song = steady(10_000, frames);
    let voice = steady(4_000, frames);
    let silence = steady(0, frames);
    let mut out = vec![0; song.len()];
    mixer.mix(&[music(&song, 100), say(&voice)], &mut out);
    // A short pause between sentences: still ducked.
    let pause = steady(10_000, SECOND / 4);
    let mut out = vec![0; pause.len()];
    mixer.mix(
        &[music(&pause, 100), say(&silence[..pause.len()])],
        &mut out,
    );
    assert!(
        *out.last().unwrap() < 4_000,
        "the duck let go in a pause: {}",
        out.last().unwrap()
    );
    // Done speaking: within two seconds the song is back.
    let after = steady(10_000, 2 * SECOND);
    let mut out = vec![0; after.len()];
    mixer.mix(&[music(&after, 100)], &mut out);
    assert!(*out.last().unwrap() >= 9_950, "{}", out.last().unwrap());
}

#[test]
fn ducking_moves_smoothly_with_no_click() {
    let mut mixer = Mixer::new();
    let song = steady(20_000, SECOND / 2);
    let voice = steady(1_000, SECOND / 2);
    let mut out = vec![0; song.len()];
    mixer.mix(
        &[
            music(&song, 100),
            Input {
                samples: &voice,
                level: level(100),
                voice: true,
            },
        ],
        &mut out,
    );
    // A click is a jump: no frame may move more than a few hundred from the one before.
    let biggest = out
        .chunks_exact(CHANNELS)
        .zip(out.chunks_exact(CHANNELS).skip(1))
        .map(|(a, b)| (i32::from(a[0]) - i32::from(b[0])).abs())
        .max()
        .unwrap();
    assert!(biggest < 300, "a jump of {biggest}");
}

#[test]
fn a_quiet_voice_source_does_not_duck_anything() {
    // The voice source is wired all day; only when it actually speaks does it duck.
    let mut mixer = Mixer::new();
    let song = steady(10_000, SECOND / 10);
    let hiss = steady(8, SECOND / 10);
    let mut out = vec![0; song.len()];
    mixer.mix(
        &[
            music(&song, 100),
            Input {
                samples: &hiss,
                level: level(100),
                voice: true,
            },
        ],
        &mut out,
    );
    assert!(out.chunks_exact(CHANNELS).all(|f| f[0] == 10_008));
}
