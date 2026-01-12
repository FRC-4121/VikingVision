#[cfg(feature = "supply")]
use supply::prelude::*;

pub mod component_filter;
pub mod components;
pub mod configure;
pub mod pipeline;

/// A [`Provider`] that doesn't supply any values.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoContext;
#[cfg(feature = "supply")]
impl<'r> Provider<'r> for NoContext {
    type Lifetimes = l!['r];

    fn provide(&'r self, _want: &mut dyn Want<Self::Lifetimes>) {}
}
