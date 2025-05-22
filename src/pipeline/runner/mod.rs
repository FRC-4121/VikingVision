use super::component::{Component, Data};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt::{self, Debug, Display, Formatter};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;

mod deps;
mod input;
mod run;

pub use deps::*;
pub use input::*;
pub use run::*;

/// Newtype wrapper around a component ID.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct ComponentId(pub usize);
impl ComponentId {
    pub const PLACEHOLDER: Self = Self(usize::MAX);
    pub const fn is_placeholder(&self) -> bool {
        self.0 == usize::MAX
    }
    pub const fn is_valid(&self) -> bool {
        self.0 != usize::MAX
    }
}
impl Display for ComponentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.is_placeholder() {
            f.write_str("PLACEHOLDER")
        } else {
            write!(f, "#{}", self.0)
        }
    }
}
impl Debug for ComponentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        #[derive(Debug)]
        #[allow(non_camel_case_types)]
        struct PLACEHOLDER;
        let mut tuple = f.debug_tuple("ComponentId");
        if self.is_placeholder() {
            tuple.field(&PLACEHOLDER);
        } else {
            tuple.field(&self.0);
        }
        tuple.finish()
    }
}

/// A unique identifier for which set of inputs a component's being run on.
///
/// It's guaranteed that every time a [`PipelineRunner`] runs a component, this value will be different.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId {
    /// Which combination of outputs this is. This should never be empty.
    pub invocs: SmallVec<[u32; 2]>,
}
impl RunId {
    pub fn new(invoc: u32) -> Self {
        Self {
            invocs: smallvec::smallvec![invoc],
        }
    }
    pub fn starts_with(&self, other: &RunId) -> bool {
        self.invocs.starts_with(&other.invocs)
    }
    pub fn push(&mut self, val: u32) {
        self.invocs.push(val);
    }
    pub fn base_run(&self) -> u32 {
        self.invocs[0]
    }
}
impl Display for RunId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let Some((head, tail)) = self.invocs.split_first() else {
            return Ok(());
        };
        write!(f, "{head}")?;
        for elem in tail {
            write!(f, ".{elem}")?;
        }
        Ok(())
    }
}

/// A callback function to be called after a pipeline completes.
pub type Callback<'a> = Box<dyn FnOnce(&'a PipelineRunner) + Send + Sync + 'a>;

/// The core runner for vision pipelines.
///
/// Note that in order for the lifetimes to work, this should be defined outside of the call to [`rayon::scope`].
///
/// ```ignore
/// let mut runner = PipelineRunner::new();
/// // It's best to do initialization here.
/// let component_a = runner.add_component("A", component_a()).unwrap();
/// let component_b = runner.add_component("B", component_b()).unwrap();
/// runner.add_dependency(component_a, None, component_b, None).unwrap();
/// rayon::scope(|scope| {
///     // Initialization *can* be done here, but probably shouldn't unless the scope is needed.
///     runner.run(component_a, Arc::new("input data".to_string()), scope);
///     // the call to run() immutably borrows the runner for the whole scope, so it can't be configured more here
///     runner.run(componet_a, Arc::new("different data".to_string()), scope); // You can, however, run more pipelines from here
/// });
/// // By the time the call to `scope` returns, all of the pipelines will have run.
/// ```
#[derive(Debug, Default)]
pub struct PipelineRunner {
    components: Vec<ComponentData>,
    lookup: HashMap<triomphe::Arc<str>, ComponentId>,
    running: AtomicUsize,
    run_id: AtomicU32,
}
impl PipelineRunner {
    /// Create a new runner with no components.
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            components: Vec::new(),
            lookup: HashMap::new(),
            running: AtomicUsize::new(0),
            run_id: AtomicU32::new(0),
        }
    }
    /// Get a map from the registered component names to their IDs.
    #[inline(always)]
    pub fn components(&self) -> &HashMap<triomphe::Arc<str>, ComponentId> {
        &self.lookup
    }
    /// Get the number of running pipelines.
    #[inline(always)]
    pub fn running(&self) -> usize {
        self.running.load(Ordering::Relaxed)
    }
    /// Get the number of times [`run`](Self::run) has been called.
    #[inline(always)]
    pub fn run_count(&self) -> u32 {
        self.run_id.load(Ordering::Relaxed)
    }
}
