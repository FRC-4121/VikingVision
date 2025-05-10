use crate::buffer::Buffer;
use config::Config;
use polonius_the_crab::{ForLt, Placeholder, PoloniusResult, polonius};
use std::fmt::{self, Debug, Formatter};
use std::io;
use std::time::Instant;
use tracing::{debug, error, info, info_span};

pub mod config;

pub mod frame;

pub trait CameraImpl: Send + Sync {
    fn config(&self) -> &dyn Config;
    fn read_frame(&mut self) -> io::Result<Buffer<'_>>;

    /// Try to reload the camera. Return false if we should stop trying.
    fn reload(&mut self) -> bool {
        false
    }
    /// Debug this implementation to a formatter. This is used in the `Debug` impl for the trait.
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        disqualified::ShortName::of::<Self>().fmt(f)
    }
}
impl Debug for dyn CameraImpl {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.debug(f)
    }
}

#[derive(Debug)]
pub struct Camera {
    name: String,
    inner: Box<dyn CameraImpl>,
    fail_count: usize,
    backoff: usize,
    last_frame: Instant,
}
impl Camera {
    pub fn new(name: String, inner: Box<dyn CameraImpl>) -> Self {
        Self {
            name,
            inner,
            fail_count: 0,
            backoff: 1,
            last_frame: Instant::now(),
        }
    }
    pub fn config(&self) -> &dyn Config {
        self.inner.config()
    }
    pub fn name(&self) -> &str {
        &self.name
    }
    pub fn read(&mut self) -> io::Result<Buffer<'_>> {
        let _guard = info_span!("reading frame", name = self.name);
        let now = Instant::now();
        if let Some(to_sleep) = (now - self.last_frame).checked_sub(self.config().min_frame()) {
            debug!(?to_sleep, "sleeping to throttle framerate");
            std::thread::sleep(to_sleep);
        }
        // dirty hack because Rust's borrow checker doesn't quite support non-lexical lifetimes
        match polonius::<_, _, ForLt!(Buffer<'_>)>(&mut self.inner, |inner| {
            match inner.read_frame() {
                Ok(frame) => PoloniusResult::Borrowing(frame),
                Err(err) => PoloniusResult::Owned {
                    value: err,
                    input_borrow: Placeholder,
                },
            }
        }) {
            PoloniusResult::Borrowing(frame) => Ok(frame),
            PoloniusResult::Owned {
                value: err,
                input_borrow: inner,
            } => {
                self.fail_count += 1;
                error!(%err, fail_count = self.fail_count, "failed to read frame");
                if self.fail_count == self.backoff {
                    info!("reloading camera");
                    let _guard = info_span!("reloading");
                    let retry = inner.reload(); // this line here is problematic because Rust can't reason that this branch is only reachable on the error branch, so `self.inner` isn't borrowed
                    if retry {
                        self.backoff *= 2;
                    } else {
                        self.backoff = 0;
                    }
                }
                Err(err)
            }
        }
    }
}
