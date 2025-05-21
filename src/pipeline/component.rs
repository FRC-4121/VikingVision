use super::runner::{ComponentContext, PipelineRunner};
use crate::buffer::Buffer;
use crate::utils::LogErr;
use std::any::{Any, TypeId};
use std::fmt::{self, Debug, Display, Formatter};
use std::sync::Arc;
use thiserror::Error;

#[derive(Debug, Clone, Copy, Error)]
#[error("Couldn't downcast data to {expected}")]
pub struct TypeMismatch<A> {
    pub id: TypeId,
    pub expected: disqualified::ShortName<'static>,
    pub additional: A,
}
impl<A> LogErr for TypeMismatch<A> {
    fn log_err(&self) {
        tracing::error!(id = ?self.id, "Couldn't downcast data to {}", self.expected);
    }
}

/// Some kind of data that can be passed between components.
///
/// This is implemented for:
/// - [`bool`]
/// - all primitive integers
/// - [`String`]
/// - [`Buffer`]
/// - [`Vec`]s of data
/// - tuples with up to 12 elements
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
    u8,
    u16,
    u32,
    u64,
    usize,
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

/// A serializable factory that can build a component
#[typetag::serde]
pub trait ComponentFactory {
    fn build(&self, name: &str) -> Box<dyn Component>;
}

/// Kind of an output stream
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub enum OutputKind {
    /// There's no stream associated with the given output. This is used to catch errors earlier.
    None,
    /// Only one output will be sent per input. If multiple outputs are called after this was returned, the runner will panic.
    Single,
    /// Multiple outputs can be sent from a single input. If it's possible that multiple could be returned, then this should always be chosen.
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
    /// This component takes inputs through its primary input stream.
    Primary,
    /// This component takes multiple inputs.
    Named(Vec<String>),
}

/// Some kind of component to be used in the runner.
pub trait Component: Send + Sync + 'static {
    /// Get the inputs that this component is expecting.
    fn inputs(&self) -> Inputs;
    /// Check if an output stream is available.
    fn output_kind(&self, name: Option<&str>) -> OutputKind;
    /// Run a component on a given input.
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'r, '_, 's>);
    /// Perform startup initialization on this component.
    #[allow(unused_variables)]
    fn initialize(&self, runner: &PipelineRunner) {}
}
