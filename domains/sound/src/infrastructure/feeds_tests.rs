use std::io::{Read as _, Write as _};
use std::net::TcpListener;
use std::time::{Duration, Instant};

use super::*;

#[test]
fn a_buffer_gathers_before_it_plays_and_hands_out_whole_frames() {
    let mut b = Buffer::live();
    b.push(&vec![7; PRIME - 2]);
    assert!(
        b.take(100).is_empty(),
        "it played before it had gathered enough"
    );
    b.push(&[7, 7, 7]);
    let got = b.take(101);
    assert_eq!(got.len(), 100, "not whole frames");
}

#[test]
fn a_buffer_that_runs_dry_gathers_again() {
    let mut b = Buffer::live();
    b.push(&vec![1; PRIME]);
    assert_eq!(b.take(PRIME).len(), PRIME);
    b.push(&[1; 10]);
    assert!(
        b.take(10).is_empty(),
        "it played a trickle straight after running dry"
    );
}

#[test]
fn a_buffer_that_runs_ahead_loses_its_oldest() {
    let mut b = Buffer::live();
    b.push(&vec![1; MOST]);
    b.push(&[2; 4]);
    assert_eq!(b.len(), MOST);
    let all = b.take(MOST);
    assert_eq!(&all[all.len() - 4..], &[2, 2, 2, 2]);
}

#[test]
fn speech_is_kept_whole_and_plays_at_once() {
    let mut b = Buffer::spoken();
    let sentence = vec![3; 10 * RATE as usize * CHANNELS];
    b.push(&sentence);
    assert_eq!(
        b.len(),
        sentence.len(),
        "ten seconds of speech lost its start"
    );
    b.push(&[4, 4]);
    assert_eq!(b.take(2), vec![3, 3]);
    let mut short = Buffer::spoken();
    short.push(&[5, 5]);
    assert_eq!(short.take(2), vec![5, 5], "a short word never played");
}

#[test]
fn a_station_is_fetched_without_a_shell_and_only_over_the_network() {
    let args = station_arguments("https://radio.example.com/live; rm -rf /");
    let i = args.iter().position(|a| a == "-i").unwrap();
    assert_eq!(
        args[i + 1],
        "https://radio.example.com/live; rm -rf /",
        "one argument, untouched"
    );
    let w = args
        .iter()
        .position(|a| a == "-protocol_whitelist")
        .unwrap();
    assert!(w < i, "the whitelist must come before the input");
    assert!(!args[w + 1].split(',').any(|p| p == "file" || p == "concat"));
    assert!(args.ends_with(&["pipe:1".to_owned()]));
}

#[test]
fn a_dropped_station_waits_longer_each_time_but_never_more_than_a_minute() {
    assert_eq!(backoff(0), Duration::from_secs(1));
    assert_eq!(backoff(1), Duration::from_secs(2));
    assert_eq!(backoff(3), Duration::from_secs(8));
    assert_eq!(backoff(40), Duration::from_secs(60));
}

/// A reader that hands out bytes a few at a time, odd counts included.
struct Dribble(Vec<u8>, usize);

impl std::io::Read for Dribble {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let n = 3.min(self.0.len() - self.1).min(buf.len());
        buf[..n].copy_from_slice(&self.0[self.1..self.1 + n]);
        self.1 += n;
        Ok(n)
    }
}

#[test]
fn bytes_split_mid_sample_come_out_as_the_samples_they_were() {
    let samples: Vec<i16> = (0..PRIME as i16).map(|i| i.wrapping_mul(311)).collect();
    let bytes: Vec<u8> = samples.iter().flat_map(|s| s.to_le_bytes()).collect();
    let into: Shared = Arc::default();
    pump(Dribble(bytes, 0), &into, &AtomicBool::new(false));
    assert_eq!(lock(&into).take(samples.len()), samples);
}

fn ffmpeg() -> Option<PathBuf> {
    let found = Command::new("ffmpeg")
        .arg("-version")
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
    if found.is_ok_and(|s| s.success()) {
        return Some(PathBuf::from("ffmpeg"));
    }
    eprintln!(
        "no ffmpeg here: the live station test is skipped (it runs in the image and on the Mac)"
    );
    None
}

/// Two seconds of a 440 Hz tone, as a WAV file.
fn tone() -> Vec<u8> {
    let frames = 2 * RATE as usize;
    let mut data = Vec::with_capacity(frames * 4);
    for i in 0..frames {
        let v = ((i as f32 * 440.0 * std::f32::consts::TAU / RATE as f32).sin() * 12_000.0) as i16;
        data.extend_from_slice(&v.to_le_bytes());
        data.extend_from_slice(&v.to_le_bytes());
    }
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&(36 + data.len() as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVEfmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&RATE.to_le_bytes());
    wav.extend_from_slice(&(RATE * 4).to_le_bytes());
    wav.extend_from_slice(&4u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(data.len() as u32).to_le_bytes());
    wav.extend_from_slice(&data);
    wav
}

/// A web server on this machine that answers every request with `body`.
fn serve(body: Vec<u8>) -> String {
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let port = listener.local_addr().unwrap().port();
    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(mut stream) = stream else { continue };
            let mut request = [0u8; 2048];
            let _ = stream.read(&mut request);
            let _ = stream.write_all(
                format!(
                    "HTTP/1.0 200 OK\r\nContent-Type: audio/wav\r\nContent-Length: {}\r\n\r\n",
                    body.len()
                )
                .as_bytes(),
            );
            let _ = stream.write_all(&body);
        }
    });
    format!("http://127.0.0.1:{port}/classical.wav")
}

fn wait_for(mut done: impl FnMut() -> bool) -> bool {
    let until = Instant::now() + Duration::from_secs(15);
    while Instant::now() < until {
        if done() {
            return true;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    false
}

#[test]
fn a_station_plays_through_ffmpeg_into_its_source() {
    let Some(ffmpeg) = ffmpeg() else { return };
    let address = serve(tone());
    let mut bay = Patchbay::default();
    let radio = bay
        .add_source("Classical", SourceKind::Radio(address), None)
        .unwrap();
    let mut feeds = FeedSet::new(Places {
        ffmpeg,
        pipes: std::env::temp_dir(),
    });
    feeds.keep(&bay, &[radio]);
    let mut heard = Vec::new();
    assert!(
        wait_for(|| {
            heard.extend(feeds.take(radio, 4410));
            heard.len() >= RATE as usize
        }),
        "nothing came from the station"
    );
    assert!(
        heard.iter().any(|s| s.unsigned_abs() > 10_000),
        "the tone did not come through"
    );
    feeds.keep(&bay, &[]);
    assert!(feeds.open().is_empty());
}

#[cfg(unix)]
#[test]
fn a_named_pipe_is_read_as_it_is_written_and_let_go_when_closed() {
    let dir = std::env::temp_dir().join(format!(
        "endora-sound-pipes-{}-{:?}",
        std::process::id(),
        Instant::now()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let mut bay = Patchbay::default();
    let phone = bay
        .add_source("Endora · iPhone", SourceKind::AirPlay, Some("iphone"))
        .unwrap();
    let mut feeds = FeedSet::new(Places {
        ffmpeg: PathBuf::from("ffmpeg"),
        pipes: dir.clone(),
    });
    let path = feeds.pipe_of(phone);
    assert!(
        Command::new("mkfifo")
            .arg(&path)
            .status()
            .unwrap()
            .success()
    );
    feeds.keep(&bay, &[phone]);
    let writer_path = path.clone();
    let writer = std::thread::spawn(move || {
        let mut w = std::fs::OpenOptions::new()
            .write(true)
            .open(writer_path)
            .unwrap();
        let bytes: Vec<u8> = vec![9i16; PRIME * 2]
            .iter()
            .flat_map(|s| s.to_le_bytes())
            .collect();
        w.write_all(&bytes).unwrap();
    });
    writer.join().unwrap();
    let mut heard = Vec::new();
    assert!(wait_for(|| {
        heard.extend(feeds.take(phone, 4410));
        heard.len() >= PRIME
    }));
    assert!(heard.iter().all(|s| *s == 9));
    // Closing it does not hang, even with its reader waiting for the next writer.
    let started = Instant::now();
    feeds.keep(&bay, &[]);
    assert!(started.elapsed() < Duration::from_secs(2));
    let _ = std::fs::remove_dir_all(&dir);
}
