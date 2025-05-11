use std::collections::VecDeque;
use std::time::{Duration, Instant};

/// A simple counter to find the average framerate over a period of time
#[derive(Debug, Clone)]
pub struct FpsCounter {
    duration: Duration,
    frames: VecDeque<Instant>,
}
/// The default duration is 5 seconds.
impl Default for FpsCounter {
    fn default() -> Self {
        Self::new(Duration::from_secs(5))
    }
}
impl FpsCounter {
    /// Create a new counter with the given duration.
    pub const fn new(duration: Duration) -> Self {
        Self {
            duration,
            frames: VecDeque::new(),
        }
    }
    /// Record a frame with the given timestamp.
    pub fn tick_at(&mut self, now: Instant) {
        while let Some(&frame) = self.frames.front() {
            if now.duration_since(frame) > self.duration {
                self.frames.pop_front();
            } else {
                break;
            }
        }
        self.frames.push_back(now);
    }
    /// Record a frame with the current timestamp.
    pub fn tick(&mut self) {
        self.tick_at(Instant::now());
    }
    /// Count the number of frames within the duration.
    pub fn frames(&self) -> usize {
        self.frames.len()
    }
    /// Get the longest and shortest frames, or `None` if less than two frames have been recorded.
    pub fn minmax_frames(&self) -> Option<[Duration; 2]> {
        let mut it = self.frames.iter();
        let first = *it.next()?;
        let mut last = *it.next()?;
        let mut min = last - first;
        let mut max = min;
        for &frame in it {
            let dur = frame - last;
            last = frame;
            if dur > max {
                max = dur;
            }
            if dur < min {
                min = dur;
            }
        }
        Some([min, max])
    }
    /// Get the minimum and maximum frame*rates*, or `None` if less than two frames have been recorded.
    pub fn minmax(&self) -> Option<[f64; 2]> {
        self.minmax_frames().and_then(|[min, max]| {
            (!(min.is_zero() || max.is_zero()))
                .then(|| [max.as_secs_f64().recip(), min.as_secs_f64().recip()])
        })
    }
    pub fn fps(&self) -> f64 {
        self.frames.len() as f64 / self.duration.as_secs_f64()
    }
}
