use crate::pipeline::UnknownComponentName;
use crate::pipeline::component::ComponentFactory;
use crate::pipeline::graph::{AddDependencyError, DuplicateNamedComponent, PipelineGraph};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::collections::HashMap;
use std::fmt::{self, Display, Formatter};
use supply::prelude::*;
use thiserror::Error;

#[derive(Debug, Clone, PartialEq, Error)]
pub enum ParseSourceError {
    #[error("Component name is empty")]
    EmptyComponent,
    #[error("Channel name is empty")]
    EmptyChannel,
    #[error("Non-alphanumeric character in byte {0} of channel")]
    NonAlphaNumComponent(usize),
}

#[derive(Debug, Default, Clone, PartialEq)]
pub struct ComponentChannel {
    pub component: SmolStr,
    pub channel: Option<SmolStr>,
}
impl Display for ComponentChannel {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(channel) = &self.channel {
            write!(f, "{}.{channel}", self.component)
        } else {
            f.write_str(&self.component)
        }
    }
}
impl TryFrom<&str> for ComponentChannel {
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
            Ok(ComponentChannel {
                component: component.into(),
                channel: Some(channel.into()),
            })
        } else {
            if let Some((n, _)) = value
                .char_indices()
                .find(|&(_, c)| !(c == '-' || c == '_' || c.is_alphanumeric()))
            {
                return Err(ParseSourceError::NonAlphaNumComponent(n));
            }
            Ok(ComponentChannel {
                component: value.into(),
                channel: None,
            })
        }
    }
}
impl TryFrom<String> for ComponentChannel {
    type Error = ParseSourceError;
    fn try_from(value: String) -> Result<Self, Self::Error> {
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
            Ok(ComponentChannel {
                component: component.into(),
                channel: Some(channel.into()),
            })
        } else {
            if let Some((n, _)) = value
                .char_indices()
                .find(|&(_, c)| !(c == '-' || c == '_' || c.is_alphanumeric()))
            {
                return Err(ParseSourceError::NonAlphaNumComponent(n));
            }
            Ok(ComponentChannel {
                component: value.into(),
                channel: None,
            })
        }
    }
}
impl Serialize for ComponentChannel {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&self.to_string())
    }
}
impl<'de> Deserialize<'de> for ComponentChannel {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct ChannelVisitor;
        impl<'de> serde::de::Visitor<'de> for ChannelVisitor {
            type Value = ComponentChannel;

            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a component channel")
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                ComponentChannel::try_from(v).map_err(E::custom)
            }
            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                ComponentChannel::try_from(v).map_err(E::custom)
            }
        }
        deserializer.deserialize_string(ChannelVisitor)
    }
}

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum OneOrMany {
    One(ComponentChannel),
    Many(Vec<ComponentChannel>),
}
impl OneOrMany {
    pub fn as_slice(&self) -> &[ComponentChannel] {
        match self {
            Self::One(v) => std::slice::from_ref(v),
            Self::Many(v) => v,
        }
    }
}
impl<'de> Deserialize<'de> for OneOrMany {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct OOMVisitor;
        impl<'de> serde::de::Visitor<'de> for OOMVisitor {
            type Value = OneOrMany;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a component channel")
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                ComponentChannel::try_from(v)
                    .map(OneOrMany::One)
                    .map_err(E::custom)
            }
            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                ComponentChannel::try_from(v)
                    .map(OneOrMany::One)
                    .map_err(E::custom)
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut out = seq.size_hint().map_or(Vec::new(), Vec::with_capacity);
                while let Some(elem) = seq.next_element()? {
                    out.push(elem);
                }
                Ok(OneOrMany::Many(out))
            }
        }
        deserializer.deserialize_any(OOMVisitor)
    }
}

/// Which inputs to use for the given component
#[derive(Debug, Default, Clone, PartialEq, Serialize)]
#[serde(untagged)]
pub enum InputConfig {
    /// No pre-configured input; instead an input will be passed externally
    #[default]
    None,
    /// One input, on the default channel
    Single(OneOrMany),
    /// Multiple named inputs
    Multiple(HashMap<SmolStr, OneOrMany>),
}
impl<'de> Deserialize<'de> for InputConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct InputVisitor;
        impl<'de> serde::de::Visitor<'de> for InputVisitor {
            type Value = InputConfig;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("an input specification")
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                ComponentChannel::try_from(v)
                    .map(OneOrMany::One)
                    .map(InputConfig::Single)
                    .map_err(E::custom)
            }
            fn visit_string<E>(self, v: String) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                ComponentChannel::try_from(v)
                    .map(OneOrMany::One)
                    .map(InputConfig::Single)
                    .map_err(E::custom)
            }
            fn visit_seq<A>(self, mut seq: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::SeqAccess<'de>,
            {
                let mut out = seq.size_hint().map_or(Vec::new(), Vec::with_capacity);
                while let Some(elem) = seq.next_element()? {
                    out.push(elem);
                }
                Ok(InputConfig::Single(OneOrMany::Many(out)))
            }
            fn visit_map<A>(self, mut map: A) -> Result<Self::Value, A::Error>
            where
                A: serde::de::MapAccess<'de>,
            {
                use serde::de::Error;
                use std::collections::hash_map::Entry;
                let mut inputs = map
                    .size_hint()
                    .map_or_else(HashMap::new, HashMap::with_capacity);
                while let Some((k, v)) = map.next_entry()? {
                    match inputs.entry(k) {
                        Entry::Occupied(e) => {
                            return Err(A::Error::custom(format_args!(
                                "duplicate input {:?}",
                                e.key()
                            )));
                        }
                        Entry::Vacant(e) => {
                            e.insert(v);
                        }
                    }
                }
                Ok(InputConfig::Multiple(inputs))
            }
        }
        deserializer.deserialize_any(InputVisitor)
    }
}

#[derive(Serialize, Deserialize)]
pub struct ComponentConfig {
    /// Inputs for this component
    #[serde(default)]
    pub input: InputConfig,
    #[serde(flatten)]
    pub factory: Box<dyn ComponentFactory>,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum BuildGraphError<'a> {
    #[error(transparent)]
    AddComponentError(DuplicateNamedComponent),
    #[error(transparent)]
    AddDependencyError(AddDependencyError<UnknownComponentName, UnknownComponentName>),
    #[error("No component named {0:?}")]
    NoComponent(&'a str),
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
#[serde(transparent)]
pub struct SerializedGraph(pub HashMap<SmolStr, ComponentConfig>);

impl SerializedGraph {
    pub fn add_to_graph(
        &self,
        graph: &mut PipelineGraph,
        context: &mut dyn ProviderDyn,
    ) -> Result<(), BuildGraphError<'_>> {
        for (name, config) in &self.0 {
            let component = config.factory.build(&mut InjectName {
                inner: context,
                name,
            });
            graph
                .add_named_component(component.into(), name.clone())
                .map_err(BuildGraphError::AddComponentError)?;
        }
        for (name, config) in &self.0 {
            if config.input == InputConfig::None {
                continue;
            }
            match &config.input {
                InputConfig::None => unreachable!(),
                InputConfig::Single(s) => {
                    for c in s.as_slice() {
                        graph
                            .add_dependency((c.component.as_str(), c.channel.clone()), name)
                            .map_err(BuildGraphError::AddDependencyError)?;
                    }
                }
                InputConfig::Multiple(m) => {
                    for (channel, s) in m {
                        for c in s.as_slice() {
                            graph
                                .add_dependency(
                                    (c.component.as_str(), c.channel.clone()),
                                    (name, channel),
                                )
                                .map_err(BuildGraphError::AddDependencyError)?;
                        }
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
    ) -> Result<PipelineGraph, BuildGraphError<'_>> {
        let mut graph = PipelineGraph::new();
        self.add_to_graph(&mut graph, context).map(|_| graph)
    }
}
