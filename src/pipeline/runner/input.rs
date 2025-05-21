use super::*;
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
    fn get(&self, stream: &str) -> Option<Arc<dyn Data>>;
}
impl<D: Clone + IntoData> InputSpecifier for (&str, D) {
    fn get(&self, stream: &str) -> Option<Arc<dyn Data>> {
        (stream == self.0).then(|| self.1.clone().into_data())
    }
}
impl<T: InputSpecifier> InputSpecifier for &T {
    fn get(&self, stream: &str) -> Option<Arc<dyn Data>> {
        T::get(self, stream)
    }
}
impl<T: InputSpecifier> InputSpecifier for [T] {
    fn get(&self, stream: &str) -> Option<Arc<dyn Data>> {
        self.iter().find_map(|i| i.get(stream))
    }
}
impl<T: InputSpecifier, const N: usize> InputSpecifier for [T; N] {
    fn get(&self, stream: &str) -> Option<Arc<dyn Data>> {
        self.iter().find_map(|i| i.get(stream))
    }
}
impl<T: InputSpecifier> InputSpecifier for Vec<T> {
    fn get(&self, stream: &str) -> Option<Arc<dyn Data>> {
        self.iter().find_map(|i| i.get(stream))
    }
}
impl<S: Borrow<str> + Hash + Eq, D: Clone + Into<Arc<dyn Data>>> InputSpecifier for HashMap<S, D> {
    fn get(&self, stream: &str) -> Option<Arc<dyn Data>> {
        HashMap::get(self, stream).map(|d| d.clone().into())
    }
}

macro_rules! impl_for_tuple {
    () => {};
    ($head:ident $(, $tail:ident)*) => {
        impl<$head: InputSpecifier, $($tail: InputSpecifier,)*> InputSpecifier for ($head, $($tail,)*) {
            #[allow(non_snake_case)]
            fn get(&self, stream: &str) -> Option<Arc<dyn Data>> {
                let ($head, $($tail,)*) = self;
                if let Some(val) = $head.get(stream) {
                    return Some(val);
                }
                $(
                    if let Some(val) = $tail.get(stream) {
                        return Some(val);
                    }
                )*
                None
            }
        }
        impl_for_tuple!($($tail),*);
    };
}
impl_for_tuple!(T1, T2, T3, T4, T5, T6, T7, T8, T9, T10, T11, T12);

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum PackArgsError<'a> {
    #[error("No component {0}")]
    NoComponent(ComponentId),
    #[error("Component expects an input on the pimary stream")]
    ExpectingPrimary,
    #[error("Component needs {0:?} as an input stream, but none was specified")]
    MissingInput(&'a str),
}

/// Multi-input arguments packed in the order that the component expects them.
#[derive(Debug, Default, Clone)]
pub struct ComponentArgs(pub(super) Vec<Option<Arc<dyn Data>>>);
impl ComponentArgs {
    /// Create a new empty argument list.
    #[inline(always)]
    pub const fn empty() -> Self {
        Self(Vec::new())
    }
    /// Create an argument list with a single element.
    #[inline(always)]
    pub fn single(arg: impl IntoData) -> Self {
        Self(vec![Some(arg.into_data())])
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
    pub fn pack_args<I: InputSpecifier>(
        &self,
        component: ComponentId,
        input: I,
    ) -> Result<ComponentArgs, PackArgsError> {
        let data = self
            .components
            .get(component.0)
            .ok_or(PackArgsError::NoComponent(component))?;
        match &data.input_mode {
            InputMode::Single { name, .. } => {
                let name = name.as_deref().ok_or(PackArgsError::ExpectingPrimary)?;
                input
                    .get(name)
                    .ok_or(PackArgsError::MissingInput(&name))
                    .map(ComponentArgs::single)
            }
            InputMode::Multiple(lookup) => {
                let mut packed = vec![None; lookup.len()];
                for (name, idx) in lookup {
                    packed[*idx] = Some(input.get(name).ok_or(PackArgsError::MissingInput(&name))?);
                }
                Ok(ComponentArgs(packed))
            }
        }
    }
}
