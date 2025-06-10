#![allow(clippy::collapsible_else_if)]
//! Implementation of [`GroupComponent`], a component that acts as a group of other components.

use super::ComponentIdentifier;
use crate::pipeline::prelude::*;
use crate::serialized::Source as NameSource;
use crate::utils::{Configurable, Configure};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::marker::PhantomData;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::{Arc, Mutex};
use tracing::{error, warn};

#[derive(Default)]
struct Listener {
    data: Mutex<HashMap<u32, Vec<Arc<dyn Data>>>>,
}
impl Component for Listener {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, _: Option<&str>) -> OutputKind {
        OutputKind::None
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'r, '_, 's>) {
        let base = context.run_id().base_run();
        let Some(val) = context.get(None) else { return };
        self.data.lock().unwrap().entry(base).or_default().push(val);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub component: ComponentIdentifier,
    pub channel: Option<String>,
}
impl From<crate::serialized::Source> for Source {
    fn from(value: crate::serialized::Source) -> Self {
        Self {
            component: ComponentIdentifier::Name(value.component),
            channel: value.channel,
        }
    }
}

pub struct GroupConfig {
    pub input: ComponentIdentifier,
    pub primary_output: Option<Source>,
    pub output_map: HashMap<String, Source>,
}
struct GroupState {
    inputs: Inputs,
    input_component: ComponentId,
    primary_out: (Option<Arc<Listener>>, OutputKind),
    outputs: HashMap<String, (Arc<Listener>, OutputKind)>,
}
impl Configure<GroupConfig, Option<GroupState>, (&mut PipelineRunner, ComponentId)>
    for PhantomData<GroupComponent>
{
    fn name(&self) -> impl std::fmt::Display {
        "GroupComponent"
    }
    fn configure(
        &self,
        config: GroupConfig,
        (runner, self_id): (&mut PipelineRunner, ComponentId),
    ) -> Option<GroupState> {
        let input_component = match config.input {
            ComponentIdentifier::Name(name) => {
                if let Some(&id) = runner.components().get(&*name) {
                    id
                } else {
                    error!(name = name, "couldn't resolve input name");
                    return None;
                }
            }
            ComponentIdentifier::Id(id) => id,
        };
        let Some(component) = runner.component(input_component) else {
            error!(id = %input_component, "input component ID out of range");
            return None;
        };
        let inputs = component.inputs();
        let primary_out = if let Some(out) = config.primary_output {
            let id = match out.component {
                ComponentIdentifier::Name(name) => {
                    if let Some(&id) = runner.components().get(&*name) {
                        id
                    } else {
                        error!(name = name, "couldn't resolve output name");
                        return None;
                    }
                }
                ComponentIdentifier::Id(id) => id,
            };
            let Some(component) = runner.component(input_component) else {
                error!(%id, "output component ID out of range");
                return None;
            };
            let mut kind = component.output_kind(None);
            if kind == OutputKind::Single && runner.branch_chain(id).next().is_some_and(|c| c != id)
            {
                kind = OutputKind::Multiple;
            }
            let listener = Arc::new(Listener::default());
            let listen_id = runner
                .add_hidden_component(format!("listener-{self_id}-primary"), listener.clone());
            if let Err(err) = runner.add_dependency(id, out.channel.as_deref(), listen_id, None) {
                error!(%err, "failed to add primary listener");
                return None;
            }
            (Some(listener), kind)
        } else {
            (None, OutputKind::None)
        };
        let mut outputs = HashMap::with_capacity(config.output_map.len());
        for (name, out) in config.output_map {
            let id = match out.component {
                ComponentIdentifier::Name(name) => {
                    if let Some(&id) = runner.components().get(&*name) {
                        id
                    } else {
                        error!(name = name, "couldn't resolve output name");
                        return None;
                    }
                }
                ComponentIdentifier::Id(id) => id,
            };
            let Some(component) = runner.component(input_component) else {
                error!(%id, "output component ID out of range");
                return None;
            };
            let mut kind = component.output_kind(None);
            if kind == OutputKind::Single && runner.branch_chain(id).next().is_some_and(|c| c != id)
            {
                kind = OutputKind::Multiple;
            }
            let listener = Arc::new(Listener::default());
            let listen_id = runner
                .add_hidden_component(format!("listener-{self_id}-named-{name}"), listener.clone());
            if let Err(err) = runner.add_dependency(id, out.channel.as_deref(), listen_id, None) {
                error!(%err, "failed to add primary listener");
                return None;
            }
            outputs.insert(name, (listener, kind));
        }
        Some(GroupState {
            inputs,
            input_component,
            primary_out,
            outputs,
        })
    }
}

/// A component that acts as a "group" of other components.
///
/// Whenever this component is run, it runs another pipeline and completes when it's finished.
pub struct GroupComponent {
    inner: Configurable<GroupConfig, Option<GroupState>, PhantomData<Self>>,
}
impl GroupComponent {
    pub const fn new(config: GroupConfig) -> Self {
        Self {
            inner: Configurable::new(config, PhantomData),
        }
    }
}

impl Component for GroupComponent {
    fn inputs(&self) -> Inputs {
        self.inner
            .get_state_flat()
            .map_or(Inputs::none(), |c| c.inputs.clone())
    }
    fn output_kind(&self, name: Option<&str>) -> OutputKind {
        self.inner.get_state_flat().map_or(OutputKind::None, |s| {
            if let Some(name) = name {
                s.outputs.get(name).map_or(OutputKind::None, |o| o.1)
            } else {
                s.primary_out.1
            }
        })
    }
    fn run<'s, 'r: 's>(&self, ComponentContext { inner, scope }: ComponentContext<'r, '_, 's>) {
        let Some(state) = self.inner.get_state_flat() else {
            return;
        };
        let flag = Arc::new(AtomicU32::new(u32::MAX));
        let flag_clone = Arc::clone(&flag);
        let params = RunParams::new(state.input_component)
            .with_args(inner.packed_args())
            .with_callback(move |_, run| flag.store(run, Ordering::Release));
        if let Err(err) = inner.runner().run(params, scope) {
            error!(%err, "failed to start sub-run");
        } else {
            let span = tracing::Span::current();
            let (primary, kind) = state.primary_out.clone();
            let multi = state
                .outputs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>();
            spawn_recursive(scope, move |scope| {
                let flag = flag_clone.load(Ordering::Acquire);
                flag == u32::MAX || {
                    let _guard = span.enter();
                    if let Some(listener) = &primary {
                        let val = listener.data.lock().unwrap().remove(&flag);
                        if inner.listening(None) {
                            if kind.is_multi() {
                                for data in val.into_iter().flatten() {
                                    inner.submit(None, data, scope);
                                }
                            } else {
                                if let Some(vec) = val {
                                    if vec.len() > 1 {
                                        warn!(
                                            len = vec.len(),
                                            name = ?None::<&String>,
                                            "multiple values submitted on a single-value channel"
                                        );
                                    }
                                    let data = vec.into_iter().next().unwrap();
                                    inner.submit(None, data, scope);
                                }
                            }
                        }
                    }
                    for (channel, (listener, kind)) in multi.iter() {
                        let val = listener.data.lock().unwrap().remove(&flag);
                        if inner.listening(None) {
                            if kind.is_multi() {
                                for data in val.into_iter().flatten() {
                                    inner.submit(None, data, scope);
                                }
                            } else {
                                if let Some(vec) = val {
                                    if vec.len() > 1 {
                                        warn!(
                                            len = vec.len(),
                                            name = ?Some(channel),
                                            "multiple values submitted on a single-value channel"
                                        );
                                    }
                                    let data = vec.into_iter().next().unwrap();
                                    inner.submit(channel.as_str(), data, scope);
                                }
                            }
                        }
                    }
                    false
                }
            });
        }
    }
    fn initialize(&self, runner: &mut PipelineRunner, self_id: ComponentId) {
        let ran = self.inner.init((runner, self_id));
        if !ran {
            error!("called initialize() on an already initialized Group");
        }
    }
}
fn spawn_recursive<'s>(
    scope: &rayon::Scope<'s>,
    mut op: impl FnMut(&rayon::Scope<'s>) -> bool + Send + Sync + 's,
) {
    scope.spawn(move |scope| {
        if op(scope) {
            spawn_recursive(scope, op);
        }
    });
}

#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct GroupFactory {
    pub input: String,
    pub output: Option<NameSource>,
    pub outputs: HashMap<String, NameSource>,
}
impl From<GroupFactory> for GroupConfig {
    fn from(value: GroupFactory) -> Self {
        GroupConfig {
            input: ComponentIdentifier::Name(value.input),
            primary_output: value.output.map(From::from),
            output_map: value
                .outputs
                .into_iter()
                .map(|(k, v)| (k, v.into()))
                .collect(),
        }
    }
}
#[typetag::serde(name = "group")]
impl ComponentFactory for GroupFactory {
    fn build(&self, _name: &str) -> Box<dyn Component> {
        Box::new(GroupComponent::new(self.clone().into()))
    }
}
