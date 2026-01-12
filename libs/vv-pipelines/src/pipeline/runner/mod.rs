//! The pipeline runner, component context, and related traits and errors
#![allow(clippy::type_complexity)]

use super::component::{Component, Data};
use super::{ComponentChannel, ComponentId};
use smallvec::SmallVec;
use smol_str::SmolStr;
use std::collections::HashMap;
use std::fmt::{self, Debug, Display, Formatter};
use std::sync::Arc;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use thiserror::Error;
use vv_utils::mutex::Mutex;

mod context;
mod input;
mod run;

pub mod lazy_maps;

/// Alias for component IDs used in a [`PipelineRunner`].
pub type RunnerComponentId = ComponentId<PipelineRunner>;
/// Alias for component channels used in a [`PipelineRunner`].
pub type RunnerComponentChannel = ComponentChannel<PipelineRunner>;

#[cfg(test)]
mod tests;

pub use context::*;
pub use input::*;
pub use run::*;

mod trait_impls {
    use super::{PipelineRunner, RunnerComponentId};
    use crate::pipeline::{ComponentSpecifier, InvalidComponentId, UnknownComponentName};
    use smol_str::SmolStr;

    impl ComponentSpecifier<PipelineRunner> for RunnerComponentId {
        type Error = InvalidComponentId<PipelineRunner>;

        fn resolve(&self, runner: &PipelineRunner) -> Result<RunnerComponentId, Self::Error> {
            if self.is_placeholder() {
                return Err(InvalidComponentId(*self));
            }
            let this = self.unflagged();
            (this.index() < runner.components.len())
                .then_some(this)
                .ok_or(InvalidComponentId(this))
        }
    }
    impl ComponentSpecifier<PipelineRunner> for str {
        type Error = UnknownComponentName;

        fn resolve(&self, runner: &PipelineRunner) -> Result<RunnerComponentId, Self::Error> {
            runner
                .lookup
                .get(self)
                .copied()
                .ok_or_else(|| UnknownComponentName(self.into()))
        }
    }
    impl ComponentSpecifier<PipelineRunner> for String {
        type Error = UnknownComponentName;

        fn resolve(&self, runner: &PipelineRunner) -> Result<RunnerComponentId, Self::Error> {
            runner
                .lookup
                .get(self.as_str())
                .copied()
                .ok_or_else(|| UnknownComponentName(self.into()))
        }
    }
    impl ComponentSpecifier<PipelineRunner> for SmolStr {
        type Error = UnknownComponentName;

        fn resolve(&self, runner: &PipelineRunner) -> Result<RunnerComponentId, Self::Error> {
            runner
                .lookup
                .get(self)
                .copied()
                .ok_or_else(|| UnknownComponentName(self.clone()))
        }
    }
}

/// The compiled runner for a pipeline.
///
/// The pipeline runner is immutable, and a non-empty runner can only be created by compiling a pipeline graph.
/// See the [pipeline module documentation](super) for how to compile and run a runner.
#[derive(Debug, Default)]
pub struct PipelineRunner {
    pub(crate) components: Vec<ComponentData>,
    pub lookup: HashMap<SmolStr, RunnerComponentId>,
    pub(crate) running: AtomicUsize,
    pub(crate) run_id: AtomicU32,
}

impl PipelineRunner {
    /// Create a new, empty pipeline runner.
    #[inline(always)]
    pub fn empty() -> Self {
        Self {
            components: Vec::new(),
            lookup: HashMap::new(),
            running: AtomicUsize::new(0),
            run_id: AtomicU32::new(0),
        }
    }

    /// Get the number of currently running pipelines.
    #[inline(always)]
    pub fn running(&self) -> usize {
        self.running.load(Ordering::Relaxed)
    }

    /// Get the total number of pipeline runs.
    #[inline(always)]
    pub fn run_count(&self) -> u32 {
        self.run_id.load(Ordering::Relaxed)
    }
    /// Get the component data associated with an ID.
    #[inline(always)]
    pub fn component(&self, id: RunnerComponentId) -> Option<&ComponentData> {
        self.components.get(id.index())
    }
    /// Get the component storage for this runner.
    ///
    /// It can be indexed with the [`index`](ComponentId::index) function of a [`RunnerComponentId`].
    #[inline(always)]
    pub fn components(&self) -> &[ComponentData] {
        &self.components
    }
}
