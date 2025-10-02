use super::*;
use crate::pipeline::ComponentSpecifier;
use crate::pipeline::component::TypeMismatch;
use crate::utils::LogErr;
use litemap::LiteMap;
use std::convert::Infallible;
use std::num::NonZero;
use std::ops::Deref;
use std::sync::LazyLock;
use supply::prelude::*;

#[derive(Debug)]
pub(crate) struct InputTree {
    pub vals: SmallVec<[Arc<dyn Data>; 2]>,
    pub next: Vec<Option<InputTree>>,
    pub remaining_inputs: u32,
    pub remaining_finish: u32,
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
        refs: Mutex<Vec<(SmallVec<[u32; 2]>, NonZero<u32>)>>,
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

/// A unique identifier for an execution of a component.
///
/// Each component will be called with a particular [`RunId`] at most once. They are
/// not unique between different components, and their length is not guaranteed to be the same (if a component takes multiple inputs).
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId(pub SmallVec<[u32; 2]>);
impl Display for RunId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut first = true;
        for v in &self.0 {
            if first {
                first = false;
                f.write_str(":")?;
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

/// Data associated with components in the graph.
pub struct ComponentData {
    pub component: Arc<dyn Component>,
    pub name: SmolStr,
    pub(crate) dependents: HashMap<SmolStr, Vec<(RunnerComponentId, InputIndex, Option<u32>)>>,
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

/// Context passed to a post-run callback.
#[derive(Debug)]
pub struct CallbackContext<'r> {
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
    fn call_if_unique(self: Arc<Self>, ctx: CallbackContext<'r>) -> bool;
}
impl<'r, F: FnOnce(CallbackContext<'r>) + Send + Sync + 'r> CallbackInner<'r> for F {
    fn call_if_unique(self: Arc<Self>, ctx: CallbackContext<'r>) -> bool {
        if let Some(this) = Arc::into_inner(self) {
            let _guard = tracing::info_span!("callback", run_id = ctx.run_id);
            this(ctx);
            true
        } else {
            false
        }
    }
}
struct NoopCallback;
impl<'r> CallbackInner<'r> for NoopCallback {
    fn call_if_unique(self: Arc<Self>, _ctx: CallbackContext<'r>) -> bool {
        Arc::into_inner(self).is_some()
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
        callback: impl FnOnce(CallbackContext) + Send + Sync + 'a,
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
    /// #         fn output_kind(&self, _: &str) -> OutputKind {
    /// #             OutputKind::None
    /// #         }
    /// #         fn run<'s, 'r: 's>(&self, _: ComponentContext<'_, 's, 'r>) {}
    /// #     }
    /// #     Arc::new(NamedInput)
    /// # }
    /// let mut graph = PipelineGraph::new();
    /// let no_args = graph.add_named_component(some_component_no_args(), "no-args").unwrap();
    /// let primary = graph.add_named_component(some_component_with_primary(), "primary").unwrap();
    /// let named = graph.add_named_component(some_component_with_named(), "named").unwrap();
    /// let (resolver, runner) = graph.compile().unwrap();
    /// let [no_args, primary, named] = resolver.get_all([no_args, primary, named]).map(Option::unwrap);
    /// rayon::scope(|scope| {
    ///     // Run with direct parameters
    ///     runner.run(no_args, scope).unwrap();
    ///
    ///     // Run with component and data
    ///     runner.run((primary, "primary input".to_string()), scope).unwrap();
    ///
    ///     // Run with named inputs
    ///     runner.run((named, [("input", "named input".to_string())]), scope).unwrap();
    ///
    ///     // alternatively, look up by name
    ///     runner.run("no-args", scope).unwrap();
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
        let callback = Some(callback.unwrap_or_else(|| Arc::new(NoopCallback)));
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
        remaining_inputs: 0,
        remaining_finish: 0,
        iter: 0,
        prev_done: true,
    };
    let mut tree = &mut root;
    let mut last = 0;
    while let Some(&sum) = shape.split_off_first() {
        let len = sum - last;
        last = sum;
        tree.vals.extend(iter.by_ref().take(len as _));
        tree.remaining_inputs = len;
        if !shape.is_empty() {
            tree.next = vec![Some(InputTree {
                vals: SmallVec::new(),
                next: Vec::new(),
                remaining_inputs: 0,
                remaining_finish: 0,
                iter: 0,
                prev_done: true,
            })];
            tree = tree.next[0].as_mut().unwrap();
        }
    }
    root
}
