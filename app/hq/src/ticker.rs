//! HQ's tick (ADR 0102, amended 2026-11-04): a pass of the engine every `every` seconds, and one
//! more as soon as the engine asks for it (the owner's new work), so that work is taken up at
//! once rather than at the next tick. Passes never overlap: one runs after the other ends.
//! Asking is coalesced: however many asks arrive before a pass starts, that pass serves them all,
//! and asks made while a pass runs bring one more pass after it. The clock's own ticks keep their
//! cadence, and nothing passes while nobody asks.

use std::future::Future;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use tokio::sync::Notify;

/// The engine's asks for a pass, as the ticker hears them.
#[derive(Default)]
pub struct PassAsked {
    /// Whether a pass has been asked for since the last one started.
    asked: AtomicBool,
    /// Wakes the ticker; holds one wake when it is not waiting.
    wake: Notify,
}

impl PassAsked {
    /// Asks for a pass now. Cheap and never blocks: the engine calls it with its lock released.
    pub fn ask(&self) {
        self.asked.store(true, Ordering::SeqCst);
        self.wake.notify_one();
    }
}

/// Runs `pass` on the clock, every `every`, and whenever `asked` asks; one at a time, forever.
pub async fn run<F, Fut>(every: Duration, asked: Arc<PassAsked>, mut pass: F)
where
    F: FnMut() -> Fut,
    Fut: Future<Output = ()>,
{
    let mut clock = tokio::time::interval(every);
    clock.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
    loop {
        tokio::select! {
            _ = clock.tick() => {}
            () = asked.wake.notified() => {
                // A wake left over from an ask the last pass already served: nothing to do.
                if !asked.asked.load(Ordering::SeqCst) {
                    continue;
                }
            }
        }
        // This pass serves every ask made before it starts.
        asked.asked.store(false, Ordering::SeqCst);
        pass().await;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;
    use tokio::time::Instant;

    /// When each pass started, in seconds since the test began.
    type Passes = Arc<Mutex<Vec<f64>>>;

    /// A ticker every 30 s whose passes each take `takes`, recording when they start, and the
    /// instant the test began.
    fn ticker(takes: Duration) -> (Arc<PassAsked>, Passes, Instant) {
        let asked = Arc::new(PassAsked::default());
        let passes: Passes = Arc::default();
        let (kept, start) = (passes.clone(), Instant::now());
        let heard = asked.clone();
        tokio::spawn(run(Duration::from_secs(30), heard, move || {
            kept.lock().unwrap().push(start.elapsed().as_secs_f64());
            tokio::time::sleep(takes)
        }));
        (asked, passes, start)
    }

    /// Waits until `seconds` after the test began.
    async fn at(start: Instant, seconds: f64) {
        tokio::time::sleep_until(start + Duration::from_secs_f64(seconds)).await;
    }

    fn starts(passes: &Passes) -> Vec<f64> {
        passes.lock().unwrap().iter().map(|s| s.round()).collect()
    }

    #[tokio::test(start_paused = true)]
    async fn idle_it_passes_only_on_its_clock() {
        let (_asked, passes, start) = ticker(Duration::from_millis(100));
        at(start, 95.0).await;
        assert_eq!(starts(&passes), vec![0.0, 30.0, 60.0, 90.0]);
    }

    #[tokio::test(start_paused = true)]
    async fn an_ask_between_ticks_passes_at_once_and_keeps_the_clock() {
        let (asked, passes, start) = ticker(Duration::from_millis(100));
        at(start, 7.0).await;
        asked.ask();
        at(start, 31.0).await;
        assert_eq!(starts(&passes), vec![0.0, 7.0, 30.0]);
    }

    #[tokio::test(start_paused = true)]
    async fn asks_together_bring_one_pass() {
        let (asked, passes, start) = ticker(Duration::from_millis(100));
        at(start, 12.0).await;
        for _ in 0..5 {
            asked.ask();
        }
        at(start, 29.0).await;
        assert_eq!(starts(&passes), vec![0.0, 12.0]);
    }

    #[tokio::test(start_paused = true)]
    async fn asks_during_a_pass_bring_one_more_after_it() {
        let (asked, passes, start) = ticker(Duration::from_secs(4));
        at(start, 1.0).await; // the first pass runs 0 s to 4 s
        asked.ask();
        asked.ask();
        at(start, 20.0).await;
        assert_eq!(starts(&passes), vec![0.0, 4.0]);
    }

    #[tokio::test(start_paused = true)]
    async fn an_ask_at_a_tick_brings_no_second_pass() {
        let (asked, passes, start) = ticker(Duration::from_millis(100));
        at(start, 30.0).await; // the clock's pass at 30 s has started
        asked.ask();
        at(start, 59.0).await;
        // The ask came after the pass at 30 s began, so one pass more serves it; no others.
        assert!(starts(&passes).len() <= 3, "{:?}", starts(&passes));
        assert_eq!(starts(&passes)[..2], [0.0, 30.0]);
    }
}
