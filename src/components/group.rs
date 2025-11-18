#![allow(clippy::collapsible_else_if)]
//! Implementation of [`GroupComponent`], a component that acts as a group of other components.

use super::ComponentIdentifier;
use crate::pipeline::graph::IdResolver;
use crate::pipeline::prelude::*;
use crate::pipeline::serialized::ComponentChannel as NameSource;
use crate::utils::{Configurable, Configure};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::collections::HashMap;
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
    fn output_kind(&self, _: &str) -> OutputKind {
        OutputKind::None
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let base = context.run_id().base_run();
        let Some(val) = context.get(None) else { return };
        self.data.lock().unwrap().entry(base).or_default().push(val);
    }
}

#[derive(Debug, Clone, PartialEq)]
pub struct Source {
    pub component: ComponentIdentifier,
    pub channel: Option<SmolStr>,
}
impl From<NameSource> for Source {
    fn from(value: NameSource) -> Self {
        Self {
            component: ComponentIdentifier::Name(value.component),
            channel: value.channel,
        }
    }
}

struct FirstConfig;
struct SecondConfig;

pub struct GroupConfig {
    pub input: ComponentIdentifier,
    pub outputs: HashMap<SmolStr, Source>,
}
struct GroupState {
    inputs: Inputs,
    outputs: HashMap<SmolStr, (Arc<Listener>, OutputKind)>,
}

impl<T>
    Configure<
        GroupConfig,
        Option<(GroupState, Configurable<GraphComponentId, T, SecondConfig>)>,
        (&mut PipelineGraph, GraphComponentId),
    > for FirstConfig
{
    fn name(&self) -> impl std::fmt::Display {
        "GroupComponent"
    }
    fn configure(
        &self,
        config: GroupConfig,
        (graph, self_id): (&mut PipelineGraph, GraphComponentId),
    ) -> Option<(GroupState, Configurable<GraphComponentId, T, SecondConfig>)> {
        let input_component = match config.input {
            ComponentIdentifier::Name(name) => {
                if let Some(&id) = graph.lookup().get(&*name) {
                    id
                } else {
                    error!(name = &*name, "couldn't resolve input name");
                    return None;
                }
            }
            ComponentIdentifier::Id(id) => id,
        };
        let Some(component) = graph.component(input_component) else {
            error!(id = %input_component, "input component ID out of range");
            return None;
        };
        let inputs = component.inputs();
        let mut outputs = HashMap::with_capacity(config.outputs.len());
        for (name, out) in config.outputs {
            let output_component = match out.component {
                ComponentIdentifier::Name(name) => {
                    if let Some(&id) = graph.lookup().get(&*name) {
                        id
                    } else {
                        error!(name = &*name, "couldn't resolve output name");
                        return None;
                    }
                }
                ComponentIdentifier::Id(id) => id,
            };
            let Some(component) = graph.component(output_component) else {
                error!(%output_component, "output component ID out of range");
                return None;
            };
            let mut kind = crate::pipeline::component::component_output(&**component, &name);
            if graph.branches_between(input_component, output_component) {
                kind = OutputKind::Multiple;
            }
            let listener = Arc::new(Listener::default());
            let listen_id = graph
                .add_hidden_component(listener.clone(), format!("listener-{self_id}: {name:?}"));
            if let Err(err) =
                graph.add_dependency((output_component, out.channel.clone()), listen_id)
            {
                error!(%err, "failed to add primary listener");
                return None;
            }
            outputs.insert(name, (listener, kind));
        }
        Some((
            GroupState { inputs, outputs },
            Configurable::new(input_component, SecondConfig),
        ))
    }
}

impl Configure<GraphComponentId, Option<RunnerComponentId>, &IdResolver> for SecondConfig {
    fn configure(&self, config: GraphComponentId, arg: &IdResolver) -> Option<RunnerComponentId> {
        arg.get(config)
    }
}

/// A component that acts as a "group" of other components.
///
/// Whenever this component is run, it runs another pipeline and completes when it's finished.
pub struct GroupComponent {
    #[allow(clippy::type_complexity)]
    inner: Configurable<
        GroupConfig,
        Option<(
            GroupState,
            Configurable<GraphComponentId, Option<RunnerComponentId>, SecondConfig>,
        )>,
        FirstConfig,
    >,
}
impl GroupComponent {
    pub const fn new(config: GroupConfig) -> Self {
        Self {
            inner: Configurable::new(config, FirstConfig),
        }
    }
}

impl Component for GroupComponent {
    fn inputs(&self) -> Inputs {
        self.inner
            .get_state_flat()
            .map_or(Inputs::none(), |c| c.0.inputs.clone())
    }
    fn output_kind(&self, name: &str) -> OutputKind {
        self.inner.get_state_flat().map_or(OutputKind::None, |s| {
            s.0.outputs.get(name).map_or(OutputKind::None, |o| o.1)
        })
    }
    fn run<'s, 'r: 's>(&self, ctx: ComponentContext<'_, 's, 'r>) {
        let (mut inner, scope) = ctx.explode();
        let Some((state, c)) = self.inner.get_state_flat() else {
            return;
        };
        let Some(component) = c.get_state_flat() else {
            return;
        };
        let flag = Arc::new(AtomicU32::new(u32::MAX));
        let flag_clone = Arc::clone(&flag);
        let params = RunParams::new(*component)
            .with_args(inner.packed_args())
            .with_callback(move |ctx| flag.store(ctx.run_id, Ordering::Release))
            .with_context(inner.context.clone());
        if let Err(err) = inner.runner().run(params, scope) {
            error!(%err, "failed to start sub-run");
        } else {
            let span = tracing::Span::current();
            let multi = state
                .outputs
                .iter()
                .map(|(k, v)| (k.clone(), v.clone()))
                .collect::<Vec<_>>();
            spawn_recursive(scope, move |scope| {
                let flag = flag_clone.load(Ordering::Acquire);
                let next = flag == u32::MAX;
                if next {
                    inner.finish(scope);
                } else {
                    let _guard = span.enter();
                    for (channel, (listener, kind)) in multi.iter() {
                        let val = listener.data.lock().unwrap().remove(&flag);
                        if inner.listening(channel) {
                            if kind.is_multi() {
                                for data in val.into_iter().flatten() {
                                    inner.submit(channel, data, scope);
                                }
                            } else {
                                if let Some(vec) = val {
                                    if vec.len() > 1 {
                                        warn!(
                                            len = vec.len(),
                                            name = &**channel,
                                            "multiple values submitted on a single-value channel"
                                        );
                                    }
                                    let data = vec.into_iter().next().unwrap();
                                    inner.submit(channel, data, scope);
                                }
                            }
                        }
                    }
                }
                next
            });
        }
    }
    fn initialize(&self, runner: &mut PipelineGraph, self_id: GraphComponentId) {
        let ran = self.inner.init((runner, self_id));
        if !ran {
            error!("called initialize() on an already initialized Group");
        }
    }
    fn remap(&self, resolver: &crate::pipeline::graph::IdResolver) {
        let Some((_, c)) = self.inner.get_state_flat() else {
            return;
        };
        if !c.init(resolver) {
            error!("called remap() on an already remapped Group");
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
    pub input: SmolStr,
    pub output: Option<NameSource>,
    pub outputs: HashMap<SmolStr, NameSource>,
}
impl From<GroupFactory> for GroupConfig {
    fn from(value: GroupFactory) -> Self {
        let mut outputs =
            HashMap::with_capacity(value.outputs.len() + usize::from(value.output.is_some()));
        outputs.extend(value.output.map(|s| (SmolStr::new_static(""), s.into())));
        outputs.extend(value.outputs.into_iter().map(|(n, s)| (n, s.into())));
        GroupConfig {
            input: ComponentIdentifier::Name(value.input),
            outputs,
        }
    }
}
#[typetag::serde(name = "group")]
impl ComponentFactory for GroupFactory {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(GroupComponent::new(self.clone().into()))
    }
}

crate::impl_register!([dyn ComponentFactory]; "group" => GroupFactory);
