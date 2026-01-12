//! Definition of the [`Component`] trait
//!
//! See the documentation for [`Component`] for more information on implementation.

use super::runner::ComponentContext;
use crate::pipeline::graph::{GraphComponentId, IdResolver, PipelineGraph};
use smol_str::SmolStr;
use std::any::{Any, TypeId};
use std::borrow::Cow;
use std::fmt::{self, Debug, Display, Formatter};
use std::sync::Mutex;
use std::sync::{Arc, TryLockError};
use thiserror::Error;
use vv_utils::utils::LogErr;

#[cfg(feature = "apriltag")]
mod impl_apriltag;
mod impl_utils;
#[cfg(feature = "vision")]
mod impl_vision;

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
    #[track_caller]
    fn log_err(&self) {
        let location = std::panic::Location::caller();
        tracing::error!(
            id = ?self.id,
            "source.file" = location.file(),
            "source.line" = location.line(),
            "couldn't downcast data to {}",
            self.expected
        );
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
/// use vv_pipelines::pipeline::component::Data;
///
/// // Primitive types implement Data
/// let num: Arc<dyn Data> = Arc::new(42i32);
///
/// // Custom types can implement Data
/// #[derive(Debug, Clone)]
/// struct MyData(String);
/// impl Data for MyData {
///     // Optionally override debug for custom formatting
///     fn debug(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
///         write!(f, "MyData({})", self.0)
///     }
///     fn clone_to_arc(&self) -> Arc<dyn Data> {
///         Arc::new(self.clone())
///     }
/// }
/// ```
pub trait Data: Any + Send + Sync {
    fn type_name(&self) -> &'static str {
        std::any::type_name_of_val(self)
    }
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Display::fmt(&disqualified::ShortName::of::<Self>(), f)
    }
    fn clone_to_arc(&self) -> Arc<dyn Data>;
    #[allow(unused_variables)]
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        None
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &[]
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
impl ToOwned for dyn Data {
    type Owned = Arc<dyn Data>;
    fn to_owned(&self) -> Self::Owned {
        self.clone_to_arc()
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
            fn clone_to_arc(&self) -> Arc<dyn Data> {
                Arc::new(*self)
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
    f64
);
impl Data for String {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(&self, f)
    }
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(self.clone())
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        (field == "len").then(|| Cow::Owned(Arc::new(self.len()) as _))
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["len"]
    }
}
impl<T: Data + Clone> Data for Vec<T> {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_list()
            .entries(self.iter().map(|e| e as &dyn Data))
            .finish()
    }
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(self.clone())
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        (field == "len").then(|| Cow::Owned(Arc::new(self.len()) as _))
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["len"]
    }
}
impl<T: Data + Clone> Data for Mutex<T> {
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
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        let poisoned = self.is_poisoned();
        let inner = self.lock().unwrap_or_else(|e| e.into_inner()).clone();
        let new = Mutex::new(inner);
        if poisoned {
            // to poison the new mutex, we trigger an unwind past its guard.
            // we use resume_unwind because we don't want to trigger any panic handlers,
            // and an empty payload to avoid allocation-- as far as anyone else is concerned,
            // this panic didn't happen.
            // we can be fairly confident that this won't abort the program because our panics
            // have to unwind in order for the mutex to be poisoned in the first place. It's possible
            // that the mode was set to abort afterwards, but that's unlikely.
            let _ = std::panic::catch_unwind(|| {
                let _guard = new.lock();
                std::panic::resume_unwind(Box::new(()));
            });
        }
        Arc::new(new)
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        (field == "inner").then(|| {
            Cow::Owned(
                self.lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .clone_to_arc(),
            )
        })
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["inner"]
    }
}
static NUMS: &[&str] = &["0", "1", "2", "3", "4", "5", "6", "7", "8", "9", "10", "11"];
macro_rules! make_1 {
    ($ign:ident) => {
        1
    };
}
macro_rules! extract {
    ($this:expr, $val:ident; $($fields:tt),*;) => {};
    ($this:expr, $val:ident; $field:tt $(, $fields:tt)*; $id:ident $(, $ids:ident)*) => {
        if $val == stringify!($field) {
            return Some(Cow::Borrowed(&$this.$field));
        }
        extract!($this, $val; $($fields),*; $($ids),*);
    };
}
macro_rules! impl_for_tuple {
    () => {};
    ($head:ident $(, $tail:ident)*) => {
        impl<$head: Data + Clone, $($tail: Data + Clone,)*> Data for ($head, $($tail,)*) {
            #[allow(non_snake_case)]
            fn debug(&self, f: &mut Formatter) -> fmt::Result {
                let mut tuple = f.debug_tuple("");
                let ($head, $($tail,)*) = self;
                tuple.field(&($head as &dyn Data));
                $(tuple.field(&($tail as &dyn Data));)*
                tuple.finish()
            }
            fn clone_to_arc(&self) -> Arc<dyn Data> {
                Arc::new(self.clone())
            }
            fn known_fields(&self) -> &'static [&'static str] {
                const LEN: usize = 1 $(+ make_1!($tail))*;
                &NUMS[..LEN]
            }
            fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
                extract!(self, field; 0, 1, 2, 3, 4, 5, 6, 7, 8, 9, 10, 11, 12; $head $(, $tail)*);
                None
            }
        }
        impl_for_tuple!($($tail),*);
    };
}
impl_for_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);

/// A serializable factory that can build a component.
///
/// This is useful for serialization and deserialization of components, but isn't required for their use in pipelines.
#[cfg_attr(feature = "serde", typetag::serde(tag = "type"))]
pub trait ComponentFactory {
    fn build(&self) -> Box<dyn Component>;
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
    ///
    /// If one input branches before another is reached, this will rerun this component with each set of inputs.
    Named(Vec<SmolStr>),
    /// This comopnent receives an [`InputTree`] on the primary input, rooted at the earliest branching input.
    ///
    /// This is useful for components that "collect" previous values to combine them.
    MinTree(Vec<SmolStr>),
    /// This component receives an [`InputTree`] on the primary input, once per pipeline run.
    ///
    /// Similarly to [`Inputs::Mintree`], this can collect and combine previous values, but it has access to all of the input for a pipeline run.
    FullTree(Vec<SmolStr>),
}
impl Inputs {
    /// `Named(Vec::new())` means a component taktes no inputs; this is just an alias for that.
    #[inline(always)]
    pub const fn none() -> Self {
        Self::Named(Vec::new())
    }
    /// Convenience function to create a [`Named`] variant.
    pub fn named<S: Into<smol_str::SmolStr>, I: IntoIterator<Item = S>>(iter: I) -> Self {
        Self::Named(iter.into_iter().map(Into::into).collect())
    }
    /// Convenience function to create a [`MinTree`] variant.
    pub fn min_tree<S: Into<smol_str::SmolStr>, I: IntoIterator<Item = S>>(iter: I) -> Self {
        Self::MinTree(iter.into_iter().map(Into::into).collect())
    }
    /// Convenience function to create a [`FullTree`] variant.
    pub fn full_tree<S: Into<smol_str::SmolStr>, I: IntoIterator<Item = S>>(iter: I) -> Self {
        Self::FullTree(iter.into_iter().map(Into::into).collect())
    }
    /// Get the number of inputs this component is expecting
    pub fn expecting(&self) -> usize {
        match self {
            Self::Primary => 1,
            Self::Named(v) | Self::MinTree(v) | Self::FullTree(v) => v.len(),
        }
    }
    /// Check whether a channel is expected from this component.
    ///
    /// If this is `Inputs::Named` and the channel is `Some` and not specified, a component can be specified to call
    /// [`Component::can_take`] on instead.
    pub fn can_take(&self, channel: Option<&str>, component: Option<&dyn Component>) -> bool {
        match self {
            Self::Primary => channel.is_none(),
            Self::Named(vec) | Self::MinTree(vec) | Self::FullTree(vec) => {
                channel.is_some_and(|ch| {
                    vec.iter().any(|v| v == ch) || component.is_some_and(|c| c.can_take(ch))
                })
            }
        }
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
/// # #[cfg(not(feature = "vision"))]
/// # mod vv_vision {
/// #     pub mod buffer {
/// #         use std::sync::Arc;
/// #         use vv_pipelines::pipeline::prelude::Data;
/// #         #[derive(Clone, Copy)]
/// #         pub struct Buffer<'a>(&'a ());
/// #         impl Data for Buffer<'static> {
/// #             fn clone_to_arc(&self) -> Arc<dyn Data> { Arc::new(*self) }
/// #         }
/// #     }
/// # }
/// use vv_pipelines::pipeline::prelude::*;
/// use vv_vision::buffer::Buffer;
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

/// Check whether a component can accept input on a channel.
///
/// This matches against the result of [`Component::inputs`] and if it doesn't match, checks [`Component::can_take`].
/// Possibly re-allocating a `Vec` of elements isn't the most efficient, but anything else would be less convenient.
pub fn component_takes(component: &dyn Component, channel: Option<&str>) -> bool {
    component.inputs().can_take(channel, Some(component))
}
