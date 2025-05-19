use super::runner::{ComponentInput, ComponentOutput};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::collections::HashMap;
use std::fmt::{self, Debug, Display, Formatter};

/// Some kind of data that can be passed between components
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

/// Which inputs to use for the given component
#[derive(Debug, Default, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum InputConfig {
    /// No pre-configured input; instead an input will be passed externally
    #[default]
    None,
    /// One input, on the default channel
    Single(String),
    /// Multiple named inputs
    Multiple(HashMap<String, String>),
}

/// A serializable factory that can build a component
#[typetag::serde]
pub trait ComponentFactory {
    fn build(&self, name: &str) -> Box<dyn Component>;
}

#[derive(Serialize, Deserialize)]
pub struct ComponentConfig {
    /// Inputs for this component
    input: InputConfig,
    #[serde(flatten)]
    factory: Box<dyn ComponentFactory>,
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
    fn output_kind(&self, name: Option<&str>) -> OutputKind;
    fn run<'a, 's, 'r: 's>(&self, input: ComponentInput<'r>, output: ComponentOutput<'r, 'a, 's>);
}
