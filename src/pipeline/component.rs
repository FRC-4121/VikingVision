use super::runner::ComponentContext;
use crate::buffer::Buffer;
use std::any::Any;
use std::fmt::{self, Debug, Display, Formatter};

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

/// Some kind of component to be used in the runner.
pub trait Component: Send + Sync + 'static {
    /// Check if an output stream is available.
    fn output_kind(&self, name: Option<&str>) -> OutputKind;
    /// Run a component on a given input.
    fn run<'a, 's, 'r: 's>(&self, context: ComponentContext<'r, 'a, 's>);
}
