use super::*;
use crate::pipeline::component::TypeMismatch;
use crate::utils::LogErr;
use std::convert::Infallible;
use std::ops::Deref;

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
    type Error;
    fn into_run_params(self, runner: &'a PipelineRunner) -> Result<RunParams<'a>, Self::Error>;
}
impl<'a> IntoRunParams<'a, ()> for RunParams<'a> {
    type Error = Infallible;
    fn into_run_params(self, _runner: &'a PipelineRunner) -> Result<RunParams<'a>, Self::Error> {
        Ok(self)
    }
}
impl<'a, A: Into<ComponentArgs>> IntoRunParams<'a, markers::ArgListMarker> for (ComponentId, A) {
    type Error = Infallible;
    fn into_run_params(self, _runner: &'a PipelineRunner) -> Result<RunParams<'a>, Self::Error> {
        Ok(RunParams::new(self.0).with_args(self.1.into()))
    }
}
impl<'a, I: InputSpecifier> IntoRunParams<'a, markers::InputMapMarker> for (ComponentId, I) {
    type Error = PackArgsError<'a>;
    fn into_run_params(self, runner: &'a PipelineRunner) -> Result<RunParams<'a>, Self::Error> {
        let args = runner.pack_args(self.0, self.1)?;
        Ok(RunParams::new(self.0).with_args(args))
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

/// An error that could arise from running a pipeline, after the `RunParams` have been created.
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

/// An error, either before or after the run parameters were created.
#[derive(Debug, Error)]
pub enum RunError<'a, E> {
    /// We created the parameters, and then there was a common error.
    #[error(transparent)]
    WithParams(RunErrorWithParams<'a>),
    /// An error specific to the parameter creation.
    #[error(transparent)]
    FromConversion(E),
}

enum InputKind {
    Empty,
    Single(Arc<dyn Data>),
    Multiple(usize, Option<Arc<dyn Data>>),
}

/// Context passed to components, without the threadpool scope.
///
/// In order to defer tasks to the threadpool, the context has to be able to be separated from the scope, and a new `ComponentContext` can be created with the new scope.
/// This type has a destructor that tells all dependent components that no more data will come from here, so we need to make sure that this inner context isn't dropped during that transition.
pub struct ComponentContextInner<'r> {
    runner: &'r PipelineRunner,
    component: &'r ComponentData,
    input: InputKind,
    decr: Arc<Cleanup<'r>>,
    run_id: RunId,
    invoc: AtomicU32,
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
        use tracing::subscriber::*;
        with_default(NoSubscriber::new(), || self.finish());
    }
}
impl<'r> ComponentContextInner<'r> {
    /// Get the ID of this component. This is mostly useful for logging.
    pub fn comp_id(&self) -> ComponentId {
        ComponentId(
            (self.component as *const ComponentData as usize
                / self.runner.components.as_ptr() as usize)
                / size_of::<ComponentData>(),
        )
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
        &self.component.name
    }
    /// Get the current value from a given stream.
    pub fn get<'b>(&self, stream: impl Into<Option<&'b str>>) -> Option<Arc<dyn Data>> {
        if self.finished {
            tracing::error!("get() was called after finish() for a component");
            return None;
        }
        let req_stream = stream.into();
        match self.input {
            InputKind::Empty => None,
            InputKind::Single(ref data) => {
                let InputMode::Single { name, .. } = &self.component.input_mode else {
                    unreachable!()
                };
                (name.as_deref() == req_stream).then(|| data.clone())
            }
            InputKind::Multiple(run_idx, ref arg) => req_stream.and_then(|name| {
                let InputMode::Multiple { lookup, multi } = &self.component.input_mode else {
                    unreachable!()
                };
                if multi.as_ref().map(|x| x.0.as_str()) == req_stream {
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
    /// Same as [`get`](Self::get) but returns a `Result` that implements [`LogErr`].
    pub fn get_res<'b>(
        &self,
        stream: impl Into<Option<&'b str>>,
    ) -> Result<Arc<dyn Data>, DowncastInputError<'b>> {
        let stream = stream.into();
        self.get(stream)
            .ok_or(DowncastInputError::MissingInput(stream))
    }
    /// Get the current value from a given stream and attempt to downcast it.
    ///
    /// For even more convenience, [`DowncastInputError::log_err`] and the let-else pattern can be used.
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
        if self.invoc.load(Ordering::Relaxed) == u32::MAX {
            tracing::error!("submit() was called after finish() for a component");
            return;
        }
        let dependents = stream.map_or_else(
            || self.component.primary_dependents.as_slice(),
            |name| {
                self.component
                    .dependents
                    .get(name)
                    .map_or(&[], Vec::as_slice)
            },
        );
        for &(comp_id, stream) in dependents {
            let next_comp = &self.runner.components[comp_id.0];
            match stream {
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
        let decr = self.decr.clone();
        let mut run_id = self.run_id.clone();
        if let Some(run) = push_run {
            run_id.push(run);
        }
        scope.spawn(move |scope| {
            ComponentContextInner {
                input,
                runner,
                component,
                decr,
                run_id,
                invoc: AtomicU32::new(0),
                finished: false,
            }
            .run(scope);
        });
    }
    /// Signal that we're done with this component.
    ///
    /// After this is called, [`submit`](Self::submit) will become a no-op and [`get`](Self::get) will return `None`.
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
    }
    /// Get the info-level span for this run.
    pub fn tracing_span(&self) -> tracing::Span {
        tracing::info_span!("run", name = %self.name(), run = %self.run_id, component = %self.comp_id())
    }
    /// Run the component with tracing instrumentation (and possibly more in the future).
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
    /// Publish a result on a given stream.
    #[inline(always)]
    pub fn submit<'b>(&self, stream: impl Into<Option<&'b str>>, data: Arc<dyn Data>) {
        self.inner.submit(stream, data, self.scope);
    }
    pub fn finish(&mut self) {
        self.inner.finish();
    }
    /// Defer an operation to run later on the threadpool.
    pub fn defer(self, op: impl FnOnce(ComponentContext<'r, '_, 's>) + Send + Sync + 'r) {
        let ComponentContext { inner, scope } = self;
        scope.spawn(move |scope| op(ComponentContext { inner, scope }));
    }
}

impl PipelineRunner {
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
        let Some(data) = self.components.get(params.component.0) else {
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
        } = params;
        let decr = Arc::new(Cleanup {
            runner: self,
            callback,
        });
        let run_id = RunId::new(self.run_id.fetch_add(1, Ordering::Relaxed));
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
        let data = &self.components[component.0];
        scope.spawn(move |scope| {
            ComponentContextInner {
                runner: self,
                component: data,
                input,
                decr,
                run_id,
                invoc: AtomicU32::new(0),
                finished: false,
            }
            .run(scope);
        });
        Ok(())
    }
    fn cleanup_runs<'a>(&'a self, component: &'a ComponentData, prefix: &RunId) {
        for (name, deps) in std::iter::once((None, &component.primary_dependents))
            .chain(component.dependents.iter().map(|(k, v)| (Some(&**k), v)))
        {
            if !component.component.output_kind(name).is_multi() {
                continue;
            }
            for (comp, channel) in deps {
                let component = &self.components[comp.0];
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
