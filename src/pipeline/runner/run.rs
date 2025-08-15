use super::*;
use crate::pipeline::ComponentSpecifier;
use crate::pipeline::component::{IntoData, TypeMismatch};
use crate::utils::LogErr;
use std::convert::Infallible;
use std::ops::{Deref, DerefMut};
use supply::prelude::*;

#[derive(Debug)]
pub struct CleanupContext<'r> {
    pub runner: &'r PipelineRunner,
    pub run_id: u32,
    pub context: Context<'r>,
}

/// A callback function to be called after a pipeline completes.
pub type Callback<'a> = Arc<dyn CallbackInner<'a>>;

/// Implementation detail for cleanup callbacks.
///
/// This consumes a reference to an [`Arc`], calling `self` if it's unique.
pub trait CallbackInner<'r>: Send + Sync + 'r {
    fn call_if_unique(self: Arc<Self>, ctx: CleanupContext<'r>);
}
impl<'r, F: FnOnce(CleanupContext<'r>) + Send + Sync + 'r> CallbackInner<'r> for F {
    fn call_if_unique(self: Arc<Self>, ctx: CleanupContext<'r>) {
        if let Some(this) = Arc::into_inner(self) {
            this(ctx);
        }
    }
}

type ProviderTrait<'c> = dyn for<'a> ProviderDyn<'a> + Send + Sync + 'c;

/// Context to be passed around between components.
///
/// This dereferences to [`Provider`] and can have typed fields requested from it.
#[derive(Default, Clone)]
pub enum Context<'c> {
    /// No context was passed.
    #[default]
    None,

    /// An external reference is passing context.
    Borrowed(&'c ProviderTrait<'c>),

    /// An shared reference holds the context.
    Owned(Arc<ProviderTrait<'c>>),
}
impl Debug for Context<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Context").finish_non_exhaustive()
    }
}
impl<'c> Deref for Context<'c> {
    type Target = ProviderTrait<'c>;

    fn deref(&self) -> &Self::Target {
        match self {
            Self::None => &crate::utils::NoContext,
            Self::Borrowed(p) => *p,
            Self::Owned(p) => &**p,
        }
    }
}
impl<'c> From<&'c ProviderTrait<'c>> for Context<'c> {
    #[inline(always)]
    fn from(value: &'c ProviderTrait) -> Self {
        Self::Borrowed(value)
    }
}
impl<'c> From<Arc<ProviderTrait<'c>>> for Context<'c> {
    #[inline(always)]
    fn from(value: Arc<ProviderTrait<'c>>) -> Self {
        Self::Owned(value)
    }
}
impl<'c> From<()> for Context<'c> {
    #[inline(always)]
    fn from(_value: ()) -> Self {
        Self::None
    }
}

/// Error that occurs when attempting to get and downcast input data.
///
/// This error occurs either when the requested input doesn't exist or when the input exists but has an incompatible type.
#[derive(Debug, Clone, Error)]
pub enum DowncastInputError<'a> {
    /// The requested input was not provided
    #[error("Component doesn't have a{}", if let Some(name) = .0 { format!("n input named {name:?}") } else { " primary input".to_string() })]
    MissingInput(Option<&'a str>),

    /// The input exists but has a different type than requested
    #[error(transparent)]
    TypeMismatch(#[from] TypeMismatch<Arc<dyn Data>>),
}

impl LogErr for DowncastInputError<'_> {
    fn log_err(&self) {
        match self {
            Self::MissingInput(name) => {
                tracing::error!(
                    "Component doesn't have a{}",
                    if let Some(name) = name {
                        format!("n input named {name:?}")
                    } else {
                        " primary input".to_string()
                    }
                )
            }
            Self::TypeMismatch(m) => m.log_err(),
        }
    }
}

/// Parameters for configuring a pipeline run. Controls component selection,
/// input data, execution limits, and completion callbacks.
pub struct RunParams<'a> {
    /// Optional callback to execute after pipeline completion
    pub callback: Option<Callback<'a>>,

    /// Optional limit on concurrent pipeline executions
    pub max_running: Option<usize>,

    /// The component to execute
    pub component: RunnerComponentId,

    /// Input arguments for the component
    pub args: ComponentArgs,

    /// Shared context for the run.
    pub context: Context<'a>,
}

impl<'a> RunParams<'a> {
    /// Create a new set of parameters with no run limit or callback.
    #[inline(always)]
    pub const fn new(component: RunnerComponentId) -> Self {
        Self {
            component,
            args: ComponentArgs::empty(),
            max_running: None,
            callback: None,
            context: Context::None,
        }
    }

    /// Add a completion callback to the parameters.
    #[inline(always)]
    pub fn with_callback(
        mut self,
        callback: impl FnOnce(CleanupContext) + Send + Sync + 'a,
    ) -> Self {
        self.callback = Some(Arc::new(callback));
        self
    }

    /// Add a pre-boxed callback to the parameters.
    #[inline(always)]
    pub fn with_boxed_callback(mut self, callback: Callback<'a>) -> Self {
        self.callback = Some(callback);
        self
    }

    /// Set a limit on concurrent pipeline executions.
    #[inline(always)]
    pub fn with_max_running(mut self, max_running: usize) -> Self {
        self.max_running = Some(max_running);
        self
    }

    /// Set the input arguments for the component.
    #[inline(always)]
    pub fn with_args(mut self, args: impl Into<ComponentArgs>) -> Self {
        self.args = args.into();
        self
    }

    /// Set the context for the pipeline run.
    #[inline(always)]
    pub fn with_context(mut self, context: impl Into<Context<'a>>) -> Self {
        self.context = context.into();
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
            .field("context", &self.context)
            .finish()
    }
}

/// Marker types for parameter conversion traits.
///
/// These types are used to disambiguate implementations of [`IntoRunParams`]
/// for different input types. They should not be used directly.
pub mod markers {
    /// Marker for a component specifier
    pub struct ComponentSpecMarker;
    /// Marker for direct argument list conversions
    pub struct ArgListMarker;
    /// Marker for input map conversions
    pub struct InputMapMarker;
}

/// Trait for types that can be converted into pipeline run parameters.
///
/// This trait enables flexible input handling for pipeline runs, supporting:
/// - Direct parameter passing
/// - Component ID with arguments
/// - Component ID with input maps
///
/// The marker type parameter allows for multiple implementations for the same
/// type without conflicts.
///
/// # Examples
///
/// ```rust
/// # use viking_vision::pipeline::prelude::for_test::*;
/// # let mut runner = PipelineRunner::new();
/// # let component_id = runner.add_component("processor", produce_component()).unwrap();
///
/// rayon::scope(|scope| {
///     // Direct parameters
///     let params = RunParams::new(component_id);
///     runner.run(params, scope);
///
///     // Component with data
///     runner.run((component_id, vec![1, 2, 3]), scope);
///
///     // Component with named inputs
///     runner.run((component_id, [("input", "data".to_string())]), scope);
/// });
/// ```
pub trait IntoRunParams<'a, Marker> {
    /// The error type returned if conversion fails
    type Error;

    /// Converts this type into run parameters
    fn into_run_params(self, runner: &'a PipelineRunner) -> Result<RunParams<'a>, Self::Error>;
}
impl<'a> IntoRunParams<'a, ()> for RunParams<'a> {
    type Error = Infallible;
    fn into_run_params(self, _runner: &'a PipelineRunner) -> Result<RunParams<'a>, Self::Error> {
        Ok(self)
    }
}
impl<'a, C: ComponentSpecifier<PipelineRunner>> IntoRunParams<'a, markers::ComponentSpecMarker>
    for C
{
    type Error = C::Error;
    fn into_run_params(self, runner: &'a PipelineRunner) -> Result<RunParams<'a>, Self::Error> {
        self.resolve(runner).map(RunParams::new)
    }
}
impl<'a, C: ComponentSpecifier<PipelineRunner>, A: Into<ComponentArgs>>
    IntoRunParams<'a, markers::ArgListMarker> for (C, A)
{
    type Error = C::Error;
    fn into_run_params(self, runner: &'a PipelineRunner) -> Result<RunParams<'a>, Self::Error> {
        self.0
            .resolve(runner)
            .map(|i| RunParams::new(i).with_args(self.1.into()))
    }
}
impl<'a, C: ComponentSpecifier<PipelineRunner>, I: InputSpecifier>
    IntoRunParams<'a, markers::InputMapMarker> for (C, I)
{
    type Error = PackArgsError<'a, C::Error>;
    fn into_run_params(self, runner: &'a PipelineRunner) -> Result<RunParams<'a>, Self::Error> {
        let component = self.0.resolve(runner).map_err(PackArgsError::NoComponent)?;
        let args = runner
            .pack_args(component, self.1)
            .map_err(|err| match err {
                PackArgsError::NoComponent(_) => unreachable!(),
                PackArgsError::MissingInput(v) => PackArgsError::MissingInput(v),
                PackArgsError::ExpectingPrimary => PackArgsError::ExpectingPrimary,
            })?;
        Ok(RunParams::new(component).with_args(args))
    }
}
impl<'a, C: ComponentSpecifier<PipelineRunner>, X: Into<Context<'a>>>
    IntoRunParams<'a, markers::ComponentSpecMarker> for (C, X)
{
    type Error = C::Error;
    fn into_run_params(self, runner: &'a PipelineRunner) -> Result<RunParams<'a>, Self::Error> {
        self.0
            .resolve(runner)
            .map(|c| RunParams::new(c).with_context(self.1))
    }
}
impl<'a, C: ComponentSpecifier<PipelineRunner>, A: Into<ComponentArgs>, X: Into<Context<'a>>>
    IntoRunParams<'a, markers::ArgListMarker> for (C, A, X)
{
    type Error = C::Error;
    fn into_run_params(self, runner: &'a PipelineRunner) -> Result<RunParams<'a>, Self::Error> {
        self.0.resolve(runner).map(|i| {
            RunParams::new(i)
                .with_args(self.1.into())
                .with_context(self.2)
        })
    }
}
impl<'a, C: ComponentSpecifier<PipelineRunner>, I: InputSpecifier, X: Into<Context<'a>>>
    IntoRunParams<'a, markers::InputMapMarker> for (C, I, X)
{
    type Error = PackArgsError<'a, C::Error>;
    fn into_run_params(self, runner: &'a PipelineRunner) -> Result<RunParams<'a>, Self::Error> {
        let component = self.0.resolve(runner).map_err(PackArgsError::NoComponent)?;
        let args = runner
            .pack_args(component, self.1)
            .map_err(|err| match err {
                PackArgsError::NoComponent(_) => unreachable!(),
                PackArgsError::MissingInput(v) => PackArgsError::MissingInput(v),
                PackArgsError::ExpectingPrimary => PackArgsError::ExpectingPrimary,
            })?;
        Ok(RunParams::new(component)
            .with_args(args)
            .with_context(self.2))
    }
}

/// Core error types that can occur during pipeline execution, such as invalid
/// component references, resource limits, and argument mismatches.
#[derive(Debug, Clone, Copy, Error)]
#[non_exhaustive]
pub enum RunErrorCause {
    /// The specified component ID does not exist
    #[error("No component {0}")]
    NoComponent(RunnerComponentId),

    /// Too many pipelines are currently running
    #[error("Too many pipelines ({0}) were already running")]
    TooManyRunning(usize),

    /// The number of provided arguments doesn't match what the component expects
    #[error("Expected {expected} arguments, got {given}")]
    ArgsMismatch { expected: usize, given: usize },
}

/// An error that occurs during pipeline execution, along with the parameters that caused it.
#[derive(Debug)]
pub struct RunErrorWithParams<'a> {
    pub cause: RunErrorCause,
    pub params: RunParams<'a>,
}

impl Display for RunErrorWithParams<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(&self.cause, f)
    }
}

impl std::error::Error for RunErrorWithParams<'_> {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.cause)
    }
}

/// An error that occured when calling [`PipelineRunner::run`].
#[derive(Debug, Error)]
pub enum RunError<'a, E> {
    /// Error occurred after parameters were created
    #[error(transparent)]
    WithParams(RunErrorWithParams<'a>),

    /// Error occurred during parameter conversion
    #[error(transparent)]
    FromConversion(E),
}

/// Input passed to [`ComponentContext`].
enum InputKind {
    /// No input data
    Empty,
    /// Single piece of input data
    Single(Arc<dyn Data>),
    /// An index into the partial state, along with a value popped from the multi-input vector
    Multiple(usize, Option<Arc<dyn Data>>),
}

/// Core context used to get input and submit output from a component body.
///
/// This contains all of the core functionality, but [`ComponentContext`] is often more convenient
/// because it contains the scope required to submit the results.
pub struct ComponentContextInner<'r> {
    runner: &'r PipelineRunner,
    component: &'r ComponentData,
    input: InputKind,
    callback: Option<Callback<'r>>,
    run_id: RunId,
    invoc: AtomicU32,
    /// Context to be passed in and shared between components.
    pub context: Context<'r>,
    finished: bool,
}

impl Debug for ComponentContextInner<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentContextInner")
            .field("runner", &(&self.runner as *const _))
            .field("comp_id", &self.comp_id())
            .field("run_id", &self.run_id)
            .field("invoc", &self.invoc)
            .finish_non_exhaustive()
    }
}

impl Drop for ComponentContextInner<'_> {
    fn drop(&mut self) {
        if !self.finished {
            self.finish();
        }
    }
}

impl<'r> ComponentContextInner<'r> {
    /// Get the component identifier of this .
    pub fn comp_id(&self) -> RunnerComponentId {
        RunnerComponentId::new(
            (self.component as *const ComponentData as usize
                - self.runner.components.as_ptr() as usize)
                / size_of::<ComponentData>(),
        )
    }

    /// Returns the unique identifier for this execution run.
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    /// Returns a reference to the pipeline runner.
    pub fn runner(&self) -> &'r PipelineRunner {
        self.runner
    }

    /// Returns the name of the current component.
    pub fn name(&self) -> &'r SmolStr {
        &self.component.name
    }

    /// Retrieve the input data from either a named channel or the primary one.
    pub fn get<'b>(&self, channel: impl Into<Option<&'b str>>) -> Option<Arc<dyn Data>> {
        if self.finished {
            tracing::error!("get() was called after finish() for a component");
            return None;
        }
        let req_channel = channel.into();
        match self.input {
            InputKind::Empty => None,
            InputKind::Single(ref data) => {
                let InputMode::Single { name, .. } = &self.component.input_mode else {
                    unreachable!()
                };
                (name.as_deref() == req_channel).then(|| data.clone())
            }
            InputKind::Multiple(run_idx, ref arg) => req_channel.and_then(|name| {
                let InputMode::Multiple { lookup, multi } = &self.component.input_mode else {
                    unreachable!()
                };
                if multi.as_ref().map(|x| &*x.0) == req_channel {
                    return arg.clone();
                }
                let field_idx = lookup.get(name)?.0;
                let num_fields = lookup.len();
                let lock = self.component.partial.lock().unwrap();
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

    /// Retrieve input data similarly to [`get`](Self::get), but in a `Result`.
    ///
    /// This function is more useful for chaining with [`LogErr`](crate::utils::LogErr) and let-else chaining.
    pub fn get_res<'b>(
        &self,
        channel: impl Into<Option<&'b str>>,
    ) -> Result<Arc<dyn Data>, DowncastInputError<'b>> {
        let channel = channel.into();
        self.get(channel)
            .ok_or(DowncastInputError::MissingInput(channel))
    }

    /// Retrieve and downcast input data to a specific type.
    pub fn get_as<'b, T: Data>(
        &self,
        channel: impl Into<Option<&'b str>>,
    ) -> Result<Arc<T>, DowncastInputError<'b>> {
        self.get_res(channel)?.downcast_arc().map_err(From::from)
    }

    /// Get the packed [`ComponentArgs`].
    ///
    /// This is only really useful for forwarding to another component with the same specified inputs.
    pub fn packed_args(&self) -> ComponentArgs {
        match &self.input {
            InputKind::Empty => ComponentArgs::empty(),
            InputKind::Single(arg) => ComponentArgs::single(arg.clone()),
            InputKind::Multiple(run_idx, last) => {
                let InputMode::Multiple { lookup, .. } = &self.component.input_mode else {
                    unreachable!()
                };
                let num_fields = lookup.len();
                let lock = self.component.partial.lock().unwrap();
                let mut vec =
                    lock.data[(*run_idx * num_fields)..((*run_idx + 1) * num_fields)].to_vec();
                if let Some(last) = last {
                    vec.push(Some(last.clone()));
                }
                ComponentArgs(vec)
            }
        }
    }

    /// Check if any components are listening on a given channel.
    pub fn listening<'b>(&self, channel: impl Into<Option<&'b str>>) -> bool {
        if self.finished {
            return false;
        }
        let channel: Option<&'b str> = channel.into();
        self.component
            .dependents
            .get(&channel.map(SmolStr::from))
            .is_some_and(|d| !d.is_empty())
    }

    /// Run a callback to submit to a channel, if there's a listener on the channel.
    pub fn submit_if_listening<'b, 's, D: IntoData, F: FnOnce() -> D>(
        &self,
        channel: impl Into<Option<&'b str>>,
        create: F,
        scope: &rayon::Scope<'s>,
    ) -> bool
    where
        'r: 's,
    {
        let channel = channel.into();
        let listening = self.listening(channel);
        if listening {
            let data = create().into_data();
            self.submit_impl(channel, data, scope);
        }
        listening
    }

    /// Publish a result on a given channel.
    ///
    /// This immediately runs any listening components if possible.
    #[inline(always)]
    pub fn submit<'b, 's>(
        &self,
        channel: impl Into<Option<&'b str>>,
        data: impl IntoData,
        scope: &rayon::Scope<'s>,
    ) where
        'r: 's,
    {
        self.submit_impl(channel.into(), data.into_data(), scope);
    }

    /// Internal implementation of `submit` that handles data distribution and scheduling.
    fn submit_impl<'s>(&self, channel: Option<&str>, data: Arc<dyn Data>, scope: &rayon::Scope<'s>)
    where
        'r: 's,
    {
        if self.invoc.load(Ordering::Relaxed) == u32::MAX {
            tracing::error!("submit() was called after finish() for a component");
            return;
        }
        let dependents = channel.map_or_else(
            || self.component.primary_dependents.as_slice(),
            |name| {
                self.component
                    .dependents
                    .get(name)
                    .map_or(&[], Vec::as_slice)
            },
        );
        for &(comp_id, channel) in dependents {
            let next_comp = &self.runner.components[comp_id.index()];
            match channel {
                InputChannel::Primary(multi) => self.spawn_next(
                    next_comp,
                    InputKind::Single(data.clone()),
                    multi.then(|| self.invoc.fetch_add(1, Ordering::Relaxed)),
                    scope,
                ),
                InputChannel::Multiple => {
                    let mut partial = next_comp.partial.lock().unwrap();
                    let partial = &mut *partial;
                    let len = if let InputMode::Multiple { lookup, .. } = &next_comp.input_mode {
                        lookup.len()
                    } else {
                        unreachable!()
                    };
                    'blk: {
                        for (n, (data_ref, prdata)) in partial
                            .data
                            .chunks_mut(len)
                            .zip(&mut partial.per_run)
                            .enumerate()
                        {
                            let Some(id) = &prdata.id else { continue };
                            if *id != self.run_id {
                                continue;
                            }
                            if data_ref.iter().all(Option::is_some) {
                                prdata.refs += 1;
                                self.spawn_next(
                                    next_comp,
                                    InputKind::Multiple(n, Some(data.clone())),
                                    Some(prdata.invoc),
                                    scope,
                                );
                                prdata.invoc += 1;
                            } else {
                                prdata.multi.push(data.clone());
                            }
                            break 'blk;
                        }
                        let (_, prdata, _) = partial.alloc(len);
                        prdata.id = Some(self.run_id.clone());
                        prdata.refs = 1;
                        prdata.multi.push(data.clone());
                    }
                }
                InputChannel::Numbered(idx) => {
                    let mut partial = next_comp.partial.lock().unwrap();
                    let partial = &mut *partial;
                    let (len, has_multi) =
                        if let InputMode::Multiple { lookup, multi } = &next_comp.input_mode {
                            (lookup.len(), multi.is_some())
                        } else {
                            unreachable!()
                        };
                    'blk: {
                        for (n, (data_ref, prdata)) in partial
                            .data
                            .chunks_mut(len)
                            .zip(&mut partial.per_run)
                            .enumerate()
                        {
                            let Some(id) = &prdata.id else { continue };
                            if id.starts_with(&self.run_id) {
                                continue;
                            }
                            let elem = &mut data_ref[idx];
                            assert!(elem.is_none(), "already submitted to a matching element?");
                            *elem = Some(data.clone());
                            if data_ref.iter().all(Option::is_some) {
                                if has_multi {
                                    for elem in prdata.multi.drain(..) {
                                        prdata.refs += 1;
                                        self.spawn_next(
                                            next_comp,
                                            InputKind::Multiple(n, Some(elem)),
                                            Some(prdata.invoc),
                                            scope,
                                        );
                                        prdata.invoc += 1;
                                    }
                                } else {
                                    prdata.refs += 1;
                                    self.spawn_next(
                                        next_comp,
                                        InputKind::Multiple(n, None),
                                        Some(prdata.invoc),
                                        scope,
                                    );
                                    prdata.invoc += 1;
                                }
                            }
                            break 'blk;
                        }
                        let (_, prdata, data_ref) = partial.alloc(len);
                        prdata.id = Some(self.run_id.clone());
                        data_ref[idx] = Some(data.clone());
                        if has_multi {
                            prdata.refs = 1;
                        }
                    }
                }
            }
        }
    }

    /// Spawn a new component execution in the pipeline.
    ///
    /// This internal method handles creating the new context and running it.
    fn spawn_next<'s>(
        &self,
        component: &'r ComponentData,
        input: InputKind,
        push_run: Option<u32>,
        scope: &rayon::Scope<'s>,
    ) where
        'r: 's,
    {
        let runner = self.runner;
        let decr = self.callback.clone();
        let context = self.context.clone();
        let mut run_id = self.run_id.clone();
        if let Some(run) = push_run {
            run_id.push(run);
        }
        scope.spawn(move |scope| {
            ComponentContextInner {
                input,
                runner,
                component,
                callback: decr,
                run_id,
                invoc: AtomicU32::new(0),
                context,
                finished: false,
            }
            .run(scope);
        });
    }

    /// Mark this component as finished and cleans up resources.
    ///
    /// After calling this method, all of the inputs will be inaccessible and
    /// submitting a value will be a no-op.
    ///
    /// This happens automatically when the context is dropped,
    /// but can be called explicitly for more precise control.
    pub fn finish(&mut self) {
        if std::mem::replace(&mut self.finished, true) {
            tracing::warn!("finish() was called twice for a component");
            return;
        }
        if let InputKind::Multiple(idx, _) = self.input {
            let mut partial = self.component.partial.lock().unwrap();
            let prdata = &mut partial.per_run[idx];
            prdata.refs -= 1;
            if prdata.refs == 0 {
                let InputMode::Multiple { lookup, .. } = &self.component.input_mode else {
                    unreachable!()
                };
                partial.free(idx, lookup.len());
            }
        }
        self.runner.cleanup_runs(self.component, &self.run_id);
        self.input = InputKind::Empty;
        if let Some(callback) = self.callback.take() {
            callback.call_if_unique(CleanupContext {
                runner: self.runner,
                run_id: self.run_id.base_run(),
                context: std::mem::take(&mut self.context),
            });
        }
        self.runner.running.fetch_sub(1, Ordering::AcqRel);
    }

    /// Create a tracing span for this component execution.
    pub fn tracing_span(&self) -> tracing::Span {
        tracing::info_span!("run", name = %self.name(), run = %self.run_id, component = %self.comp_id())
    }

    /// Run the component with tracing instrumentation.
    fn run<'s>(self, scope: &rayon::Scope<'s>)
    where
        'r: 's,
    {
        self.tracing_span().in_scope(|| {
            self.component
                .component
                .run(ComponentContext { inner: self, scope })
        });
    }
}
/// Context passed to components during execution.
///
/// Components can use this context to access their input data, submit output data,
/// defer operations to run later, and access pipeline-wide information.
///
/// A component is considered finished when either the context is dropped or finish()
/// is explicitly called. After finishing, the component can no longer submit outputs
/// or access inputs.
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
impl<'r> DerefMut for ComponentContext<'r, '_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<'s, 'r: 's> ComponentContext<'r, '_, 's> {
    /// Publish a result on a given channel.
    #[inline(always)]
    pub fn submit<'b>(&self, channel: impl Into<Option<&'b str>>, data: impl IntoData) {
        self.inner.submit(channel, data, self.scope);
    }

    /// Publish a result on a given channel, if there's a listener.
    #[inline(always)]
    pub fn submit_if_listening<'b, D: IntoData, F: FnOnce() -> D>(
        &self,
        channel: impl Into<Option<&'b str>>,
        create: F,
    ) {
        self.inner.submit_if_listening(channel, create, self.scope);
    }

    /// Defer an operation to run later on the thread pool.
    pub fn defer(self, op: impl FnOnce(ComponentContext<'r, '_, 's>) + Send + Sync + 'r) {
        let ComponentContext { inner, scope } = self;
        scope.spawn(move |scope| op(ComponentContext { inner, scope }));
    }
}

impl PipelineRunner {
    /// Executes a pipeline starting from a specified component. This is the main entry
    /// point for running pipelines, supporting direct parameter passing, components with
    /// data, and components with input maps.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use viking_vision::pipeline::prelude::for_test::{*, produce_component as some_component_no_args, consume_component as some_component_with_primary};
    /// # use std::sync::Arc;
    /// # fn some_component_with_named() -> Arc<dyn Component> {
    /// #     struct NamedInput;
    /// #     impl Component for NamedInput {
    /// #         fn inputs(&self) -> Inputs {
    /// #             Inputs::named([("input")])
    /// #         }
    /// #         fn output_kind(&self, _: Option<&str>) -> OutputKind {
    /// #             OutputKind::None
    /// #         }
    /// #         fn run<'s, 'r: 's>(&self, _: ComponentContext<'r, '_, 's>) {}
    /// #     }
    /// #     Arc::new(NamedInput)
    /// # }
    /// let mut runner = PipelineRunner::new();
    /// let no_args = runner.add_component("no-args", some_component_no_args()).unwrap();
    /// let primary = runner.add_component("primary", some_component_with_primary()).unwrap();
    /// let named = runner.add_component("named", some_component_with_named()).unwrap();
    /// rayon::scope(|scope| {
    ///     // Run with direct parameters
    ///     runner.run(RunParams::new(no_args), scope).unwrap();
    ///
    ///     // Run with component and data
    ///     runner.run((primary, "primary input".to_string()), scope).unwrap();
    ///
    ///     // Run with named inputs
    ///     runner.run((named, [("input", "named input".to_string())]), scope).unwrap();
    /// });
    /// ```
    #[inline(always)]
    pub fn run<'s, 'a: 's, M, P: IntoRunParams<'a, M>>(
        &'a self,
        params: P,
        scope: &rayon::Scope<'s>,
    ) -> Result<(), RunError<'a, P::Error>> {
        let params = params
            .into_run_params(self)
            .map_err(RunError::FromConversion)?;
        self.run_impl(params, scope).map_err(RunError::WithParams)
    }

    fn run_impl<'s, 'a: 's>(
        &'a self,
        params: RunParams<'a>,
        scope: &rayon::Scope<'s>,
    ) -> Result<(), RunErrorWithParams<'a>> {
        let running = self.running.fetch_add(1, Ordering::AcqRel);
        if params.max_running.is_some_and(|max| running >= max) {
            self.running.fetch_sub(1, Ordering::AcqRel);
            return Err(RunErrorWithParams {
                cause: RunErrorCause::TooManyRunning(running),
                params,
            });
        }
        let Some(data) = self.components.get(params.component.index()) else {
            return Err(RunErrorWithParams {
                cause: RunErrorCause::NoComponent(params.component),
                params,
            });
        };
        match (&data.input_mode, params.args.len()) {
            (InputMode::Single { .. }, n) => {
                if n != 1 {
                    return Err(RunErrorWithParams {
                        cause: RunErrorCause::ArgsMismatch {
                            expected: 1,
                            given: n,
                        },
                        params,
                    });
                }
            }
            (InputMode::Multiple { lookup, multi }, n) => {
                let expected = lookup.len() + usize::from(multi.is_some());
                if expected != n {
                    return Err(RunErrorWithParams {
                        cause: RunErrorCause::ArgsMismatch { expected, given: n },
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
            context,
        } = params;
        let run_id = self.run_id.fetch_add(1, Ordering::Relaxed);
        let run_id = RunId::new(run_id);
        let input = match args.len() {
            0 => InputKind::Empty,
            1 => InputKind::Single(args.0.pop().unwrap().unwrap()),
            len => {
                let mut lock = data.partial.lock().unwrap();
                let (idx, run, inputs) = lock.alloc(len);
                run.id = Some(run_id.clone());
                run.refs = 1;
                let arg = matches!(data.input_mode, InputMode::Multiple { multi: Some(_), .. })
                    .then(|| args.0.pop().unwrap().unwrap());
                for (to, from) in inputs.iter_mut().zip(&mut args.0) {
                    *to = from.take();
                }
                InputKind::Multiple(idx, arg)
            }
        };
        let data = &self.components[component.index()];
        scope.spawn(move |scope| {
            ComponentContextInner {
                runner: self,
                component: data,
                input,
                callback,
                run_id,
                invoc: AtomicU32::new(0),
                context,
                finished: false,
            }
            .run(scope);
        });
        Ok(())
    }

    /// Clean up resources after a component run completes.
    ///
    /// This internal method handles cleanup of component inputs and propagates to dependent components.
    fn cleanup_runs<'a>(&'a self, component: &'a ComponentData, prefix: &RunId) {
        for (name, deps) in std::iter::once((None, &component.primary_dependents))
            .chain(component.dependents.iter().map(|(k, v)| (Some(&**k), v)))
        {
            if !component.component.output_kind(name).is_multi() {
                continue;
            }
            for (comp, channel) in deps {
                let component = &self.components[comp.index()];
                match *channel {
                    InputChannel::Primary(_) => self.cleanup_runs(component, prefix),
                    InputChannel::Numbered(idx) => {
                        let len = if let InputMode::Multiple { lookup, .. } = &component.input_mode
                        {
                            lookup.len()
                        } else {
                            unreachable!()
                        };
                        let mut partial = component.partial.lock().unwrap();
                        let partial = &mut *partial;
                        for (n, (data, prd)) in partial
                            .data
                            .chunks(len)
                            .zip(&mut partial.per_run)
                            .enumerate()
                        {
                            if prd.id.as_ref().is_some_and(|id| id.starts_with(prefix)) {
                                if data[idx].is_none() {
                                    partial.free(n, len);
                                    self.cleanup_runs(component, prefix);
                                }
                                break;
                            }
                        }
                    }
                    InputChannel::Multiple => {
                        let len = if let InputMode::Multiple { lookup, .. } = &component.input_mode
                        {
                            lookup.len()
                        } else {
                            unreachable!()
                        };
                        let mut partial = component.partial.lock().unwrap();
                        let partial = &mut *partial;
                        for (n, prd) in partial.per_run.iter_mut().enumerate() {
                            if prd.id.as_ref().is_some_and(|id| id.starts_with(prefix)) {
                                prd.refs -= 1;
                                if prd.refs == 0 {
                                    partial.free(n, len);
                                    self.cleanup_runs(component, prefix);
                                }
                                break;
                            }
                        }
                    }
                }
            }
        }
    }
}
