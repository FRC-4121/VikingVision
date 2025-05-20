use crate::pipeline::component::IntoData;

use super::*;
use std::borrow::Borrow;
use std::collections::HashMap;
use std::hash::Hash;
use std::sync::Arc;

/// A way to provide a lookup for
pub trait InputSpecifier {
    fn get(&self, stream: &str) -> Option<Arc<dyn Data>>;
}
impl<D: Clone + Into<Arc<dyn Data>>> InputSpecifier for (&str, D) {
    fn get(&self, stream: &str) -> Option<Arc<dyn Data>> {
        (stream == self.0).then(|| self.1.clone().into())
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

#[derive(Debug, Clone, Copy, PartialEq, Error)]
pub enum PackArgsError<'a> {
    #[error("No component {}", .0.0)]
    NoComponent(ComponentId),
    #[error("Component {} has 0 or 1 input", .0.0)]
    NotPacked(ComponentId),
    #[error("Component needs {0:?} as an input stream, but none was specified")]
    MissingInput(&'a str),
}

/// Multi-input arguments packed in the order that the parameter is
#[derive(Debug, Clone)]
pub struct PackedArgs(pub Vec<Option<Arc<dyn Data>>>);

/// Arguments to pass to a component.
#[derive(Debug, Default, Clone)]
pub enum ComponentArgs {
    /// No inputs are needed.
    #[default]
    None,
    /// A single input is passed.
    ///
    /// This doesn't have to be for the primary stream, if the component takes just one named input.
    Single(Arc<dyn Data>),
    /// Packed inputs, packed by a call to [`PipelineRunner::pack_args`].
    Multiple(PackedArgs),
}

impl<T: IntoData> From<T> for ComponentArgs {
    fn from(value: T) -> Self {
        Self::Single(value.into_data())
    }
}
impl From<PackedArgs> for ComponentArgs {
    fn from(value: PackedArgs) -> Self {
        Self::Multiple(value)
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
    ) -> Result<PackedArgs, PackArgsError> {
        let data = self
            .components
            .get(component.0)
            .ok_or(PackArgsError::NoComponent(component))?;
        let InputMode::Multiple(lookup) = &data.input_mode else {
            return Err(PackArgsError::NotPacked(component));
        };
        let mut packed = vec![None; lookup.len()];
        for (name, idx) in lookup {
            packed[*idx] = Some(input.get(name).ok_or(PackArgsError::MissingInput(&name))?);
        }
        Ok(PackedArgs(packed))
    }
}
