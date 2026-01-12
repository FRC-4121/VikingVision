use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};
use std::thread::JoinHandle;
use std::{io, thread};
use tracing::{error, warn};

/// Context to be shared between the thread and its handle
#[derive(Debug)]
pub struct SharedContext<T> {
    /// The current state of the thread. Note that if the thread is in the `PAUSED` state, it will park and needs to be unparked.
    /// See the associated constants for the different states.
    pub run_state: AtomicU8,
    pub context: T,
}
/// Recognized states for the deamon thread
pub mod states {
    /// The thread isn't doing any work, and is likely parked.
    pub const PAUSED: u8 = 0;
    /// The thread is actively doing work.
    pub const RUNNING: u8 = 1;
    /// The thread is shutting down or has done so already.
    pub const SHUTDOWN: u8 = 2;
}

/// Some kind of worker to run on a daemon, that can take a given task
pub trait Worker<T> {
    /// Get a name for the thread
    fn name(&self) -> String;
    /// Execute one "step" of work
    fn work(&mut self, context: &T);
    /// Gracefully clean up any resources after a shutdown was requested
    #[allow(unused_variables)]
    fn cleanup(&mut self, context: &T) {}
}

/// A handle to some thread made to run in the background with utilities to manage its running
#[derive(Debug)]
pub struct DaemonHandle<T> {
    context: Arc<SharedContext<T>>,
    handle: JoinHandle<()>,
}
impl<T: Send + Sync + 'static> DaemonHandle<T> {
    /// Create a new handle with an initial state and a given worker.
    pub fn new<W: Worker<T> + Send + 'static>(init_ctx: T, mut worker: W) -> io::Result<Self> {
        let context = Arc::new(SharedContext {
            run_state: AtomicU8::new(states::PAUSED),
            context: init_ctx,
        });
        let ctx = context.clone();
        let handle = thread::Builder::new()
            .name(worker.name())
            .spawn(move || {
                loop {
                    let state = ctx.run_state.load(Ordering::Acquire);
                    match state {
                        states::SHUTDOWN => break,
                        states::PAUSED => {
                            thread::park();
                            continue;
                        }
                        _ => {}
                    }
                    worker.work(&ctx.context);
                }
                worker.cleanup(&ctx.context);
            })
            .inspect_err(|err| error!(%err, "failed to start worker thread"))?;
        Ok(Self { context, handle })
    }
    /// Get the shared context for the worker.
    pub fn context(&self) -> &Arc<SharedContext<T>> {
        &self.context
    }
    /// Start the worker, unparking its thread if necessary.
    pub fn start(&self) {
        let old = self
            .context
            .run_state
            .swap(states::RUNNING, Ordering::Release);
        match old {
            states::PAUSED => self.handle.thread().unpark(),
            states::SHUTDOWN => warn!("attempted to call shut down daemon"),
            _ => {}
        }
    }
    /// Pause the worker. This will park it.
    pub fn pause(&self) {
        let old = self
            .context
            .run_state
            .swap(states::PAUSED, Ordering::Release);
        if old == states::SHUTDOWN {
            warn!("attempted to call shut down daemon");
        }
    }
    /// Tell the worker to gracefully shut down.
    pub fn shutdown(&self) {
        self.context
            .run_state
            .store(states::SHUTDOWN, Ordering::Release);
        self.handle.thread().unpark();
    }
    /// Return whether or not the thread has completely finished.
    pub fn is_finished(&self) -> bool {
        self.handle.is_finished()
    }
}
