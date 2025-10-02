//! Definition of the [`Component`] trait
//!
//! See the documentation for [`Component`] for more information on implementation.

use super::runner::ComponentContext;
use crate::buffer::Buffer;
use crate::pipeline::graph::{GraphComponentId, IdResolver, PipelineGraph};
use crate::utils::LogErr;
use std::any::{Any, TypeId};
use std::fmt::{self, Debug, Display, Formatter};
use std::sync::{Arc, Mutex, TryLockError};
use supply::prelude::*;
use thiserror::Error;

/// A pretty error for when downcasts fail.
#[derive(Debug, Clone, Copy, Error)]
#[error("Couldn't downcast data to {expected}")]
pub struct TypeMismatch<A> {
    /// The actual type ID
    pub id: TypeId,
    /// The name of the expected type
    pub expected: disqualified::ShortName<'static>,
    /// Additional data to be passed along with this, an [`Arc<dyn Data>`] in the case of `Data::downcast_arc`.
    pub additional: A,
}
impl<A> LogErr for TypeMismatch<A> {
    fn log_err(&self) {
        tracing::error!(id = ?self.id, "couldn't downcast data to {}", self.expected);
    }
}

/// A trait representing data that can be passed between pipeline components.
///
/// This trait is automatically implemented for common types but is opt-in, which is better for
/// certain generic stuff done in other places.
///
/// # Implemented Types
/// - Primitive types: `bool`, integers, and floats
/// - `String`
/// - `Buffer`
/// - `Vec<T>` where T: Data
/// - Tuples up to 12 elements where each element implements Data
///
/// # Example
///
/// ```rust
/// use std::sync::Arc;
/// use viking_vision::pipeline::component::Data;
///
/// // Primitive types implement Data
/// let num: Arc<dyn Data> = Arc::new(42i32);
///
/// // Custom types can implement Data
/// #[derive(Debug)]
/// struct MyData(String);
/// impl Data for MyData {
///     // Optionally override debug for custom formatting
///     fn debug(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
///         write!(f, "MyData({})", self.0)
///     }
/// }
/// ```
pub trait Data: Any + Send + Sync {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Display::fmt(&disqualified::ShortName::of::<Self>(), f)
    }
}
impl Debug for dyn Data {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.debug(f)
    }
}
impl dyn Data {
    pub fn downcast<T: Data>(&self) -> Result<&T, TypeMismatch<()>> {
        let any: &dyn Any = self;
        any.downcast_ref().ok_or_else(|| TypeMismatch {
            id: any.type_id(),
            expected: disqualified::ShortName::of::<T>(),
            additional: (),
        })
    }
    pub fn downcast_arc<T: Data>(self: Arc<Self>) -> Result<Arc<T>, TypeMismatch<Arc<Self>>> {
        let any: &dyn Any = &*self;
        let id = any.type_id();
        if id == TypeId::of::<T>() {
            Arc::downcast(self).map_err(|_| unreachable!())
        } else {
            Err(TypeMismatch {
                id,
                expected: disqualified::ShortName::of::<T>(),
                additional: self,
            })
        }
    }
}

/// A type that can be converted into a [`Arc<dyn Data>`]
pub trait IntoData {
    fn into_data(self) -> Arc<dyn Data>;
}
impl<T: Data> IntoData for T {
    fn into_data(self) -> Arc<dyn Data> {
        Arc::new(self)
    }
}
impl<T: Data> IntoData for Arc<T> {
    fn into_data(self) -> Arc<dyn Data> {
        self
    }
}
impl IntoData for Arc<dyn Data> {
    fn into_data(self) -> Arc<dyn Data> {
        self
    }
}
macro_rules! impl_via_debug {
    () => {};
    ($ty:ty $(, $rest:ty)*) => {
        impl Data for $ty {
            fn debug(&self, f: &mut Formatter) -> fmt::Result {
                Debug::fmt(self, f)
            }
        }
        impl_via_debug!($($rest),*);
    };
}
impl_via_debug!(
    (),
    bool,
    i8,
    i16,
    i32,
    i64,
    isize,
    u8,
    u16,
    u32,
    u64,
    usize,
    f32,
    f64,
    String,
    Buffer<'static>
);
impl<T: Data> Data for Vec<T> {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_list()
            .entries(self.iter().map(|e| e as &dyn Data))
            .finish()
    }
}
impl<T: Data> Data for Mutex<T> {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        let mut d = f.debug_struct("Mutex");
        match self.try_lock() {
            Ok(guard) => {
                d.field("data", &(&*guard as &dyn Data) as &dyn Debug);
            }
            Err(TryLockError::Poisoned(err)) => {
                d.field("data", &(&**err.get_ref() as &dyn Data) as &dyn Debug);
            }
            Err(TryLockError::WouldBlock) => {
                d.field("data", &format_args!("<locked>"));
            }
        }
        d.field("poisoned", &self.is_poisoned());
        d.finish_non_exhaustive()
    }
}
macro_rules! impl_for_tuple {
    () => {};
    ($head:ident $(, $tail:ident)*) => {
        impl<$head: Data, $($tail: Data,)*> Data for ($head, $($tail,)*) {
            #[allow(non_snake_case)]
            fn debug(&self, f: &mut Formatter) -> fmt::Result {
                let mut tuple = f.debug_tuple("");
                let ($head, $($tail,)*) = self;
                tuple.field(&($head as &dyn Data));
                $(tuple.field(&($tail as &dyn Data));)*
                tuple.finish()
            }
        }
        impl_for_tuple!($($tail),*);
    };
}
impl_for_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);

/// A serializable factory that can build a component.
///
/// This is useful for serialization and deserialization of components, but isn't required for their use in pipelines.
#[typetag::serde(tag = "type")]
pub trait ComponentFactory {
    fn build(&self, ctx: &mut dyn ProviderDyn) -> Box<dyn Component>;
}

/// What will come from an output channel.
///
/// It's the component's responsibility to adhere to this. The aren't any checks in place to prevent a component
/// from submitting multiple values to a channel reported to only send one, but it'll likely lead to some kind of error.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutputKind {
    /// There's no channel associated with the given output.
    None,
    /// Only one output will be sent per input.
    Single,
    /// Multiple outputs can be sent from a single input.
    ///
    /// If it's possible that multiple could be returned, then this should always be chosen.
    Multiple,
}
impl OutputKind {
    #[inline(always)]
    pub const fn is_none(&self) -> bool {
        matches!(self, Self::None)
    }
    #[inline(always)]
    pub const fn is_some(&self) -> bool {
        !self.is_none()
    }
    #[inline(always)]
    pub const fn is_multi(&self) -> bool {
        matches!(self, Self::Multiple)
    }
}

/// The kind of inputs that this component is expecting, returned fron [`Component::inputs`].
#[derive(Debug, Clone, PartialEq)]
pub enum Inputs {
    /// This component takes inputs through its primary input.
    Primary,
    /// This component takes multiple, named inputs (or none at all).
    Named(Vec<smol_str::SmolStr>),
}
impl Inputs {
    /// `Named(Vec::new())` means a component taktes no inputs; this is just an alias for that.
    #[inline(always)]
    pub const fn none() -> Self {
        Self::Named(Vec::new())
    }
    pub fn named<S: Into<smol_str::SmolStr>, I: IntoIterator<Item = S>>(iter: I) -> Self {
        Self::Named(iter.into_iter().map(Into::into).collect())
    }
}

/// A component that can be used in a vision processing pipeline.
///
/// Components are the building blocks of the pipeline system. Each component
/// can have inputs, produce outputs, and runs independently within the pipeline.
///
/// # Example
///
/// ```rust
/// use viking_vision::pipeline::prelude::*;
/// use viking_vision::buffer::Buffer;
/// use std::sync::Arc;
/// # fn process_image(_: &Buffer<'static>) {}
/// struct ImageProcessor;
///
/// impl Component for ImageProcessor {
///     fn inputs(&self) -> Inputs {
///         Inputs::named(["image"])
///     }
///
///     fn output_kind(&self, name: &str) -> OutputKind {
///         match name {
///             "" => OutputKind::Single,
///             "debug" => OutputKind::Multiple,
///             _ => OutputKind::None,
///         }
///     }
///
///     fn run<'s, 'r: 's>(&self, ctx: ComponentContext<'_, 's, 'r>) {
///         let Ok(image) = ctx.get_as::<Buffer<'static>>("image").and_log_err() else { return };
///         
///         // Process image...
///         let result = process_image(&image);
///         
///         // Submit result
///         ctx.submit("", Arc::new(result));
///     }
/// }
/// ```
#[allow(unused_variables)]
pub trait Component: Send + Sync + 'static {
    /// Get the inputs that this component is expecting.
    ///
    /// This should specify the values that a component *needs* in order to run.
    fn inputs(&self) -> Inputs;
    /// Check if this component can take an additional input.
    ///
    /// This is only called if an input wasn't specified as an input through [`inputs`](Self::inputs).
    fn can_take(&self, input: &str) -> bool {
        false
    }
    /// Check if an output channel is available.
    fn output_kind(&self, name: &str) -> OutputKind;
    /// Run a component on a given input.
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>);
    /// Perform startup initialization on this component.
    fn initialize(&self, graph: &mut PipelineGraph, self_id: GraphComponentId) {}
    /// Remap any indices on compilation, if necessary.
    fn remap(&self, resolver: &IdResolver) {}
}

/// Get the output of a component for a channel.
///
/// This wraps [`Component::output_kind`] but overrides the channel for special output channels (currently just `$finish`).
pub fn component_output(component: &dyn Component, channel: &str) -> OutputKind {
    match channel {
        "$finish" => OutputKind::Single,
        _ => component.output_kind(channel),
    }
}
