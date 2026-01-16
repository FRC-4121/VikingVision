//! Pipeline running utilities
//!
//! A pipeline is a collection of components connected by channels. To create a pipeline, start with a [`PipelineGraph`],
//! and then [compile](PipelineGraph::compile) it to a [`PipelineRunner`] to run the pipeline.
//!
//! # Example
//!
//! ```rust
//! # use vv_pipelines::pipeline::prelude::for_test::{*, consume_component as process_component};
//! # use std::sync::Arc;
//! # fn input_component() -> Arc<dyn Component> {
//! #     pub struct EchoComponent;
//! #     impl Component for EchoComponent {
//! #         fn inputs(&self) -> Inputs {
//! #             Inputs::Primary
//! #         }
//! #         fn output_kind(&self, name: &str) -> OutputKind {
//! #             if name.is_empty() {
//! #                 OutputKind::Single
//! #             } else {
//! #                 OutputKind::None
//! #             }
//! #         }
//! #         fn run<'s, 'r: 's>(&self, ctx: ComponentContext<'_, 's, 'r>) {
//! #             if let Some(data) = ctx.get(None) {
//! #                 ctx.submit("", data);
//! #             }
//! #         }
//! #     }
//! #     Arc::new(EchoComponent)
//! # }
//! let mut graph = PipelineGraph::new();
//!
//! // Register components
//! let input = graph.add_named_component(input_component(), "input").unwrap();
//! let process = graph.add_named_component(process_component(), "process").unwrap();
//!
//! // Set up dependencies
//! graph.add_dependency(input, process).unwrap();
//!
//! let (resolver, runner) = graph.compile().unwrap();
//! let input = resolver[input];
//!
//! // Run the pipeline
//! rayon::scope(|scope| {
//!     runner.run((input, "initial data".to_string()), scope).unwrap();
//! });
//! ```

use smol_str::SmolStr;
use std::fmt::{self, Debug, Display, Formatter};
use std::hash::Hash;
use std::marker::PhantomData;
use thiserror::Error;

pub mod component;
pub mod graph;
pub mod runner;
pub mod serialized;

pub use graph::PipelineGraph;
pub use runner::PipelineRunner;
pub use serialized::SerializedGraph;

const IDX_MASK: usize = usize::MAX >> 1;
const FLAG_MASK: usize = !IDX_MASK;

/// A unique identifier for components within a [`PipelineRunner`].
///
/// ComponentId is a transparent wrapper around a `usize` that serves as an index into the
/// PipelineRunner's internal component storage. It's clearer than a raw index, and has a special value of `ComponentId::PLACEHOLDER`
/// to indicate an unassigned component.
#[repr(transparent)]
pub struct ComponentId<Marker> {
    pub raw: usize,
    pub _marker: PhantomData<Marker>,
}
impl<Marker> Clone for ComponentId<Marker> {
    fn clone(&self) -> Self {
        *self
    }
}
impl<Marker> Copy for ComponentId<Marker> {}
impl<Marker> PartialEq for ComponentId<Marker> {
    fn eq(&self, other: &Self) -> bool {
        self.raw == other.raw
    }
}
impl<Marker> Eq for ComponentId<Marker> {}
impl<Marker> PartialOrd for ComponentId<Marker> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<Marker> Ord for ComponentId<Marker> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.raw.cmp(&other.raw)
    }
}
impl<Marker> Hash for ComponentId<Marker> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        state.write_usize(self.raw);
    }
}

impl<Marker> ComponentId<Marker> {
    /// A placeholder component, with a value equal to `usize::MAX`.
    pub const PLACEHOLDER: Self = Self {
        raw: usize::MAX,
        _marker: PhantomData,
    };
    /// Check if `self == Self::PLACEHOLDER`
    #[inline(always)]
    pub const fn is_placeholder(&self) -> bool {
        self.raw == usize::MAX
    }
    /// Opposite of [`is_placeholder`](Self::is_placeholder)
    #[inline(always)]
    pub const fn is_valid(&self) -> bool {
        self.raw != usize::MAX
    }
    /// Get the value of a boolean flag stored here.
    #[inline(always)]
    pub const fn flag(&self) -> bool {
        self.is_valid() && self.raw & FLAG_MASK != 0
    }
    /// Get the value of the index, without the flag.
    #[inline(always)]
    pub const fn index(&self) -> usize {
        self.raw & IDX_MASK
    }
    /// Create a new `ComponentID` without a flag
    #[inline(always)]
    pub const fn new(index: usize) -> Self {
        debug_assert!(index < IDX_MASK, "value is out of range for a component ID");
        Self {
            raw: index & IDX_MASK,
            _marker: PhantomData,
        }
    }
    /// Create a flagged `ComponentId`.
    #[inline(always)]
    pub const fn new_flagged(index: usize) -> Self {
        debug_assert!(index < IDX_MASK, "value is out of range for a component ID");
        Self {
            raw: index | FLAG_MASK,
            _marker: PhantomData,
        }
    }
    /// Create an ID with the same index and a given flag.
    #[inline(always)]
    pub const fn with_flag(self, flag: bool) -> Self {
        if flag {
            self.flagged()
        } else {
            self.unflagged()
        }
    }
    /// Create a new ID with the flag set.
    #[inline(always)]
    pub const fn flagged(self) -> Self {
        Self {
            raw: self.raw | FLAG_MASK,
            _marker: PhantomData,
        }
    }
    /// Create a new ID without the flag set.
    #[inline(always)]
    pub const fn unflagged(self) -> Self {
        if self.is_valid() {
            Self {
                raw: self.raw & IDX_MASK,
                _marker: PhantomData,
            }
        } else {
            self
        }
    }
    /// Split this value into a flag and an unflagged ID.
    #[inline(always)]
    pub const fn decompose(self) -> (bool, Self) {
        (self.flag(), self.unflagged())
    }
    /// Change this ID to one of a different kind.
    pub fn transmute<NewMarker>(self) -> ComponentId<NewMarker> {
        ComponentId {
            raw: self.raw,
            _marker: PhantomData,
        }
    }
}

impl<Marker> Display for ComponentId<Marker> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if self.is_placeholder() {
            f.write_str("PLACEHOLDER")
        } else {
            write!(f, "#{}", self.index())
        }
    }
}

impl<Marker> Debug for ComponentId<Marker> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        #[derive(Debug)]
        #[allow(non_camel_case_types, clippy::upper_case_acronyms)]
        struct PLACEHOLDER;
        let mut f = f.debug_struct("ComponentId");
        if self.is_placeholder() {
            f.field("index", &PLACEHOLDER);
        } else {
            f.field("index", &self.index());
        }
        f.field("flag", &self.flag())
            .field("raw", &self.raw)
            .field("marker", &disqualified::ShortName::of::<Marker>())
            .finish()
    }
}
impl<Marker> Default for ComponentId<Marker> {
    fn default() -> Self {
        Self::PLACEHOLDER
    }
}

#[derive(Debug, Default)]
pub struct ComponentChannel<Marker>(pub ComponentId<Marker>, pub SmolStr);

impl<Marker> ComponentChannel<Marker> {
    pub const PLACEHOLDER: Self = Self(ComponentId::PLACEHOLDER, SmolStr::new_static(""));
    #[inline(always)]
    pub fn is_placeholder(&self) -> bool {
        self.0.is_placeholder()
    }
    #[inline(always)]
    pub fn is_valid(&self) -> bool {
        self.0.is_valid()
    }
    #[inline(always)]
    pub fn with_flag(self, flag: bool) -> Self {
        let Self(id, chan) = self;
        Self(id.with_flag(flag), chan)
    }
    #[inline(always)]
    pub fn decompose(self) -> (bool, Self) {
        let Self(id, chan) = self;
        let (flag, id) = id.decompose();
        (flag, Self(id, chan))
    }
}

impl<Marker> Clone for ComponentChannel<Marker> {
    fn clone(&self) -> Self {
        Self(self.0, self.1.clone())
    }
}
impl<Marker> PartialEq for ComponentChannel<Marker> {
    fn eq(&self, other: &Self) -> bool {
        self.0 == other.0 && self.1 == other.1
    }
}
impl<Marker> Eq for ComponentChannel<Marker> {}
impl<Marker> PartialOrd for ComponentChannel<Marker> {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}
impl<Marker> Ord for ComponentChannel<Marker> {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.0.cmp(&other.0).then_with(|| self.1.cmp(&other.1))
    }
}

impl<Marker> Display for ComponentChannel<Marker> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)?;
        if !self.1.is_empty() {
            write!(f, "/{}", self.1)
        } else {
            Ok(())
        }
    }
}

/// A type that can be resolved to a component ID.
///
/// This allows either component IDs or their names to be used.
pub trait ComponentSpecifier<T> {
    type Error;

    fn resolve(&self, container: &T) -> Result<ComponentId<T>, Self::Error>;
}
impl<T, S: ComponentSpecifier<T> + ?Sized> ComponentSpecifier<T> for &S {
    type Error = S::Error;

    #[inline(always)]
    fn resolve(&self, resolver: &T) -> Result<ComponentId<T>, Self::Error> {
        S::resolve(self, resolver)
    }
}

/// An error that occurs if a component ID can't be found
#[derive(Debug, Clone, PartialEq, Error)]
#[error("Component ID {0} doesn't point to a valid component")]
pub struct InvalidComponentId<T>(pub ComponentId<T>);

/// An error that occurs if a component can't be looked up by name
#[derive(Debug, Clone, PartialEq, Error)]
#[error("No component named {0:?}")]
pub struct UnknownComponentName(pub SmolStr);

pub mod prelude {
    pub use super::ComponentId;
    pub use super::component::{Component, ComponentFactory, Data, Inputs, OutputKind};
    pub use super::graph::{GraphComponentBuilder, GraphComponentId, PipelineGraph};
    pub use super::runner::{
        ComponentArgs, ComponentContext, InputIndex, InputTree, PipelineRunner, RunParams,
        RunnerComponentId,
    };
    #[cfg(feature = "supply")]
    pub use super::runner::{ProviderRef, ProviderTrait};
    #[cfg(feature = "supply")]
    pub use supply::prelude::*;
    pub use vv_utils::utils::LogErr;

    /// Useful components for pipeline doctests.
    #[doc(hidden)]
    pub mod for_test {
        pub use super::*;
        pub use std::sync::Arc;

        pub struct ProduceComponent;
        impl ProduceComponent {
            #[allow(clippy::new_without_default)]
            pub fn new() -> Self {
                Self
            }
        }
        impl Component for ProduceComponent {
            fn inputs(&self) -> Inputs {
                Inputs::none()
            }
            fn output_kind(&self, name: &str) -> OutputKind {
                if name.is_empty() {
                    OutputKind::Single
                } else {
                    OutputKind::None
                }
            }
            fn run<'s, 'r: 's>(&self, ctx: ComponentContext<'_, 's, 'r>) {
                ctx.submit("", Arc::new("data".to_string()));
            }
        }

        pub struct ConsumeComponent;
        impl Component for ConsumeComponent {
            fn inputs(&self) -> Inputs {
                Inputs::Primary
            }
            fn output_kind(&self, _: &str) -> OutputKind {
                OutputKind::None
            }
            fn run<'s, 'r: 's>(&self, _: ComponentContext<'_, 's, 'r>) {}
        }

        pub fn produce_component() -> Arc<dyn Component> {
            Arc::new(ProduceComponent)
        }

        pub fn consume_component() -> Arc<dyn Component> {
            Arc::new(ConsumeComponent)
        }
    }
}
