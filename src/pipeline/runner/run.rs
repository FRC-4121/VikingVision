use super::*;
use crate::pipeline::ComponentSpecifier;
use crate::pipeline::component::{IntoData, OutputKind, TypeMismatch};
use crate::utils::LogErr;
use litemap::LiteMap;
use std::convert::Infallible;
use std::ops::{Deref, DerefMut};
use std::sync::LazyLock;
use supply::prelude::*;

#[derive(Debug)]
pub(crate) struct InputTree {
    pub vals: SmallVec<[Arc<dyn Data>; 2]>,
    pub next: Vec<Option<InputTree>>,
    pub remaining: u32,
    pub iter: u32,
    pub prev_done: bool,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct InputIndex(pub u32, pub u32);

#[derive(Debug, Default)]
pub(crate) struct MutableData {
    pub inputs: Vec<Option<InputTree>>,
    /// First open index
    pub first: usize,
}

#[derive(Debug)]
pub(crate) enum InputMode {
    Single {
        name: Option<SmolStr>,
    },
    Multiple {
        lookup: HashMap<SmolStr, InputIndex>,
        tree_shape: SmallVec<[u32; 2]>,
        mutable: Mutex<MutableData>,
    },
}
pub(super) struct PlaceholderData;
impl Data for PlaceholderData {}
pub(super) static PLACEHOLDER_DATA: LazyLock<Arc<dyn Data>> =
    LazyLock::new(|| Arc::new(PlaceholderData));

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId(pub SmallVec<[u32; 2]>);
impl Display for RunId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for v in &self.0 {
            if first {
                first = false;
                f.write_str("#")?;
            } else {
                f.write_str(".")?;
            }
            Display::fmt(v, f)?;
        }
        Ok(())
    }
}
impl RunId {
    /// Create a run ID with no branches.
    pub const fn new(run: u32) -> RunId {
        unsafe { Self(SmallVec::from_const_with_len_unchecked([run, 0], 1)) }
    }
    /// Get the base run that ran this.
    pub fn base_run(&self) -> u32 {
        self.0[0]
    }
}

/// Data associated with components.
pub struct ComponentData {
    pub component: Arc<dyn Component>,
    pub name: SmolStr,
    #[allow(clippy::type_complexity)]
    pub(crate) dependents:
        HashMap<Option<SmolStr>, Vec<(RunnerComponentId, InputIndex, Option<u32>)>>,
    pub(crate) input_mode: InputMode,
}
impl Debug for ComponentData {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentData")
            .field("dependents", &self.dependents)
            .field("name", &self.name)
            .field("input_mode", &self.input_mode)
            .finish_non_exhaustive()
    }
}

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
    /// An index into the partial state
    Multiple(SmallVec<[u32; 2]>),
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
    branch_count: Mutex<LiteMap<Option<SmolStr>, u32>>,
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
            .finish_non_exhaustive()
    }
}

impl Drop for ComponentContextInner<'_> {
    fn drop(&mut self) {
        if !self.finished {
            // self.finish();
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

    /// Get the run ID of this run.
    #[inline(always)]
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }

    /// Returns a reference to the pipeline runner.
    #[inline(always)]
    pub fn runner(&self) -> &'r PipelineRunner {
        self.runner
    }

    /// Returns the name of the current component.
    #[inline(always)]
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
        match &self.input {
            InputKind::Empty => None,
            InputKind::Single(data) => {
                let InputMode::Single { name, .. } = &self.component.input_mode else {
                    unreachable!()
                };
                (name.as_deref() == req_channel).then(|| data.clone())
            }
            InputKind::Multiple(branch) => req_channel.and_then(|name| {
                let InputMode::Multiple {
                    lookup, mutable, ..
                } = &self.component.input_mode
                else {
                    unreachable!()
                };
                let mut idx = *lookup.get(name)?;
                let (head, mut branch) = branch.split_first().unwrap_or((&0, &[]));
                let lock = mutable.lock().unwrap();
                let mut this = lock.inputs[*head as usize].as_ref().unwrap();
                loop {
                    if idx.0 == 0 {
                        return Some(this.vals[(idx.1) as usize].clone());
                    }
                    let b = branch.split_off_first().unwrap_or(&0);
                    // let s = shape.split_off_first().unwrap_or_else(|| {
                    //     tracing::error!(?idx, "empty shape");
                    //     panic!()
                    // });
                    idx.0 -= 1;
                    this = this.next.get(*b as usize).and_then(Option::as_ref).unwrap();
                    // last = *s;
                }
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
            InputKind::Multiple(branch) => {
                let InputMode::Multiple {
                    lookup,
                    tree_shape,
                    mutable,
                } = &self.component.input_mode
                else {
                    unreachable!()
                };
                let mut out = vec![PLACEHOLDER_DATA.clone(); lookup.len()];
                let mut out_slice = &mut out[..];
                let (head, mut tail) = branch.split_first().unwrap_or((&0, &[]));
                let mut last = 0;
                let lock = mutable.lock().unwrap();
                let mut tree = lock.inputs[*head as usize].as_ref().unwrap();
                for cum in tree_shape {
                    let b = tail.split_off_first().unwrap_or(&0);
                    let sz = cum - last;
                    last = *cum;
                    let head = out_slice.split_off_mut(..(sz as usize)).unwrap();
                    head.clone_from_slice(
                        &tree.vals[((sz * b) as usize)..((sz * (b + 1)) as usize)],
                    );
                    tree = tree.next[*b as usize].as_ref().unwrap();
                }
                ComponentArgs(out)
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
        let _guard = tracing::info_span!("submit", ?channel).entered();
        let dependents = self
            .component
            .dependents
            .get(&channel.map(SmolStr::from))
            .map_or(&[] as _, Vec::as_slice);
        if dependents.is_empty() {
            return;
        }
        let mut cloned;
        let run_id = match self.component.component.output_kind(channel) {
            OutputKind::None => {
                tracing::warn!(?channel, "submitted output to channel that wasn't expected");
                &self.run_id
            }
            OutputKind::Single => &self.run_id,
            OutputKind::Multiple => {
                let mut guard = self.branch_count.lock().unwrap();
                let b = guard.entry(channel.map(From::from)).or_insert(0);
                let old = *b;
                *b += 1;
                cloned = self.run_id.clone();
                cloned.0.push(old);
                &cloned
            }
        };
        for &(comp_id, index, ext) in dependents {
            let mut next_id = run_id.clone();
            next_id.0.extend(ext);
            let next_comp = &self.runner.components[comp_id.index()];
            match &next_comp.input_mode {
                InputMode::Single { .. } => {
                    self.spawn_next(next_comp, InputKind::Single(data.clone()), next_id, scope)
                }
                InputMode::Multiple {
                    tree_shape,
                    mutable,
                    ..
                } => {
                    let mut lock = mutable.lock().unwrap();
                    // this has to be written as a tail-recursive function because Rust's control-flow can't track the looping
                    #[allow(clippy::too_many_arguments)]
                    fn insert_arg<'s, 'r: 's>(
                        mut slice: &[u32],
                        mut shape: &[u32],
                        last_idx: u32,
                        index: usize,
                        inputs: &mut Vec<Option<InputTree>>,
                        data: Arc<dyn Data>,
                        mut path: SmallVec<[u32; 2]>,
                        prev_done: bool,
                        this: &ComponentContextInner<'r>,
                        scope: &rayon::Scope<'s>,
                        component: &'r ComponentData,
                        mut run_id: RunId,
                    ) {
                        let (Some(&idx), Some(&sum)) =
                            (slice.split_off_first(), shape.split_off_first())
                        else {
                            return;
                        };
                        let is_last = slice.is_empty();
                        let mut open = None;
                        let size = sum - last_idx;
                        for (n, i) in inputs.iter_mut().enumerate() {
                            let Some(tree) = i else {
                                open = Some(n);
                                continue;
                            };
                            if tree.iter == idx {
                                let done = prev_done && tree.remaining == 0;
                                if is_last {
                                    tree.vals[index] = data;
                                    tree.remaining -= 1;
                                    if done {
                                        maybe_run(
                                            shape.len(),
                                            &mut tree.next,
                                            &mut path,
                                            this,
                                            scope,
                                            component,
                                            &mut run_id,
                                        );
                                    }
                                } else {
                                    path.push(n as u32);
                                    insert_arg(
                                        slice,
                                        shape,
                                        sum,
                                        index,
                                        &mut tree.next,
                                        data,
                                        path,
                                        done,
                                        this,
                                        scope,
                                        component,
                                        run_id,
                                    );
                                }
                                return;
                            }
                        }
                        let mut vals = smallvec::smallvec![PLACEHOLDER_DATA.clone(); size as usize];
                        let remaining = if is_last {
                            vals[index] = data.clone();
                            size - 1
                        } else {
                            size
                        };
                        let new = InputTree {
                            vals,
                            next: Vec::new(),
                            iter: idx,
                            remaining,
                            prev_done,
                        };
                        let (inserted, new_inputs) = if let Some(n) = open {
                            let r = &mut inputs[n];
                            *r = Some(new);
                            (n, &mut r.as_mut().unwrap().next)
                        } else {
                            let n = inputs.len();
                            inputs.push(Some(new));
                            (n, &mut inputs[n].as_mut().unwrap().next)
                        };
                        path.push(inserted as u32);
                        let done = prev_done && remaining == 0;
                        if is_last {
                            if done {
                                maybe_run(
                                    shape.len(),
                                    new_inputs,
                                    &mut path,
                                    this,
                                    scope,
                                    component,
                                    &mut run_id,
                                );
                            }
                        } else {
                            insert_arg(
                                slice, shape, sum, index, new_inputs, data, path, done, this,
                                scope, component, run_id,
                            );
                        }
                    }
                    #[allow(clippy::ptr_arg)]
                    fn maybe_run<'s, 'r: 's>(
                        remaining: usize,
                        inputs: &mut Vec<Option<InputTree>>,
                        path: &mut SmallVec<[u32; 2]>,
                        this: &ComponentContextInner<'r>,
                        scope: &rayon::Scope<'s>,
                        component: &'r ComponentData,
                        run_id: &mut RunId,
                    ) {
                        let Some(next) = remaining.checked_sub(1) else {
                            this.spawn_next(
                                component,
                                InputKind::Multiple(path.clone()),
                                run_id.clone(),
                                scope,
                            );
                            return;
                        };
                        for (n, opt) in inputs.iter_mut().enumerate() {
                            let Some(tree) = opt else { continue };
                            if tree.remaining > 0 {
                                continue;
                            }
                            tree.prev_done = true;
                            path.push(n as _);
                            run_id.0.push(tree.iter);
                            maybe_run(next, &mut tree.next, path, this, scope, component, run_id);
                            path.pop();
                            run_id.0.pop();
                        }
                    }
                    insert_arg(
                        &run_id.0,
                        tree_shape,
                        0,
                        index.1 as _,
                        &mut lock.inputs,
                        data.clone(),
                        SmallVec::new(),
                        true,
                        self,
                        scope,
                        next_comp,
                        next_id.clone(),
                    );
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
        run_id: RunId,
        scope: &rayon::Scope<'s>,
    ) where
        'r: 's,
    {
        let runner = self.runner;
        let callback = self.callback.clone();
        let context = self.context.clone();
        scope.spawn(move |scope| {
            ComponentContextInner {
                input,
                runner,
                component,
                callback,
                context,
                run_id,
                branch_count: Mutex::new(LiteMap::new()),
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
        self.input = InputKind::Empty;
        if let Some(callback) = self.callback.take() {
            callback.call_if_unique(CleanupContext {
                runner: self.runner,
                run_id: self.run_id.0[0],
                context: std::mem::take(&mut self.context),
            });
        }
        // TODO: literally any level of cleanup
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
        let nargs = params.args.len();
        match &data.input_mode {
            InputMode::Single { .. } => {
                if nargs != 1 {
                    return Err(RunErrorWithParams {
                        cause: RunErrorCause::ArgsMismatch {
                            expected: 1,
                            given: nargs,
                        },
                        params,
                    });
                }
            }
            InputMode::Multiple { lookup, .. } => {
                let expected = lookup.len();
                if expected != nargs {
                    return Err(RunErrorWithParams {
                        cause: RunErrorCause::ArgsMismatch {
                            expected,
                            given: nargs,
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
            context,
        } = params;
        let run_id = self.run_id.fetch_add(1, Ordering::Relaxed);
        let input = match args.len() {
            0 => InputKind::Empty,
            1 => InputKind::Single(args.0.pop().unwrap()),
            _ => {
                let InputMode::Multiple {
                    tree_shape,
                    mutable,
                    ..
                } = &data.input_mode
                else {
                    unreachable!()
                };
                let mut indices = smallvec::smallvec![0; tree_shape.len()];
                let mut tree = build_tree(args.0.into_iter(), tree_shape);
                tree.iter = run_id;
                let mut lock = mutable.lock().unwrap();
                let n = lock.first;
                indices[0] = n as _;
                if n == lock.inputs.len() {
                    lock.first += 1;
                    lock.inputs.push(Some(tree));
                } else {
                    lock.inputs[n] = Some(tree);
                    lock.first = lock.inputs[(n + 1)..]
                        .iter()
                        .position(Option::is_none)
                        .map_or(lock.inputs.len(), |x| x + n);
                }
                InputKind::Multiple(indices)
            }
        };
        let data = &self.components[component.index()];
        scope.spawn(move |scope| {
            ComponentContextInner {
                runner: self,
                component: data,
                input,
                callback,
                context,
                run_id: RunId(smallvec::smallvec![run_id]),
                branch_count: Mutex::new(LiteMap::new()),
                finished: false,
            }
            .run(scope);
        });
        Ok(())
    }
}

fn build_tree(mut iter: std::vec::IntoIter<Arc<dyn Data>>, mut shape: &[u32]) -> InputTree {
    let mut root = InputTree {
        vals: SmallVec::new(),
        next: Vec::new(),
        remaining: 0,
        iter: 0,
        prev_done: true,
    };
    let mut tree = &mut root;
    let mut last = 0;
    while let Some(&sum) = shape.split_off_first() {
        let len = sum - last;
        last = sum;
        tree.vals.extend(iter.by_ref().take(len as _));
        tree.remaining = len;
        if !shape.is_empty() {
            tree.next = vec![Some(InputTree {
                vals: SmallVec::new(),
                next: Vec::new(),
                remaining: 0,
                iter: 0,
                prev_done: true,
            })];
            tree = tree.next[0].as_mut().unwrap();
        }
    }
    root
}
