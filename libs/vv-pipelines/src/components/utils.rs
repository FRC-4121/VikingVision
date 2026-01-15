//! Basic utility components.

use super::ComponentIdentifier;
use crate::configure::{Configurable, Configure};
use crate::pipeline::prelude::*;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::marker::PhantomData;
use std::sync::{Arc, mpsc};
use std::time::Duration;
use supply::{Request, prelude::*};
use tracing::{error, info};
use vv_utils::common_types::PipelineId;
use vv_utils::mutex::Mutex;
use vv_utils::utils::FpsCounter;
use vv_vision::buffer::Buffer;

#[cfg(feature = "serde")]
const fn true_default() -> bool {
    true
}
/// A simple component that prints an info-level span with the information in it.
#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct DebugComponent {
    #[cfg_attr(feature = "serde", serde(default = "true_default"))]
    pub noisy: bool,
}
impl Component for DebugComponent {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, _: &str) -> OutputKind {
        OutputKind::None
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(val) = context.get_res(None).and_log_err() else {
            return;
        };
        info!(?val, allow_noisy = self.noisy, "debug");
    }
}
#[cfg_attr(feature = "serde", typetag::serde(name = "debug"))]
impl ComponentFactory for DebugComponent {
    fn build(&self) -> Box<dyn Component> {
        Box::new(*self)
    }
}

impl Configure<ComponentIdentifier, Option<Arc<dyn Component>>, &mut PipelineGraph>
    for PhantomData<CloneComponent>
{
    fn name(&self) -> impl std::fmt::Display {
        "CloneComponent"
    }
    fn configure(
        &self,
        config: ComponentIdentifier,
        arg: &mut PipelineGraph,
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
                let id = arg.lookup.get(&*name);
                if id.is_none() {
                    error!(name = &*name, "couldn't resolve component name");
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
    fn output_kind(&self, name: &str) -> OutputKind {
        self.inner
            .get_state_flat()
            .map_or(OutputKind::None, |c| c.output_kind(name))
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        if let Some(comp) = self.inner.get_state_flat() {
            comp.run(context);
        }
    }
    fn initialize(&self, runner: &mut PipelineGraph, _self_id: GraphComponentId) {
        self.inner.init(runner);
    }
}

/// A factory to build a [`CloneComponent`].
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CloneFactory {
    pub name: String,
}
#[cfg_attr(feature = "serde", typetag::serde(name = "clone"))]
impl ComponentFactory for CloneFactory {
    fn build(&self) -> Box<dyn Component> {
        Box::new(CloneComponent::new(self.name.clone().into()))
    }
}

#[derive(Debug, Default, Clone)]
pub struct WrapMutexComponent<T> {
    _marker: PhantomData<fn(T) -> T>,
}
impl<T: Data + Clone> WrapMutexComponent<T> {
    pub const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
    pub fn new_boxed() -> Box<dyn Component> {
        Box::new(Self::new())
    }
}
impl<T: Data + Clone> Component for WrapMutexComponent<T> {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, name: &str) -> OutputKind {
        if name.is_empty() {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(data) = context.get_as::<T>(None).and_log_err() else {
            return;
        };
        context.submit("", Mutex::new(T::clone(&data)));
    }
}

/// Convenience component factory to make a `WrapMutexComponent<Buffer>`
#[derive(Debug, Default, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CanvasFactory {}
#[cfg_attr(feature = "serde", typetag::serde(name = "canvas"))]
impl ComponentFactory for CanvasFactory {
    fn build(&self) -> Box<dyn Component> {
        Box::new(WrapMutexComponent::<Buffer>::new())
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "WMFShim"))]
pub struct WrapMutexFactory {
    /// The inner type.
    ///
    /// Currently supported types are:
    /// - integer types
    /// - `f32` and `f64`
    /// - [`String`] as `string`
    /// - [`Buffer`] as `buffer`
    /// - a [`Vec`] of any of the previous types, as the previous wrapped in brackets e.g. `[string]` for `Vec<String>`
    pub inner: String,
    /// The actual construction function.
    ///
    /// This is skipped in de/serialization, and looked up based on the type name
    #[cfg_attr(feature = "serde", serde(skip))]
    pub factory: fn() -> Box<dyn Component>,
}
#[cfg_attr(feature = "serde", typetag::serde(name = "wrap-mutex"))]
impl ComponentFactory for WrapMutexFactory {
    fn build(&self) -> Box<dyn Component> {
        (self.factory)()
    }
}

#[cfg(feature = "serde")]
#[derive(Deserialize)]
struct WMFShim {
    inner: String,
}
#[cfg(feature = "serde")]
impl TryFrom<WMFShim> for WrapMutexFactory {
    type Error = String;

    fn try_from(value: WMFShim) -> Result<Self, Self::Error> {
        let factory = match &*value.inner {
            "i8" => WrapMutexComponent::<i8>::new_boxed,
            "i16" => WrapMutexComponent::<i16>::new_boxed,
            "i32" => WrapMutexComponent::<i32>::new_boxed,
            "i64" => WrapMutexComponent::<i64>::new_boxed,
            "isize" => WrapMutexComponent::<isize>::new_boxed,
            "u8" => WrapMutexComponent::<u8>::new_boxed,
            "u16" => WrapMutexComponent::<u16>::new_boxed,
            "u32" => WrapMutexComponent::<u32>::new_boxed,
            "u64" => WrapMutexComponent::<u64>::new_boxed,
            "usize" => WrapMutexComponent::<usize>::new_boxed,
            "f32" => WrapMutexComponent::<f32>::new_boxed,
            "f64" => WrapMutexComponent::<f64>::new_boxed,
            "buffer" => WrapMutexComponent::<Buffer>::new_boxed,
            "string" => WrapMutexComponent::<String>::new_boxed,
            "[i8]" => WrapMutexComponent::<Vec<i8>>::new_boxed,
            "[i16]" => WrapMutexComponent::<Vec<i16>>::new_boxed,
            "[i32]" => WrapMutexComponent::<Vec<i32>>::new_boxed,
            "[i64]" => WrapMutexComponent::<Vec<i64>>::new_boxed,
            "[isize]" => WrapMutexComponent::<Vec<isize>>::new_boxed,
            "[u8]" => WrapMutexComponent::<Vec<u8>>::new_boxed,
            "[u16]" => WrapMutexComponent::<Vec<u16>>::new_boxed,
            "[u32]" => WrapMutexComponent::<Vec<u32>>::new_boxed,
            "[u64]" => WrapMutexComponent::<Vec<u64>>::new_boxed,
            "[usize]" => WrapMutexComponent::<Vec<usize>>::new_boxed,
            "[f32]" => WrapMutexComponent::<Vec<f32>>::new_boxed,
            "[f64]" => WrapMutexComponent::<Vec<f64>>::new_boxed,
            "[buffer]" => WrapMutexComponent::<Vec<Buffer>>::new_boxed,
            "[string]" => WrapMutexComponent::<Vec<String>>::new_boxed,
            name => return Err(format!("Unrecognized type {name:?}")),
        };
        Ok(WrapMutexFactory {
            inner: value.inner,
            factory,
        })
    }
}

enum ChannelKind<T> {
    None,
    Bounded(mpsc::SyncSender<T>),
    Unbounded(mpsc::Sender<T>),
}

pub trait DataKind: Data {
    fn extract(value: Arc<dyn Data>) -> Option<Arc<Self>>;
}
impl<T: Data> DataKind for T {
    fn extract(value: Arc<dyn Data>) -> Option<Arc<Self>> {
        value.downcast_arc().and_log_err().ok()
    }
}
impl DataKind for dyn Data {
    fn extract(value: Arc<dyn Data>) -> Option<Arc<Self>> {
        Some(value)
    }
}

/// A component that sends its input data to a channel, along with context from the pipeline.
#[allow(clippy::type_complexity)]
#[cfg(feature = "supply")]
pub struct ChannelComponent<T, R: for<'a> Request<l!['a], Output: 'static>> {
    kind: ChannelKind<(Arc<T>, <R as Request<l!['static]>>::Output)>,
}
#[cfg(feature = "supply")]
impl<T, R: for<'a> Request<l!['a], Output: 'static>> Debug for ChannelComponent<T, R> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ChannelComponent").finish_non_exhaustive()
    }
}
#[allow(clippy::type_complexity)]
#[cfg(feature = "supply")]
impl<T, R: for<'a> Request<l!['a], Output: Send + 'static>> ChannelComponent<T, R> {
    /// Create a dummy component, with no receiver.
    pub const fn dummy() -> Self {
        Self {
            kind: ChannelKind::None,
        }
    }
    /// Create a new component with a possibly-bounded channel.
    pub fn new(
        bound: Option<usize>,
    ) -> (
        Self,
        mpsc::Receiver<(Arc<T>, <R as Request<l!['static]>>::Output)>,
    ) {
        let (kind, rx) = if let Some(bound) = bound {
            let (tx, rx) = mpsc::sync_channel(bound);
            (ChannelKind::Bounded(tx), rx)
        } else {
            let (tx, rx) = mpsc::channel();
            (ChannelKind::Unbounded(tx), rx)
        };
        (Self { kind }, rx)
    }
}
#[cfg(feature = "supply")]
impl<T: DataKind, R: for<'a> Request<l!['a], Output: Send + 'static> + 'static> Component
    for ChannelComponent<T, R>
{
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, _name: &str) -> OutputKind {
        OutputKind::None
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Some(val) = context
            .get_res(None)
            .and_log_err()
            .ok()
            .and_then(T::extract)
        else {
            return;
        };
        // SAFETY: we enforce that the output of this component is 'static in all cases, so it can't return a different type here.
        let ctx = unsafe {
            std::mem::transmute::<&dyn for<'a> ProviderDyn<'a>, &'static dyn ProviderDyn<'static>>(
                &*context.context,
            )
        }
        .request::<R>();
        match &self.kind {
            ChannelKind::None => {}
            ChannelKind::Bounded(s) => drop(s.send((val, ctx))),
            ChannelKind::Unbounded(s) => drop(s.send((val, ctx))),
        }
    }
}

/// A component that tracks the frequency that it's called at.
#[derive(Debug)]
pub struct FpsComponent {
    inner: Mutex<HashMap<Option<PipelineId>, FpsCounter>>,
    max_duration: Duration,
}
impl Default for FpsComponent {
    fn default() -> Self {
        Self::new()
    }
}
impl FpsComponent {
    pub fn with_max_duration(max_duration: Duration) -> Self {
        Self {
            inner: Mutex::new(HashMap::new()),
            max_duration,
        }
    }
    pub fn new() -> Self {
        Self::with_max_duration(default_fps_dur())
    }
    pub fn set_max_duration(&mut self, duration: Duration) {
        let Ok(inner) = self.inner.get_mut() else {
            error!("poisoned FPS counter lock");
            return;
        };
        for val in inner.values_mut() {
            val.set_max_duration(duration);
        }
    }
}
impl Component for FpsComponent {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, name: &str) -> OutputKind {
        if ["min", "max", "fps", "pretty"].contains(&name) {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(mut lock) = self.inner.lock() else {
            error!("poisoned FPS counter lock");
            return;
        };
        let fps = lock
            .entry(context.pipeline_id())
            .or_insert(FpsCounter::new(self.max_duration));
        fps.tick();
        let mut minmax = None;
        if context.listening("min") || context.listening("max") {
            let [min, max] = fps.minmax().unwrap_or_default();
            context.submit("min", min);
            context.submit("max", max);
            minmax = Some([min, max]);
        }
        context.submit_if_listening("fps", || fps.fps());
        context.submit_if_listening("pretty", || {
            let [min, max] = minmax.or_else(|| fps.minmax()).unwrap_or_default();
            format!("{min:.2}/{max:.2}/{:.2} FPS", fps.fps())
        });
    }
}

#[inline(always)]
const fn default_fps_dur() -> Duration {
    Duration::from_secs(10)
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FpsFactory {
    #[cfg_attr(
        feature = "serde",
        serde(
            alias = "period",
            default = "default_fps_dur",
            with = "humantime_serde"
        )
    )]
    pub duration: Duration,
}
#[cfg_attr(feature = "serde", typetag::serde(name = "fps"))]
impl ComponentFactory for FpsFactory {
    fn build(&self) -> Box<dyn Component> {
        Box::new(FpsComponent::with_max_duration(self.duration))
    }
}

pub struct BroadcastVec<T> {
    _marker: PhantomData<T>,
}
impl<T> BroadcastVec<T> {
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}
impl<T: Data + Clone> Component for BroadcastVec<T> {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, name: &str) -> OutputKind {
        match name {
            "" => OutputKind::Single,
            "elem" => OutputKind::Multiple,
            _ => OutputKind::None,
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(val) = context.get_as::<Vec<T>>(None).and_log_err() else {
            return;
        };
        context.submit("", val.clone());
        for elem in &*val {
            context.submit("elem", Arc::new(elem.clone()));
        }
    }
}

#[inline(always)]
#[cfg(feature = "serde")]
const fn is_false(v: &bool) -> bool {
    !*v
}

#[derive(Debug, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct UnpackFields {
    #[cfg_attr(feature = "serde", serde(default, skip_serializing_if = "is_false"))]
    pub allow_missing: bool,
}
impl Component for UnpackFields {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, _name: &str) -> OutputKind {
        OutputKind::Single
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(input) = context.get_res(None).and_log_err() else {
            return;
        };
        for chan in context.listeners().keys() {
            if let Some(field) = input.field(chan) {
                context.submit(chan, field.into_owned());
            } else if !self.allow_missing {
                tracing::warn!(type = %disqualified::ShortName(input.type_name()), field = &**chan, "missing field in component");
            }
        }
    }
}
#[cfg_attr(feature = "serde", typetag::serde(name = "unpack"))]
impl ComponentFactory for UnpackFields {
    fn build(&self) -> Box<dyn Component> {
        Box::new(*self)
    }
}
