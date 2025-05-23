//! Basic utility components.

use super::ComponentIdentifier;
use crate::pipeline::prelude::*;
use crate::utils::{Configurable, Configure};
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use std::sync::Arc;
use tracing::{error, info};

/// A simple component that prints an info-level span with the information in it.
#[derive(Debug, Clone, Copy)]
pub struct DebugComponent;
impl Component for DebugComponent {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, _: Option<&str>) -> OutputKind {
        OutputKind::None
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'r, '_, 's>) {
        let Ok(val) = context.get_res(None).and_log_err() else {
            return;
        };
        info!(?val, "debug");
    }
}

/// A factory to build a [`DebugComponent`].
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct DebugFactory {}
#[typetag::serde(name = "debug")]
impl ComponentFactory for DebugFactory {
    fn build(&self, _: &str) -> Box<dyn Component> {
        Box::new(DebugComponent)
    }
}

impl Configure<ComponentIdentifier, Option<Arc<dyn Component>>, &mut PipelineRunner>
    for PhantomData<CloneComponent>
{
    fn name(&self) -> impl std::fmt::Display {
        "CloneComponent"
    }
    fn configure(
        &self,
        config: ComponentIdentifier,
        arg: &mut PipelineRunner,
    ) -> Option<Arc<dyn Component>> {
        match config {
            ComponentIdentifier::Id(id) => {
                let component = arg.component(id);
                if component.is_none() {
                    error!(%id, "component ID out of range");
                }
                component.cloned()
            }
            ComponentIdentifier::Name(name) => {
                let id = arg.components().get(&*name);
                if id.is_none() {
                    error!(name = name, "couldn't resolve component name");
                }
                id.and_then(|&id| arg.component(id)).cloned()
            }
        }
    }
}

/// A component that refers to another component, but has its own dependencies.
pub struct CloneComponent {
    inner: Configurable<ComponentIdentifier, Option<Arc<dyn Component>>, PhantomData<Self>>,
}
impl CloneComponent {
    pub const fn new(id: ComponentIdentifier) -> Self {
        Self {
            inner: Configurable::new(id, PhantomData),
        }
    }
}
impl Component for CloneComponent {
    fn inputs(&self) -> Inputs {
        self.inner
            .get_state_flat()
            .map_or(Inputs::none(), |c| c.inputs())
    }
    fn output_kind(&self, name: Option<&str>) -> OutputKind {
        self.inner
            .get_state_flat()
            .map_or(OutputKind::None, |c| c.output_kind(name))
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'r, '_, 's>) {
        if let Some(comp) = self.inner.get_state_flat() {
            comp.run(context);
        }
    }
    fn initialize(&self, runner: &mut PipelineRunner, _self_id: ComponentId) {
        self.inner.init(runner);
    }
}

/// A factory to build a [`CloneComponent`].
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CloneFactory {
    pub name: String,
}
#[typetag::serde(name = "clone")]
impl ComponentFactory for CloneFactory {
    fn build(&self, _: &str) -> Box<dyn Component> {
        Box::new(CloneComponent::new(self.name.clone().into()))
    }
}
