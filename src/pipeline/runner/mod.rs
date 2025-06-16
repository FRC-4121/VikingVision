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
//! runner.add_dependency(component_a, None, component_b, None).unwrap();
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

/// A unique identifier for components within a [`PipelineRunner`].
///
/// ComponentId is a transparent wrapper around a `usize` that serves as an index into the
/// PipelineRunner's internal component storage. It's clearer than a raw index, and has a special value of `ComponentId::PLACEHOLDER`
/// to indicate an unassigned component.
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct ComponentId(pub usize);

impl ComponentId {
    /// A placeholder component, with a value equal to `usize::MAX`.
    pub const PLACEHOLDER: Self = Self(usize::MAX);
    /// Check if `self == Self::PLACEHOLDER`
    pub const fn is_placeholder(&self) -> bool {
        self.0 == usize::MAX
    }
    /// Opposite of [`is_placeholder`](Self::is_placeholder)
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
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
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

/// Uniquely identifies a specific execution of a component within the pipeline.
///
/// This is implemented as a sequence of invocation numbers where:
/// - The first number is the base run ID (from the initial pipeline trigger)
/// - Subsequent numbers represent nested or triggered executions
///
/// For example, a run ID of `1.2.3` indicates:
/// - This is the second pipeline run
/// - This is the third output from the first component that outputs multiple values
/// - From there, the next component that outputs multiple values output four.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId {
    /// The sequence of invocation numbers forming the execution path.
    /// This vector should never be empty.
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
/// #         fn run<'s, 'r: 's>(&self, ctx: ComponentContext<'r, '_, 's>) {
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
/// runner.add_dependency(input, None, process, None).unwrap();
///
/// // Run the pipeline
/// rayon::scope(|scope| {
///     runner.run((input, "initial data".to_string()), scope).unwrap();
/// });
/// ```
#[derive(Debug, Default)]
pub struct PipelineRunner {
    components: Vec<ComponentData>,
    lookup: HashMap<triomphe::Arc<str>, ComponentId>,
    running: AtomicUsize,
    run_id: AtomicU32,
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

    /// Get a map of registered component names to their IDs.
    #[inline(always)]
    pub fn component_lookup(&self) -> &HashMap<triomphe::Arc<str>, ComponentId> {
        &self.lookup
    }
    /// Get a slice of the components in this runner.
    #[inline(always)]
    pub fn component_slice(&self) -> &[ComponentData] {
        &self.components
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
    pub fn component(&self, id: ComponentId) -> Option<&Arc<dyn Component>> {
        self.components.get(id.0).map(|c| &c.component)
    }
    /// Get the chain of a component's multi-output nodes.
    ///
    /// This originally returns `start`, then the last multi-output component that indirectly leads to `start`,
    /// then the last multi-output component that indirectly leads to *that* node, and so on.
    pub fn branch_chain(&self, start: ComponentId) -> impl Iterator<Item = ComponentId> {
        std::iter::successors(Some(start), |&id| {
            let data = self.components.get(id.0)?;
            data.multi_input_from
        })
    }
}
