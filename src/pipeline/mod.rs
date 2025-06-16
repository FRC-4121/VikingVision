use std::fmt::{self, Debug, Display, Formatter};
use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher, Hash};

pub mod component;
pub mod daemon;
pub mod runner;

/// A comparable ID for pipeline runs.
///
/// This can be used to help components hold state between runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PipelineId(pub u64);
impl PipelineId {
    /// Create a pipeline ID from a hashable value.
    pub fn from_hash(val: impl Hash) -> Self {
        Self(BuildHasherDefault::<DefaultHasher>::new().hash_one(val))
    }
    /// Create a pipeline ID form a pointer.
    ///
    /// This gives a different value from [`from_hash`](Self::from_hash) being used with a pointer argument.
    pub fn from_ptr(val: *const impl Sized) -> Self {
        Self(val as usize as u64)
    }
}
impl Display for PipelineId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:0>16x}", self.0)
    }
}

/// A pretty name for a pipeline run.
#[derive(Clone, Copy)]
pub struct PipelineName<'a>(pub &'a dyn Display);
impl Debug for PipelineName<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        struct AsDebug<'a>(&'a dyn Display);
        impl Debug for AsDebug<'_> {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                Display::fmt(self.0, f)
            }
        }
        f.debug_tuple("PipelineName")
            .field(&AsDebug(self.0))
            .finish()
    }
}
impl Display for PipelineName<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(self.0, f)
    }
}

/// Type tag for [`PipelineId`].
#[ty_tag::tag]
pub type PipelineIdTag = PipelineId;

/// Type tag for [`PipelineName`].
#[ty_tag::tag]
pub type PipelineNameTag<'a> = PipelineName<'a>;

pub mod prelude {
    pub use super::component::{Component, ComponentFactory, Data, Inputs, OutputKind};
    pub use super::runner::{ComponentContext, ComponentId, PipelineRunner, RunParams};
    pub use crate::utils::LogErr;
    pub use supply::prelude::*;

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
            fn output_kind(&self, name: Option<&str>) -> OutputKind {
                if name.is_none() {
                    OutputKind::Single
                } else {
                    OutputKind::None
                }
            }
            fn run<'s, 'r: 's>(&self, ctx: ComponentContext<'r, '_, 's>) {
                ctx.submit(None, Arc::new("data".to_string()));
            }
        }

        pub struct ConsumeComponent;
        impl Component for ConsumeComponent {
            fn inputs(&self) -> Inputs {
                Inputs::Primary
            }
            fn output_kind(&self, _: Option<&str>) -> OutputKind {
                OutputKind::None
            }
            fn run<'s, 'r: 's>(&self, _: ComponentContext<'r, '_, 's>) {}
        }

        pub fn produce_component() -> Arc<dyn Component> {
            Arc::new(ProduceComponent)
        }

        pub fn consume_component() -> Arc<dyn Component> {
            Arc::new(ConsumeComponent)
        }
    }
}
