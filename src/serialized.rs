use crate::camera::config::CameraConfig;
use crate::pipeline::component::ComponentFactory;
use crate::pipeline::runner::{AddComponentError, AddDependencyError, PipelineRunner};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
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
    pub stream: Option<String>,
}
impl Display for Source {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(stream) = &self.stream {
            write!(f, "{}.{stream}", self.component)
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
                stream: Some(channel.to_string()),
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
                stream: None,
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
    Multiple(HashMap<String, Source>),
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
    AddComponentError(AddComponentError),
    #[error(transparent)]
    AddDependencyError(AddDependencyError<'a>),
    #[error("No component named {0:?}")]
    NoComponent(&'a str),
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunConfig {
    #[serde(default = "default_running")]
    pub max_running: usize,
}

#[derive(Serialize, Deserialize)]
pub struct ConfigFile {
    pub config: RunConfig,
    #[serde(alias = "camera")]
    pub cameras: HashMap<String, Box<dyn CameraConfig>>,
    #[serde(alias = "component")]
    pub components: HashMap<String, ComponentConfig>,
}
impl ConfigFile {
    pub fn add_to_runner(&self, runner: &mut PipelineRunner) -> Result<(), BuildRunnerError<'_>> {
        for (name, config) in &self.components {
            let component = config.factory.build(name);
            runner
                .add_component(name.clone(), component.into())
                .map_err(BuildRunnerError::AddComponentError)?;
        }
        for (name, config) in &self.components {
            if config.input == InputConfig::None {
                continue;
            }
            let sub_id = runner.components()[name.as_str()];
            match &config.input {
                InputConfig::None => unreachable!(),
                InputConfig::Single(s) => {
                    let pub_id = *runner
                        .components()
                        .get(s.component.as_str())
                        .ok_or(BuildRunnerError::NoComponent(&s.component))?;
                    runner
                        .add_dependency(pub_id, s.stream.as_deref(), sub_id, None)
                        .map_err(BuildRunnerError::AddDependencyError)?;
                }
                InputConfig::Multiple(m) => {
                    for (stream, s) in m {
                        let pub_id = *runner
                            .components()
                            .get(s.component.as_str())
                            .ok_or(BuildRunnerError::NoComponent(&s.component))?;
                        runner
                            .add_dependency(pub_id, s.stream.as_deref(), sub_id, Some(stream))
                            .map_err(BuildRunnerError::AddDependencyError)?;
                    }
                }
            }
        }
        Ok(())
    }
    #[inline(always)]
    pub fn build_runner(&self) -> Result<PipelineRunner, BuildRunnerError<'_>> {
        let mut runner = PipelineRunner::new();
        self.add_to_runner(&mut runner).map(|_| runner)
    }
}
