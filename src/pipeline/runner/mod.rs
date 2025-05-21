use super::component::{Component, Data, TypeMismatch};
use crate::utils::LogErr;
use smallvec::SmallVec;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt::{self, Debug, Display, Formatter};
use std::ops::Deref;
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;

mod deps;
mod input;

pub use deps::*;
pub use input::*;

/// Newtype wrapper around a component ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct ComponentId(pub usize);
impl Display for ComponentId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.0, f)
    }
}

/// A unique identifier for which set of inputs a component's being run on.
///
/// It's guaranteed that every time a [`PipelineRunner`] runs a component, this value will be different.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId {
    /// The base run number.
    ///
    /// This is 0 the first time [`run`](PipelineRunner::run) is called, then 1, then 2, etc.
    pub run: u32,
    /// Which combination of outputs this is. TODO: figure out what this should be.
    pub tree: Vec<SmallVec<[u32; 2]>>,
}
impl RunId {
    pub fn starts_with(&self, other: &RunId) -> bool {
        self.run == other.run && self.tree.starts_with(&other.tree)
    }
    pub fn push(&mut self, vals: SmallVec<[u32; 2]>) {
        self.tree.push(vals);
    }
}
impl Display for RunId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.run)?;
        for row in &self.tree {
            f.write_str(":")?;
            let Some((head, tail)) = row.split_first() else {
                continue;
            };
            write!(f, "{head}")?;
            for elem in tail {
                write!(f, ".{elem}")?;
            }
        }
        Ok(())
    }
}

/// A callback function to be called after a pipeline completes.
pub type Callback<'a> = Box<dyn FnOnce(&'a PipelineRunner) + Send + Sync + 'a>;

struct Cleanup<'a> {
    runner: &'a PipelineRunner,
    callback: Option<Callback<'a>>,
}
impl Drop for Cleanup<'_> {
    fn drop(&mut self) {
        if let Some(callback) = self.callback.take() {
            callback(self.runner);
        }
        self.runner.running.fetch_sub(1, Ordering::AcqRel);
    }
}

/// An error that can occur from [`ComponentContextInner::get_as`].
#[derive(Debug, Clone, Error)]
pub enum DowncastInputError<'a> {
    /// The input wasn't given.
    #[error("Component doesn't have a {} input stream", if let Some(name) = .0 { format!("{name:?}") } else { "primary".to_string() })]
    MissingInput(Option<&'a str>),
    /// The stored type doesn't match the requested one.
    #[error(transparent)]
    TypeMismatch(#[from] TypeMismatch<Arc<dyn Data>>),
}
impl LogErr for DowncastInputError<'_> {
    fn log_err(&self) {
        match self {
            Self::MissingInput(_) => tracing::error!("{self}"),
            Self::TypeMismatch(m) => m.log_err(),
        }
    }
}

/// Parameters to be passed to [`PipelineRunner::run`].
pub struct RunParams<'a> {
    /// A callback we want to run after our pipeline has run.
    pub callback: Option<Callback<'a>>,
    /// The maximum number of running pipelines we want to allow to run.
    pub max_running: Option<usize>,
    /// The target component
    pub component: ComponentId,
    /// Arguments to be passed to the target component
    pub args: ComponentArgs,
}
impl<'a> RunParams<'a> {
    pub const fn new(component: ComponentId) -> Self {
        Self {
            component,
            args: ComponentArgs::empty(),
            max_running: None,
            callback: None,
        }
    }
    pub fn with_callback(
        mut self,
        callback: impl FnOnce(&'a PipelineRunner) + Send + Sync + 'a,
    ) -> Self {
        self.callback = Some(Box::new(callback));
        self
    }
    pub fn with_boxed_callback(mut self, callback: Callback<'a>) -> Self {
        self.callback = Some(callback);
        self
    }
    pub fn with_max_running(mut self, max_running: usize) -> Self {
        self.max_running = Some(max_running);
        self
    }
    pub fn with_args(mut self, args: ComponentArgs) -> Self {
        self.args = args;
        self
    }
}
impl Debug for RunParams<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("RunParams")
            .field(
                "callback",
                &if self.callback.is_some() {
                    "Some(..)"
                } else {
                    "None"
                },
            )
            .field("max_running", &self.max_running)
            .field("component", &self.component)
            .field("args", &self.args)
            .finish()
    }
}
impl From<ComponentId> for RunParams<'_> {
    fn from(value: ComponentId) -> Self {
        Self::new(value)
    }
}
impl<A: Into<ComponentArgs>> From<(ComponentId, A)> for RunParams<'_> {
    fn from(value: (ComponentId, A)) -> Self {
        Self::new(value.0).with_args(value.1.into())
    }
}

/// Marker structs for [`IntoRunParams`].
///
/// This shouldn't be considered public API, but marking this as `#[doc(hidden)]` hides the implementations altogether.
pub mod markers {
    pub struct ArgListMarker;
    pub struct InputMapMarker;
}

/// A type that's convertible in run parameters, with the runner available for the conversion if necessary.
///
/// This trait has a marker generic parameter to allow potentially overlapping implementations. Rust can deduce the marker type used as long as there aren't actually conflicting
pub trait IntoRunParams<'a, Marker> {
    /// A custom error type allows for this conversion to fail. It also becomes the error type of [`run`](PipelineRunner::run).
    type Error: From<RunError<'a>>;
    fn into_run_params(self, runner: &'a PipelineRunner) -> Result<RunParams<'a>, Self::Error>;
}
impl<'a> IntoRunParams<'a, ()> for RunParams<'a> {
    type Error = RunError<'a>;
    fn into_run_params(self, _runner: &'a PipelineRunner) -> Result<RunParams<'a>, RunError<'a>> {
        Ok(self)
    }
}
impl<'a, A: Into<ComponentArgs>> IntoRunParams<'a, markers::ArgListMarker> for (ComponentId, A) {
    type Error = RunError<'a>;
    fn into_run_params(self, _runner: &'a PipelineRunner) -> Result<RunParams<'a>, Self::Error> {
        Ok(RunParams::new(self.0).with_args(self.1.into()))
    }
}

impl<'a, I: InputSpecifier> IntoRunParams<'a, markers::InputMapMarker> for (ComponentId, I) {
    type Error = RunOrPackArgsError<'a>;
    fn into_run_params(self, runner: &'a PipelineRunner) -> Result<RunParams<'a>, Self::Error> {
        let args = runner
            .pack_args(self.0, self.1)
            .map_err(RunOrPackArgsError::PackArgsError)?;
        Ok(RunParams::new(self.0).with_args(args))
    }
}

/// Union of [`RunError`] and [`PackArgsError`], for when [`IntoRunParams`] fails for an input specifier.
#[derive(Debug, Error)]
pub enum RunOrPackArgsError<'a> {
    #[error(transparent)]
    RunError(RunError<'a>),
    #[error(transparent)]
    PackArgsError(PackArgsError<'a>),
}
impl<'a> From<RunError<'a>> for RunOrPackArgsError<'a> {
    fn from(value: RunError<'a>) -> Self {
        Self::RunError(value)
    }
}

/// The inner error enum for [`RunError`]
#[derive(Debug, Clone, Copy, Error)]
#[non_exhaustive]
pub enum RunErrorCause {
    /// The requested component ID was out of range.
    #[error("No component {0}")]
    NoComponent(ComponentId),
    /// Too many pipelines are running.
    #[error("Too many pipelines ({0}) were already running")]
    TooManyRunning(usize),
    /// The given number of arguments doesn't match the expected number.
    #[error("Expected {expected} arguments, got {given}")]
    ArgsMismatch { expected: usize, given: usize },
}

#[derive(Debug)]
pub struct RunError<'a> {
    pub cause: RunErrorCause,
    pub params: RunParams<'a>,
}
impl Display for RunError<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.cause, f)
    }
}
impl std::error::Error for RunError<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

/// The core runner for vision pipelines.
///
/// Note that in order for the lifetimes to work, this should be defined outside of the call to [`rayon::scope`].
///
/// ```ignore
/// let mut runner = PipelineRunner::new();
/// // It's best to do initialization here.
/// let component_a = runner.add_component("A", component_a()).unwrap();
/// let component_b = runner.add_component("B", component_b()).unwrap();
/// runner.add_dependency(component_a, None, component_b, None).unwrap();
/// rayon::scope(|scope| {
///     // Initialization *can* be done here, but probably shouldn't unless the scope is needed.
///     runner.run(component_a, Arc::new("input data".to_string()), scope);
///     // the call to run() immutably borrows the runner for the whole scope, so it can't be configured more here
///     runner.run(componet_a, Arc::new("different data".to_string()), scope); // You can, however, run more pipelines from here
/// });
/// // By the time the call to `scope` returns, all of the pipelines will have run.
/// ```
#[derive(Debug, Default)]
pub struct PipelineRunner {
    components: Vec<ComponentData>,
    lookup: HashMap<triomphe::Arc<str>, ComponentId>,
    running: AtomicUsize,
    run_id: AtomicU32,
}
impl PipelineRunner {
    /// Create a new runner with no components.
    #[inline(always)]
    pub fn new() -> Self {
        Self {
            components: Vec::new(),
            lookup: HashMap::new(),
            running: AtomicUsize::new(0),
            run_id: AtomicU32::new(0),
        }
    }
    /// Get a map from the registered component names to their IDs.
    #[inline(always)]
    pub fn components(&self) -> &HashMap<triomphe::Arc<str>, ComponentId> {
        &self.lookup
    }
    /// Get the number of running pipelines.
    #[inline(always)]
    pub fn running(&self) -> usize {
        self.running.load(Ordering::Relaxed)
    }
    /// Get the number of times [`run`](Self::run) has been called.
    #[inline(always)]
    pub fn run_count(&self) -> u32 {
        self.run_id.load(Ordering::Relaxed)
    }
    /// The main entry point for running pipelines.
    ///
    /// This invokes the given component with some data, in a given [scope](rayon::Scope).
    ///
    /// The generics here can almost always be inferred. They allow various sets of run parameters to be used, including:
    /// - [`RunParams`]
    /// - [`ComponentId`]
    /// - a `(ComponentId, A)` where A is some implementor of [`Data`] or an [`Arc`] of some implementor
    /// - a `(ComponentId, I)` where I is some [`InputSpecifier`] (most likely a `(&str, A)` or some slice or tuple).
    #[inline(always)]
    pub fn run<'s, 'a: 's, M, P: IntoRunParams<'a, M>>(
        &'a self,
        params: P,
        scope: &rayon::Scope<'s>,
    ) -> Result<(), P::Error> {
        let params = params.into_run_params(self)?;
        self.run_impl(params, scope).map_err(From::from)
    }
    fn run_impl<'s, 'a: 's>(
        &'a self,
        params: RunParams<'a>,
        scope: &rayon::Scope<'s>,
    ) -> Result<(), RunError<'a>> {
        let running = self.running.fetch_add(1, Ordering::AcqRel);
        if params.max_running.is_some_and(|max| running >= max) {
            self.running.fetch_sub(1, Ordering::AcqRel);
            return Err(RunError {
                cause: RunErrorCause::TooManyRunning(running),
                params,
            });
        }
        let Some(data) = self.components.get(params.component.0) else {
            return Err(RunError {
                cause: RunErrorCause::NoComponent(params.component),
                params,
            });
        };
        match (&data.input_mode, params.args.len()) {
            (InputMode::Single { .. }, n) => {
                if n != 1 {
                    return Err(RunError {
                        cause: RunErrorCause::ArgsMismatch {
                            expected: 1,
                            given: n,
                        },
                        params,
                    });
                }
            }
            (InputMode::Multiple(m), n) => {
                if m.len() != n {
                    return Err(RunError {
                        cause: RunErrorCause::ArgsMismatch {
                            expected: n,
                            given: m.len(),
                        },
                        params,
                    });
                }
            }
        }
        let RunParams {
            callback,
            max_running: _,
            component,
            mut args,
        } = params;
        let decr = Arc::new(Cleanup {
            runner: self,
            callback,
        });
        let run_id = RunId {
            run: self.run_id.fetch_add(1, Ordering::Relaxed),
            tree: Vec::new(),
        };
        let (input, cleanup_idx) = match args.len() {
            0 => (InputKind::Empty, None),
            1 => (InputKind::Single(args.0.pop().unwrap().unwrap()), None),
            len => {
                let mut lock = data.partial.lock().unwrap();
                let (idx, run, inputs) = lock.alloc(len);
                *run = Some(run_id.clone());
                for (to, from) in inputs.iter_mut().zip(&mut args.0) {
                    *to = from.take();
                }
                (InputKind::Multiple(idx), Some((idx, len)))
            }
        };
        scope.spawn(move |scope| {
            let data = &self.components[component.0];
            ComponentContextInner {
                runner: self,
                comp_id: component,
                input,
                decr,
                run_id,
                invoc: AtomicU32::new(0),
            }
            .run(&*data.component, scope);
            if let Some((idx, len)) = cleanup_idx {
                data.partial.lock().unwrap().free(idx, len);
            }
        });
        Ok(())
    }
}

enum InputKind {
    Empty,
    Single(Arc<dyn Data>),
    Multiple(usize),
}

/// Context passed to components, without the threadpool scope.
///
/// In order to defer tasks to the threadpool, the context has to be able to be separated from the scope, and a new `ComponentContext` can be created with the new scope.
/// This type has a destructor that tells all dependent components that no more data will come from here, so we need to make sure that this inner context isn't dropped during that transition.
pub struct ComponentContextInner<'r> {
    runner: &'r PipelineRunner,
    comp_id: ComponentId,
    input: InputKind,
    decr: Arc<Cleanup<'r>>,
    run_id: RunId,
    invoc: AtomicU32,
}
impl Debug for ComponentContextInner<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentContextInner")
            .field("runner", &(&self.runner as *const _))
            .field("comp_id", &self.comp_id)
            .field("run_id", &self.run_id)
            .field("invoc", &self.invoc)
            .finish_non_exhaustive()
    }
}
impl<'r> ComponentContextInner<'r> {
    /// Get the ID of this component. This is mostly useful for logging.
    pub fn comp_id(&self) -> ComponentId {
        self.comp_id
    }
    /// Get the ID of this run. This will be unique for each time this component is called.
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }
    /// Get the [`PipelineRunner`] that's calling this component.
    pub fn runner(&self) -> &'r PipelineRunner {
        self.runner
    }
    /// Get the name of the current component.
    pub fn name(&self) -> &'r triomphe::Arc<str> {
        &self.runner.components[self.comp_id.0].name
    }
    /// Get the current value from a given stream.
    pub fn get<'b>(&self, stream: impl Into<Option<&'b str>>) -> Option<Arc<dyn Data>> {
        let req_stream = stream.into();
        match self.input {
            InputKind::Empty => None,
            InputKind::Single(ref data) => {
                let component = &self.runner.components[self.comp_id.0];
                let InputMode::Single { name, .. } = &component.input_mode else {
                    unreachable!()
                };
                (name.as_deref() == req_stream).then(|| data.clone())
            }
            InputKind::Multiple(run_idx) => req_stream.and_then(|name| {
                let component = &self.runner.components[self.comp_id.0];
                let InputMode::Multiple(lookup) = &component.input_mode else {
                    unreachable!()
                };
                let field_idx = lookup.get(name)?;
                let num_fields = lookup.len();
                let lock = component.partial.lock().unwrap();
                let index = run_idx * num_fields + field_idx;
                Some(
                    lock.data[index]
                        .as_ref()
                        .expect("All fields should be initialized here!")
                        .clone(),
                )
            }),
        }
    }
    /// Same as [`get`](Self::get) but returns a `Result` that implements [`LogErr`].
    pub fn get_res<'b>(
        &self,
        stream: impl Into<Option<&'b str>>,
    ) -> Result<Arc<dyn Data>, DowncastInputError<'b>> {
        let stream = stream.into();
        self.get(stream)
            .ok_or(DowncastInputError::MissingInput(stream))
    }
    /// Get the current value from a given stream and attempt to downcast it. For even more convenience, [`DowncastInputError::log_err`] and the let-else pattern can be used.
    pub fn get_as<'b, T: Data>(
        &self,
        stream: impl Into<Option<&'b str>>,
    ) -> Result<Arc<T>, DowncastInputError<'b>> {
        self.get_res(stream)?.downcast_arc().map_err(From::from)
    }
    /// Publish a result on a given stream.
    #[inline(always)]
    pub fn submit<'b, 's>(
        &self,
        stream: impl Into<Option<&'b str>>,
        data: Arc<dyn Data>,
        scope: &rayon::Scope<'s>,
    ) where
        'r: 's,
    {
        self.submit_impl(stream.into(), data, scope);
    }
    fn submit_impl<'s>(&self, stream: Option<&str>, data: Arc<dyn Data>, scope: &rayon::Scope<'s>)
    where
        'r: 's,
    {
        let component = &self.runner.components[self.comp_id.0];
        let dependents = stream.map_or_else(
            || component.primary_dependents.as_slice(),
            |name| component.dependents.get(name).map_or(&[], Vec::as_slice),
        );
        let run = self.invoc.fetch_add(1, Ordering::Relaxed);
        for &(comp_id, stream) in dependents {
            let next_comp = &self.runner.components[comp_id.0];
            if let Some(_stream) = stream {
                todo!()
            } else {
                self.spawn_next(
                    next_comp,
                    comp_id,
                    InputKind::Single(data.clone()),
                    next_comp.multi_input.then_some(smallvec::smallvec![run]),
                    scope,
                );
            }
        }
    }
    fn spawn_next<'s>(
        &self,
        component: &'r ComponentData,
        comp_id: ComponentId,
        input: InputKind,
        last_row: Option<SmallVec<[u32; 2]>>,
        scope: &rayon::Scope<'s>,
    ) where
        'r: 's,
    {
        let runner = self.runner;
        let inner = component.component.clone();
        let decr = self.decr.clone();
        let mut run_id = self.run_id.clone();
        if let Some(row) = last_row {
            run_id.push(row);
        }
        scope.spawn(move |scope| {
            ComponentContextInner {
                input,
                runner,
                comp_id,
                decr,
                run_id,
                invoc: AtomicU32::new(0),
            }
            .run(&*inner, scope);
        });
    }
    /// Get the info-level span for this run.
    pub fn tracing_span(&self) -> tracing::Span {
        tracing::info_span!("run", name = %self.name(), run = %self.run_id, component = %self.comp_id)
    }
    /// Run the component with tracing instrumentation (and possibly more in the future).
    pub fn run<'s>(self, component: &dyn Component, scope: &rayon::Scope<'s>)
    where
        'r: 's,
    {
        self.tracing_span()
            .in_scope(|| component.run(ComponentContext { inner: self, scope }));
    }
}

/// Context passed to components.
///
/// A component is considered to be done submitting data when this is dropped, meaning that it can live after the component's body finishes.
#[derive(Debug)]
pub struct ComponentContext<'r, 'a, 's> {
    pub inner: ComponentContextInner<'r>,
    pub scope: &'a rayon::Scope<'s>,
}
impl<'r> Deref for ComponentContext<'r, '_, '_> {
    type Target = ComponentContextInner<'r>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl<'s, 'r: 's> ComponentContext<'r, '_, 's> {
    /// Run the component with tracing instrumentation (and possibly more in the future).
    #[inline(always)]
    pub fn run(self, component: &dyn Component) {
        self.inner.run(component, self.scope);
    }
    /// Publish a result on a given stream.
    #[inline(always)]
    pub fn submit<'b>(&self, stream: impl Into<Option<&'b str>>, data: Arc<dyn Data>) {
        self.inner.submit(stream, data, self.scope);
    }
    /// Defer an operation to run later on the threadpool.
    pub fn defer(self, op: impl FnOnce(ComponentContext<'r, '_, 's>) + Send + Sync + 'r) {
        let ComponentContext { inner, scope } = self;
        scope.spawn(move |scope| op(ComponentContext { inner, scope }));
    }
}
