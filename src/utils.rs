use std::cell::UnsafeCell;
use std::collections::VecDeque;
use std::fmt::Display;
use std::mem::ManuallyDrop;
use std::sync::Once;
use std::time::{Duration, Instant};
use supply::prelude::*;

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
    pub fn set_max_duration(&mut self, duration: Duration) {
        if self.duration > duration {
            let now = Instant::now();
            while self
                .frames
                .front()
                .is_some_and(|&f| now.duration_since(f) > duration)
            {
                self.frames.pop_front();
            }
        }
        self.duration = duration;
    }
}

/// A convenience trait to log an error.
pub trait LogErr {
    /// Log an error with [`tracing`].
    fn log_err(&self);
    /// Log an error, then return self. This is for convenience with method chaining.
    fn and_log_err(self) -> Self
    where
        Self: Sized,
    {
        self.log_err();
        self
    }
}
impl<T, E: LogErr> LogErr for Result<T, E> {
    fn log_err(&self) {
        if let Err(err) = self {
            err.log_err();
        }
    }
}

pub trait Configure<C, S, A> {
    fn name(&self) -> impl Display {
        disqualified::ShortName::of::<Self>()
    }
    fn configure(&self, config: C, arg: A) -> S;
}
union ConfigurableInner<C, S> {
    config: ManuallyDrop<C>,
    state: ManuallyDrop<S>,
}
pub struct Configurable<C, S, T> {
    inner: UnsafeCell<ConfigurableInner<C, S>>,
    once: Once,
    def: T,
}
unsafe impl<C: Sync, S: Sync, T: Sync> Sync for Configurable<C, S, T> {}
impl<C, S, T> Drop for Configurable<C, S, T> {
    fn drop(&mut self) {
        unsafe {
            let mut drop_state = true;
            self.once.call_once_force(|s| {
                drop_state = false;
                if !s.is_poisoned() {
                    ManuallyDrop::drop(&mut self.inner.get_mut().config);
                }
            });
            if drop_state {
                ManuallyDrop::drop(&mut self.inner.get_mut().state);
            }
        }
    }
}
impl<C, S, T> Configurable<C, S, T> {
    pub const fn new(config: C, def: T) -> Self {
        Self {
            once: Once::new(),
            inner: UnsafeCell::new(ConfigurableInner {
                config: ManuallyDrop::new(config),
            }),
            def,
        }
    }
    pub fn get_config(&self) -> Option<&C> {
        let mut out = None;
        self.once.call_once_force(|s| {
            if !s.is_poisoned() {
                unsafe {
                    out = Some(&*(*self.inner.get()).config);
                }
            }
        });
        out
    }
    pub fn get_state(&self) -> Option<&S> {
        if self.once.is_completed() {
            Some(unsafe { &(*self.inner.get()).state })
        } else {
            tracing::error!(
                "tried to get data from an uninitialized {}",
                disqualified::ShortName::of::<T>()
            );
            None
        }
    }
    pub fn init<A>(&self, arg: A) -> bool
    where
        T: Configure<C, S, A>,
    {
        let mut ran = false;
        self.once.call_once(|| unsafe {
            let config = ManuallyDrop::take(&mut (*self.inner.get()).config);
            let state = self.def.configure(config, arg);
            (*self.inner.get()).state = ManuallyDrop::new(state);
            ran = true;
        });
        ran
    }
}
impl<C, S, T> Configurable<C, Option<S>, T> {
    /// Convenience function to flatten an `Option` state, since there isn't really a nice way to chain it.
    #[inline(always)]
    pub fn get_state_flat(&self) -> Option<&S> {
        self.get_state()?.as_ref()
    }
}

/// A [`Provider`] that doesn't supply any values.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoContext;
impl<'r> Provider<'r> for NoContext {
    type Lifetimes = l!['r];

    fn provide(&'r self, _want: &mut dyn Want<Self::Lifetimes>) {}
}
