//! A pipeline execution system for managing and running interdependent components.
//!
//! The pipeline system manages component execution with dependency tracking and parallel
//! processing support. Components can be registered, connected through dependencies, and executed concurrently.
//!
//! # Example
//! ```rust
//! # use viking_vision::pipeline::prelude::for_test::{*, produce_component as process_image, consume_component as detect_features};
//! let mut runner = PipelineRunner::new();
//!
//! // Register components with unique names
//! let component_a = runner.add_component("image_processor", process_image()).unwrap();
//! let component_b = runner.add_component("feature_detector", detect_features()).unwrap();
//!
//! // Set up dependencies between components
//! runner.add_dependency(component_a, (), component_b, ()).unwrap();
//!
//! // Execute the pipeline using rayon's parallel execution
//! rayon::scope(|scope| {
//!     // Run the pipeline with initial input
//!     runner.run((component_a, "input data".to_string()), scope);
//!
//!     // Multiple pipeline runs can be executed in parallel
//!     runner.run((component_a, "different data".to_string()), scope);
//! });
//! ```

use super::component::{Component, Data};
use super::{ComponentChannel, ComponentId};
use smallvec::SmallVec;
use smol_str::SmolStr;
use std::collections::HashMap;
use std::fmt::{self, Debug, Display, Formatter};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;

mod context;
mod input;
mod run;

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

/// A pipeline execution system for managing interdependent components.
///
/// PipelineRunner manages component registration, dependencies, and parallel execution. Components
/// are stored internally and can be referenced by [`ComponentId`]s, with optional name-based lookup.
///
/// # Example
///
/// ```rust
/// # use viking_vision::pipeline::prelude::for_test::{*, consume_component as process_component};
/// # use std::sync::Arc;
/// # fn input_component() -> Arc<dyn Component> {
/// #     pub struct EchoComponent;
/// #     impl Component for EchoComponent {
/// #         fn inputs(&self) -> Inputs {
/// #             Inputs::Primary
/// #         }
/// #         fn output_kind(&self, name: Option<&str>) -> OutputKind {
/// #             if name.is_none() {
/// #                 OutputKind::Single
/// #             } else {
/// #                 OutputKind::None
/// #             }
/// #         }
/// #         fn run<'s, 'r: 's>(&self, ctx: ComponentContext<'_, 's, 'r>) {
/// #             if let Some(data) = ctx.get(None) {
/// #                 ctx.submit(None, data);
/// #             }
/// #         }
/// #     }
/// #     Arc::new(EchoComponent)
/// # }
/// let mut runner = PipelineRunner::new();
///
/// // Register components
/// let input = runner.add_component("input", input_component()).unwrap();
/// let process = runner.add_component("process", process_component()).unwrap();
///
/// // Set up dependencies
/// runner.add_dependency(input, (), process, ()).unwrap();
///
/// // Run the pipeline
/// rayon::scope(|scope| {
///     runner.run((input, "initial data".to_string()), scope).unwrap();
/// });
/// ```
#[derive(Debug, Default)]
pub struct PipelineRunner {
    pub(crate) components: Vec<ComponentData>,
    pub(crate) lookup: HashMap<SmolStr, RunnerComponentId>,
    pub(crate) running: AtomicUsize,
    pub(crate) run_id: AtomicU32,
}

impl PipelineRunner {
    /// Create a new empty pipeline runner.
    #[inline(always)]
    pub fn new() -> Self {
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
    /// Get the component associated with an ID.
    #[inline(always)]
    pub fn component(&self, id: RunnerComponentId) -> Option<&Arc<dyn Component>> {
        self.components.get(id.index()).map(|c| &c.component)
    }
}
