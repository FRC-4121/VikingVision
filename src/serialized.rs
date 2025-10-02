use crate::camera::config::CameraConfig;
use crate::pipeline::UnknownComponentName;
use crate::pipeline::component::ComponentFactory;
use crate::pipeline::graph::{AddDependencyError, DuplicateNamedComponent, PipelineGraph};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use supply::prelude::*;
use thiserror::Error;

fn default_running() -> usize {
    rayon::current_num_threads() / 2
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ParseSourceError {
    #[error("Component name is empty")]
    EmptyComponent,
    #[error("Channel name is empty")]
    EmptyChannel,
    #[error("Non-alphanumeric character in byte {0} of channel")]
    NonAlphaNumComponent(usize),
}

#[derive(Debug, Default, Clone, PartialEq, Deserialize)]
#[serde(try_from = "&str")]
pub struct Source {
    pub component: String,
    pub channel: Option<SmolStr>,
}
impl Display for Source {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(channel) = &self.channel {
            write!(f, "{}.{channel}", self.component)
        } else {
            f.write_str(&self.component)
        }
    }
}
impl TryFrom<&str> for Source {
    type Error = ParseSourceError;
    fn try_from(value: &str) -> Result<Self, Self::Error> {
        if let Some(idx) = value.find('.') {
            if idx == 0 {
                return Err(ParseSourceError::EmptyComponent);
            }
            let component = &value[..idx];
            let channel = &value[(idx + 1)..];
            if channel.is_empty() {
                return Err(ParseSourceError::EmptyChannel);
            }
            if let Some((n, _)) = component
                .char_indices()
                .find(|&(_, c)| !(c == '-' || c == '_' || c.is_alphanumeric()))
            {
                return Err(ParseSourceError::NonAlphaNumComponent(n));
            }
            Ok(Source {
                component: component.to_string(),
                channel: Some(channel.into()),
            })
        } else {
            if let Some((n, _)) = value
                .char_indices()
                .find(|&(_, c)| !(c == '-' || c == '_' || c.is_alphanumeric()))
            {
                return Err(ParseSourceError::NonAlphaNumComponent(n));
            }
            Ok(Source {
                component: value.to_string(),
                channel: None,
            })
        }
    }
}
impl Serialize for Source {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
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
    Single(Source),
    /// Multiple named inputs
    Multiple(HashMap<SmolStr, Source>),
}

#[derive(Serialize, Deserialize)]
pub struct ComponentConfig {
    /// Inputs for this component
    pub input: InputConfig,
    #[serde(flatten)]
    pub factory: Box<dyn ComponentFactory>,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum BuildRunnerError<'a> {
    #[error(transparent)]
    AddComponentError(DuplicateNamedComponent),
    #[error(transparent)]
    AddDependencyError(AddDependencyError<UnknownComponentName, UnknownComponentName>),
    #[error("No component named {0:?}")]
    NoComponent(&'a str),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunConfig {
    #[serde(default = "default_running")]
    pub max_running: usize,
}

/// The name of a component, able to be requested from the context.
pub struct ComponentName<'a>(pub &'a str);

/// Type tag for [`ComponentName`].
#[ty_tag::tag]
pub type ComponentNameTag<'a> = ComponentName<'a>;

pub struct InjectName<'a, 'b> {
    inner: &'a mut dyn ProviderDyn<'b>,
    name: &'a str,
}
impl<'r, 'a> Provider<'r> for InjectName<'a, 'r> {
    type Lifetimes = l!['r];

    fn provide(&'r self, want: &mut dyn Want<Self::Lifetimes>) {
        want.provide_value(ComponentName(self.name));
        self.inner.provide(want);
    }

    fn provide_mut(&'r mut self, want: &mut dyn Want<Self::Lifetimes>) {
        want.provide_value(ComponentName(self.name));
        self.inner.provide_mut(want);
    }
}

#[derive(Serialize, Deserialize)]
pub struct ConfigFile {
    pub config: RunConfig,
    #[serde(alias = "camera")]
    pub cameras: HashMap<String, Box<dyn CameraConfig>>,
    #[serde(alias = "component")]
    pub components: HashMap<SmolStr, ComponentConfig>,
}
impl ConfigFile {
    pub fn add_to_graph(
        &self,
        graph: &mut PipelineGraph,
        context: &mut dyn ProviderDyn,
    ) -> Result<(), BuildRunnerError<'_>> {
        for (name, config) in &self.components {
            let component = config.factory.build(&mut InjectName {
                inner: context,
                name,
            });
            graph
                .add_named_component(component.into(), name.clone())
                .map_err(BuildRunnerError::AddComponentError)?;
        }
        for (name, config) in &self.components {
            if config.input == InputConfig::None {
                continue;
            }
            match &config.input {
                InputConfig::None => unreachable!(),
                InputConfig::Single(s) => {
                    graph
                        .add_dependency((s.component.as_str(), s.channel.clone()), name)
                        .map_err(BuildRunnerError::AddDependencyError)?;
                }
                InputConfig::Multiple(m) => {
                    for (channel, s) in m {
                        graph
                            .add_dependency(
                                (s.component.as_str(), s.channel.clone()),
                                (name, channel),
                            )
                            .map_err(BuildRunnerError::AddDependencyError)?;
                    }
                }
            }
        }
        Ok(())
    }
    #[inline(always)]
    pub fn build_graph(
        &self,
        context: &mut dyn ProviderDyn,
    ) -> Result<PipelineGraph, BuildRunnerError<'_>> {
        let mut graph = PipelineGraph::new();
        self.add_to_graph(&mut graph, context).map(|_| graph)
    }
}
