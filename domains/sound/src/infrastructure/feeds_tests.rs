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
    let path = feeds.pipe_of(bay.source(phone).unwrap());
    // Named for its device, so its receiver knows where to write; the hub makes it.
    assert_eq!(path.file_name().unwrap(), "airplay-iphone.pcm");
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

#[cfg(unix)]
#[test]
fn spotify_is_read_even_unwired_so_its_writer_never_waits() {
    let dir = std::env::temp_dir().join(format!(
        "endora-sound-spotify-{}-{:?}",
        std::process::id(),
        Instant::now()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let mut bay = Patchbay::default();
    let spotify = bay
        .add_source("Spotify", SourceKind::Spotify, None)
        .unwrap();
    // librespot started first, and left an ordinary file where its pipe should be.
    std::fs::write(dir.join("spotify.pcm"), b"not a pipe").unwrap();
    let mut feeds = FeedSet::new(Places {
        ffmpeg: PathBuf::from("ffmpeg"),
        pipes: dir.clone(),
    });
    let path = feeds.pipe_of(bay.source(spotify).unwrap());
    assert_eq!(path.file_name().unwrap(), "spotify.pcm");
    feeds.keep(&bay, &[]); // wired to nothing
    let (done, finished) = std::sync::mpsc::channel();
    std::thread::spawn(move || {
        let opened = std::fs::OpenOptions::new().write(true).open(&path);
        let _ = done.send(opened.and_then(|mut w| w.write_all(&[1, 0, 2, 0])).is_ok());
    });
    assert_eq!(
        finished.recv_timeout(Duration::from_secs(5)),
        Ok(true),
        "the writer waited: nobody read the pipe"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_devices_name_is_made_safe_for_its_pipe() {
    assert_eq!(safe("iphone"), "iphone");
    assert_eq!(safe("../etc/passwd"), "___etc_passwd");
    assert_eq!(safe("ipad 2"), "ipad_2");
}

/// A reader with as much as anyone asks for, counting what is read.
struct Endless(std::sync::Arc<std::sync::atomic::AtomicUsize>);

impl std::io::Read for Endless {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        buf.fill(1);
        self.0
            .fetch_add(buf.len(), std::sync::atomic::Ordering::Relaxed);
        Ok(buf.len())
    }
}

#[test]
fn a_writer_faster_than_real_time_is_held_back_not_skipped_through() {
    // librespot decodes as fast as its output takes it. The pump must stop taking once the
    // buffer is full, so the writer waits, instead of dropping the oldest and reading on.
    let read = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let into: Shared = Arc::default();
    let stop = Arc::new(AtomicBool::new(false));
    let (r, i, st) = (read.clone(), into.clone(), stop.clone());
    let pumping = std::thread::spawn(move || pump(Endless(r), &i, &st));
    std::thread::sleep(Duration::from_millis(300));
    let bytes = read.load(std::sync::atomic::Ordering::Relaxed);
    // Two bytes a sample; at most a buffer's worth and one read more.
    assert!(
        bytes <= MOST * 2 + 8192,
        "read {bytes} bytes: it raced ahead of its buffer"
    );
    assert!(lock(&into).full());
    // Taking some lets it read some more, and no more than that.
    let _ = lock(&into).take(PRIME);
    std::thread::sleep(Duration::from_millis(100));
    assert!(lock(&into).full());
    stop.store(true, Ordering::Relaxed);
    pumping.join().unwrap();
}

#[test]
fn an_unwired_source_is_drained_in_time_and_a_wired_one_is_left_alone() {
    let mut bay = Patchbay::default();
    let spotify = bay
        .add_source("Spotify", SourceKind::Spotify, None)
        .unwrap();
    let radio = bay.add_source("Radio", SourceKind::Voice, None).unwrap();
    let dir = std::env::temp_dir().join(format!(
        "endora-sound-drain-{}-{:?}",
        std::process::id(),
        Instant::now()
    ));
    std::fs::create_dir_all(&dir).unwrap();
    let mut feeds = FeedSet::new(Places {
        ffmpeg: PathBuf::from("ffmpeg"),
        pipes: dir.clone(),
    });
    feeds.keep(&bay, &[radio]);
    feeds.say(radio, &[3; 100]);
    let spotify_buffer = feeds.open.get(&spotify).unwrap().0.clone();
    lock(&spotify_buffer).push(&vec![1; PRIME]);
    feeds.discard_unheard(&[radio], 40);
    assert_eq!(
        lock(&spotify_buffer).len(),
        PRIME - 40,
        "the unwired source was not drained"
    );
    assert_eq!(
        feeds.take(radio, 100).len(),
        100,
        "a heard source was drained too"
    );
    let _ = std::fs::remove_dir_all(&dir);
}

/// A reader that hands out a counting sequence of samples, as fast as asked.
struct Counting(u16);

impl std::io::Read for Counting {
    fn read(&mut self, buf: &mut [u8]) -> std::io::Result<usize> {
        let mut n = 0;
        while n + 2 <= buf.len() {
            buf[n..n + 2].copy_from_slice(&self.0.to_le_bytes());
            self.0 = self.0.wrapping_add(1);
            n += 2;
        }
        Ok(n)
    }
}

#[test]
fn a_fast_writer_loses_nothing_when_taken_in_small_bites() {
    // The pump must read only as much as fits: a whole chunk into a nearly full buffer pushed
    // the oldest out, and the first live try ran at twice real time.
    let into: Shared = Arc::default();
    let stop = Arc::new(AtomicBool::new(false));
    let (i, st) = (into.clone(), stop.clone());
    let pumping = std::thread::spawn(move || pump(Counting(0), &i, &st));
    let mut next: Option<u16> = None;
    let mut taken = 0usize;
    let until = Instant::now() + Duration::from_millis(600);
    while Instant::now() < until {
        // About a tick's worth at a time, as the player takes.
        for s in lock(&into).take(1_764) {
            let s = s as u16;
            if let Some(want) = next {
                assert_eq!(s, want, "a gap after {taken} samples: some were dropped");
            }
            next = Some(s.wrapping_add(1));
            taken += 1;
        }
        std::thread::sleep(Duration::from_millis(2));
    }
    stop.store(true, Ordering::Relaxed);
    pumping.join().unwrap();
    assert!(taken > MOST, "too little went through to tell: {taken}");
}
