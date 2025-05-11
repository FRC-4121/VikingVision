use crate::buffer::Buffer;
use config::Config;
use polonius_the_crab::{ForLt, Placeholder, PoloniusResult, polonius};
use std::any::Any;
use std::fmt::{self, Debug, Formatter};
use std::io;
use std::time::Instant;
use tracing::{debug, error, info, info_span};

pub mod capture;
pub mod config;
pub mod frame;

pub trait CameraImpl: Any + Send + Sync {
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
    /// Create a new camera from a name and an implementation.
    pub fn new(name: String, inner: Box<dyn CameraImpl>) -> Self {
        Self {
            name,
            inner,
            fail_count: 0,
            backoff: 1,
            last_frame: Instant::now(),
        }
    }
    /// Get the config associated with the camera.
    pub fn config(&self) -> &dyn Config {
        self.inner.config()
    }
    /// Get the name of the camera.
    pub fn name(&self) -> &str {
        &self.name
    }
    /// Get a reference to the implementation.
    pub fn inner(&self) -> &dyn CameraImpl {
        &*self.inner
    }
    /// Get a mutable reference to the implementation.
    pub fn inner_mut(&mut self) -> &mut dyn CameraImpl {
        &mut *self.inner
    }
    /// Attempt to downcast the implementation to a concrete type.
    pub fn downcast_ref<T: CameraImpl>(&self) -> Option<&T> {
        let any = self.inner() as &dyn Any;
        any.downcast_ref()
    }
    /// Attempt to mutably downcast the implementation to a concrete type.
    pub fn downcast_mut<T: CameraImpl>(&mut self) -> Option<&mut T> {
        let any = self.inner_mut() as &mut dyn Any;
        any.downcast_mut()
    }
    /// Read a frame, reloading the camera if necessary.
    pub fn read(&mut self) -> io::Result<Buffer<'_>> {
        let _guard = info_span!("reading frame", name = self.name);
        let now = Instant::now();
        if let Some(to_sleep) = self.config().min_frame().checked_sub(now - self.last_frame) {
            debug!(?to_sleep, "sleeping to throttle framerate");
            std::thread::sleep(to_sleep);
            self.last_frame = now;
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
