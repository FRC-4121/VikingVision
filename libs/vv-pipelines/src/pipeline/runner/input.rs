use super::*;
use crate::pipeline::ComponentSpecifier;
use crate::pipeline::component::IntoData;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

/// A way to provide a lookup for component inputs.
///
/// The main implementors are `(&str, D)` and [`HashMap<String, D>`](HashMap) where D implements [`IntoData`], and with that it's implemented for:
/// - references
/// - slices
/// - arrays
/// - [`Vec`], and
/// - tuples of up to twelve elements
pub trait InputSpecifier {
    fn get(&self, channel: &str) -> Option<Arc<dyn Data>>;
    fn expected_len(&self) -> Option<usize>;
}
impl<D: Clone + IntoData> InputSpecifier for (&str, D) {
    fn get(&self, channel: &str) -> Option<Arc<dyn Data>> {
        (channel == self.0).then(|| self.1.clone().into_data())
    }
    fn expected_len(&self) -> Option<usize> {
        Some(1)
    }
}
impl<T: InputSpecifier> InputSpecifier for &T {
    fn get(&self, channel: &str) -> Option<Arc<dyn Data>> {
        T::get(self, channel)
    }
    fn expected_len(&self) -> Option<usize> {
        T::expected_len(self)
    }
}
impl<T: InputSpecifier> InputSpecifier for [T] {
    fn get(&self, channel: &str) -> Option<Arc<dyn Data>> {
        self.iter().find_map(|i| i.get(channel))
    }
    fn expected_len(&self) -> Option<usize> {
        self.iter().map(T::expected_len).sum()
    }
}
impl<T: InputSpecifier, const N: usize> InputSpecifier for [T; N] {
    fn get(&self, channel: &str) -> Option<Arc<dyn Data>> {
        self.iter().find_map(|i| i.get(channel))
    }
    fn expected_len(&self) -> Option<usize> {
        self.iter().map(T::expected_len).sum()
    }
}
impl<T: InputSpecifier> InputSpecifier for Vec<T> {
    fn get(&self, channel: &str) -> Option<Arc<dyn Data>> {
        self.iter().find_map(|i| i.get(channel))
    }
    fn expected_len(&self) -> Option<usize> {
        self.iter().map(T::expected_len).sum()
    }
}
impl<S: Borrow<str> + Hash + Eq, D: Clone + Into<Arc<dyn Data>>> InputSpecifier for HashMap<S, D> {
    fn get(&self, channel: &str) -> Option<Arc<dyn Data>> {
        HashMap::get(self, channel).map(|d| d.clone().into())
    }
    fn expected_len(&self) -> Option<usize> {
        Some(self.len())
    }
}

macro_rules! impl_for_tuple {
    () => {};
    ($head:ident $(, $tail:ident)*) => {
        #[allow(non_snake_case)]
        impl<$head: InputSpecifier, $($tail: InputSpecifier,)*> InputSpecifier for ($head, $($tail,)*) {
            fn get(&self, channel: &str) -> Option<Arc<dyn Data>> {
                let ($head, $($tail,)*) = self;
                if let Some(val) = $head.get(channel) {
                    return Some(val);
                }
                $(
                    if let Some(val) = $tail.get(channel) {
                        return Some(val);
                    }
                )*
                None
            }
            #[allow(unused_mut)]
            fn expected_len(&self) -> Option<usize> {
                let ($head, $($tail,)*) = self;
                let mut sum = $head.expected_len()?;
                $(sum += $tail.expected_len()?;)*
                Some(sum)
            }
        }
        impl_for_tuple!($($tail),*);
    };
}
impl_for_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);

/// An error that can occur from [`PipelineRunner::pack_args`].
#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum PackArgsError<'a, E> {
    /// The requested component ID was out of range.
    #[error(transparent)]
    NoComponent(E),
    /// The component takes data through its primary input.
    #[error("Component expects a primary input")]
    ExpectingPrimary,
    /// The component needs an input, but it wasn't given.
    #[error("Component needs an input named {0:?}, but none was specified")]
    MissingInput(&'a str),
}

/// Multi-input arguments packed in the order that the component expects them.
#[derive(Debug, Default, Clone)]
pub struct ComponentArgs(pub(super) Vec<Arc<dyn Data>>);
impl ComponentArgs {
    /// Create a new empty argument list.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self(Vec::new())
    }
    /// Create an argument list with a single element.
    #[inline(always)]
    pub fn single(arg: impl IntoData) -> Self {
        Self(vec![arg.into_data()])
    }
    /// Get the number
    #[inline(always)]
    pub const fn len(&self) -> usize {
        self.0.len()
    }
    #[inline(always)]
    pub const fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<T: IntoData> From<T> for ComponentArgs {
    fn from(value: T) -> Self {
        Self::single(value)
    }
}

impl PipelineRunner {
    /// Pack the given input arguments according to the order.
    ///
    /// Note that adding more inputs after this will invalidate the packed arguments and lead to unexpected behavior.
    pub fn pack_args<C: ComponentSpecifier<Self>, I: InputSpecifier>(
        &self,
        component: C,
        input: I,
    ) -> Result<ComponentArgs, PackArgsError<'_, C::Error>> {
        let component = component
            .resolve(self)
            .map_err(PackArgsError::NoComponent)?;
        let data = &self.components[component.index()];
        match &data.input_mode {
            InputMode::Single { name, .. } => {
                let (name, _) = name.as_ref().ok_or(PackArgsError::ExpectingPrimary)?;
                input
                    .get(name)
                    .ok_or(PackArgsError::MissingInput(name))
                    .map(ComponentArgs::single)
            }
            InputMode::Multiple {
                lookup, tree_shape, ..
            } => {
                let len = lookup.len();
                let mut packed = vec![PLACEHOLDER_DATA.clone(); len];
                for (name, idx) in lookup {
                    let resolved = (idx.0.checked_sub(1).map_or(0, |i| tree_shape[i as usize])
                        + idx.1) as usize;
                    packed[resolved] = input.get(name).ok_or(PackArgsError::MissingInput(name))?;
                }
                if let Some(expected) = input.expected_len()
                    && expected != len
                {
                    tracing::warn!(
                        expected,
                        read = len,
                        "number of args for the component doesn't match the number of args for the component"
                    );
                }
                Ok(ComponentArgs(packed))
            }
        }
    }
}
