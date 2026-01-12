use super::*;
use crate::pipeline::component::{IntoData, OutputKind};
use litemap::LiteMap;
use std::mem::ManuallyDrop;
use std::ops::{Deref, DerefMut};
use std::sync::LazyLock;
use vv_utils::common_types::{PipelineId, PipelineName};

static UNIT_ARC: LazyLock<Arc<dyn Data>> = LazyLock::new(|| Arc::new(()));

/// Input passed to [`ComponentContext`].
pub(super) enum InputKind {
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
    pub(super) runner: &'r PipelineRunner,
    pub(super) component: &'r ComponentData,
    pub(super) input: InputKind,
    pub(super) callback: Option<Callback<'r>>,
    pub(super) run_id: RunId,
    pub(super) branch_count: Mutex<LiteMap<SmolStr, u32>>,
    /// Context to be passed in and shared between components.
    #[cfg(feature = "supply")]
    pub context: Context<'r>,
}

impl Debug for ComponentContextInner<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let mut s = f.debug_struct("ComponentContextInner");
        s.field("runner", &(&self.runner as *const _))
            .field("comp_id", &self.comp_id())
            .field("run_id", &self.run_id)
            .field("finished", &self.finished());
        #[cfg(feature = "supply")]
        s.field("context", &self.context);
        s.finish_non_exhaustive()
    }
}

impl Drop for ComponentContextInner<'_> {
    fn drop(&mut self) {
        if !self.finished() {
            tracing::warn!("component dropped without finishing");
        }
    }
}

impl<'r> ComponentContextInner<'r> {
    /// Get the component identifier of this component.
    #[inline(always)]
    pub fn comp_id(&self) -> RunnerComponentId {
        self.runner.component_id(self.component)
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

    /// Return if a component finished.
    #[inline(always)]
    pub fn finished(&self) -> bool {
        self.callback.is_none()
    }

    /// Request the [`PipelineId`] from the context.
    ///
    /// This is a convenience function to avoid having to import and name
    /// [`PipelineId`], along with not having to feature-gate based on `supply`'s presence.
    #[cfg(feature = "supply")]
    pub fn pipeline_id(&self) -> Option<PipelineId> {
        use supply::ProviderExt;
        self.context.request::<PipelineId>()
    }
    /// Request the [`PipelineName`] from the context.
    ///
    /// This is a convenience function to avoid having to import and name
    /// [`PipelineName`], along with not having to feature-gate based on `supply`'s presence.
    #[cfg(feature = "supply")]
    pub fn pipeline_name(&self) -> Option<PipelineName<'_>> {
        use supply::ProviderExt;
        self.context.request::<PipelineName>()
    }
    /// Request the [`PipelineId`] from the context.
    ///
    /// This is a convenience function to avoid having to import and name
    /// [`PipelineId`], along with not having to feature-gate based on `supply`'s presence.
    #[cfg(not(feature = "supply"))]
    pub fn pipeline_id(&self) -> Option<PipelineId> {
        None
    }
    /// Request the [`PipelineName`] from the context.
    ///
    /// This is a convenience function to avoid having to import and name
    /// [`PipelineName`], along with not having to feature-gate based on `supply`'s presence.
    #[cfg(not(feature = "supply"))]
    pub fn pipeline_name(&self) -> Option<PipelineName<'_>> {
        None
    }

    /// Get the set of inputs for this component.
    ///
    /// See [`ComponentData::available_inputs`].
    pub fn available_inputs(&self) -> lazy_maps::InputSet<'r> {
        self.component.available_inputs()
    }

    /// Get a map of the listeners for this component.
    ///
    /// See [`ComponentData::listeners`].
    pub fn listeners(&self) -> lazy_maps::ListenerMap<'r> {
        self.component.listeners()
    }

    /// Get the map of input channels to their indices, if this component takes a tree as input.
    ///
    /// See [`ComponentData::input_indices`].
    pub fn input_indices(&self) -> Option<lazy_maps::InputIndexMap<'r>> {
        self.component.input_indices()
    }

    /// Retrieve the input data from either a named channel or the primary one.
    pub fn get<'b>(&self, channel: impl Into<Option<&'b str>>) -> Option<Arc<dyn Data>> {
        if self.finished() {
            tracing::error!("get() was called after finish() for a component");
            return None;
        }
        let req_channel = channel.into();
        let _guard = tracing::error_span!("get", channel = req_channel).entered();
        match &self.input {
            InputKind::Empty => None,
            InputKind::Single(data) => {
                let exp = match &self.component.input_mode {
                    InputMode::Single {
                        name: Some((name, false)),
                        ..
                    } => Some(&**name),
                    InputMode::Single {
                        name: Some((_, true)) | None,
                        ..
                    } => None,
                    _ => None,
                };
                (exp == req_channel).then(|| data.clone())
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
                let Ok(lock) = mutable.lock() else {
                    tracing::warn!("attempted to read from a poisoned component");
                    return None;
                };
                let mut this = lock
                    .inputs
                    .get(*head as usize)
                    .and_then(Option::as_ref)
                    .unwrap_or_else(|| {
                        tracing::error!(idx = *head as usize, "missing input in tree");
                        panic!("missing input in tree");
                    });
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
                    this = this
                        .next
                        .get(*b as usize)
                        .and_then(Option::as_ref)
                        .unwrap_or_else(|| {
                            tracing::error!(idx = *b as usize, "missing child in tree");
                            panic!("missing child in tree");
                        });
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
                    ..
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
    pub fn listening(&self, channel: &str) -> bool {
        if self.finished() {
            return false;
        }
        self.component
            .dependents
            .get(channel)
            .is_some_and(|d| !d.is_empty())
    }

    /// Run a callback to submit to a channel, if there's a listener on the channel.
    pub fn submit_if_listening<'s, D: IntoData, F: FnOnce() -> D>(
        &self,
        channel: &str,
        create: F,
        scope: &rayon::Scope<'s>,
    ) -> bool
    where
        'r: 's,
    {
        if channel.starts_with('$') {
            tracing::warn!(channel, "submitted to a special-use channel");
        }
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
    pub fn submit<'s>(&self, channel: &str, data: impl IntoData, scope: &rayon::Scope<'s>)
    where
        'r: 's,
    {
        if channel.starts_with('$') {
            tracing::warn!(channel, "submitted to a special-use channel");
        }
        self.submit_impl(channel, data.into_data(), scope);
    }

    /// Internal implementation of `submit` that handles data distribution and scheduling.
    fn submit_impl<'s>(&self, channel: &str, data: Arc<dyn Data>, scope: &rayon::Scope<'s>)
    where
        'r: 's,
    {
        let _guard = tracing::error_span!("submit", ?channel).entered();
        let dependents = self
            .component
            .dependents
            .get(channel)
            .map_or(&[] as _, Vec::as_slice);
        if dependents.is_empty() {
            return;
        }
        let mut cloned;
        let run_id = match self.component.component.output_kind(channel) {
            OutputKind::None => {
                if !channel.starts_with('$') {
                    tracing::warn!(channel, "submitted output to channel that wasn't expected");
                }
                &self.run_id
            }
            OutputKind::Single => &self.run_id,
            OutputKind::Multiple => {
                let mut guard = self.branch_count.lock().unwrap();
                let b = guard.entry(channel.into()).or_insert(0);
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
            let _guard =
                tracing::error_span!("next", %comp_id, name = &*next_comp.name, ?index).entered();
            let mut deferred = Vec::new();
            match &next_comp.input_mode {
                InputMode::Single { name, .. } => {
                    let mut arg = data.clone();
                    if matches!(name, Some((_, true))) {
                        arg = Arc::new(InputTree {
                            vals: smallvec::smallvec![arg],
                            next: Vec::new(),
                            branch_id: 0,
                            remaining_finish: 0,
                            remaining_inputs: 0,
                        });
                    }
                    self.spawn_next(next_comp, InputKind::Single(arg), next_id, scope)
                }
                InputMode::Multiple {
                    tree_shape,
                    mutable,
                    broadcast,
                    ..
                } => {
                    let Ok(mut lock) = mutable.lock() else {
                        tracing::warn!("attempted to submit to a poisoned component");
                        continue;
                    };
                    // this has to be written as a tail-recursive function because Rust's control-flow can't track the looping
                    #[allow(clippy::too_many_arguments)]
                    fn insert_arg<'s, 'r: 's>(
                        mut slice: &[u32],
                        mut shape: &[u32],
                        index: usize,
                        inputs: &mut Vec<Option<InputTree>>,
                        broadcast: bool,
                        data: Arc<dyn Data>,
                        mut path: SmallVec<[u32; 2]>,
                        deferred: &mut Vec<(SmallVec<[u32; 2]>, RunId)>,
                        prev_done: bool,
                        mut run_id: RunId,
                    ) {
                        let (Some(&idx), Some(&sum)) =
                            (slice.split_off_first(), shape.split_off_first())
                        else {
                            return;
                        };
                        let is_last = slice.is_empty();
                        let mut open = None;
                        let size = sum - shape.first().unwrap_or(&0);
                        for (n, i) in inputs.iter_mut().enumerate() {
                            let Some(tree) = i else {
                                open = Some(n);
                                continue;
                            };
                            if tree.branch_id == idx {
                                let done = prev_done && tree.remaining_inputs == 0;
                                if is_last {
                                    tree.vals[index] = data;
                                    tree.remaining_inputs -= 1;
                                    if done {
                                        maybe_run(
                                            shape.len(),
                                            &mut tree.next,
                                            &mut path,
                                            deferred,
                                            &mut run_id,
                                        );
                                    }
                                } else {
                                    path.push(n as u32);
                                    insert_arg(
                                        slice,
                                        shape,
                                        index,
                                        &mut tree.next,
                                        broadcast,
                                        data,
                                        path,
                                        deferred,
                                        done,
                                        run_id,
                                    );
                                }
                                return;
                            }
                        }
                        let mut vals = smallvec::smallvec![PLACEHOLDER_DATA.clone(); size as usize];
                        let mut remaining_inputs = size;
                        let mut remaining_finish = sum;
                        if is_last {
                            vals[index] = data.clone();
                            remaining_inputs -= 1;
                        }
                        if broadcast && shape.is_empty() {
                            remaining_finish = u32::MAX;
                        }
                        let new = InputTree {
                            vals,
                            next: Vec::new(),
                            branch_id: idx,
                            remaining_inputs,
                            remaining_finish,
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
                        let done = prev_done && remaining_inputs == 0;
                        if is_last {
                            if done {
                                maybe_run(
                                    shape.len(),
                                    new_inputs,
                                    &mut path,
                                    deferred,
                                    &mut run_id,
                                );
                            }
                        } else {
                            insert_arg(
                                slice, shape, index, new_inputs, broadcast, data, path, deferred,
                                done, run_id,
                            );
                        }
                    }
                    #[allow(clippy::ptr_arg)]
                    fn maybe_run<'s, 'r: 's>(
                        remaining: usize,
                        inputs: &mut Vec<Option<InputTree>>,
                        path: &mut SmallVec<[u32; 2]>,
                        deferred: &mut Vec<(SmallVec<[u32; 2]>, RunId)>,
                        run_id: &mut RunId,
                    ) {
                        let Some(next) = remaining.checked_sub(1) else {
                            deferred.push((path.clone(), run_id.clone()));
                            // this.spawn_next(
                            //     component,
                            //     InputKind::Multiple(path.clone()),
                            //     run_id.clone(),
                            //     scope,
                            // );
                            return;
                        };
                        for (n, opt) in inputs.iter_mut().enumerate() {
                            let Some(tree) = opt else { continue };
                            if tree.remaining_inputs > 0 {
                                continue;
                            }
                            path.push(n as _);
                            run_id.0.push(tree.branch_id);
                            maybe_run(next, &mut tree.next, path, deferred, run_id);
                            path.pop();
                            run_id.0.pop();
                        }
                    }
                    insert_arg(
                        &run_id.0,
                        tree_shape,
                        index.1 as _,
                        &mut lock.inputs,
                        broadcast.is_none(),
                        data.clone(),
                        SmallVec::new(),
                        &mut deferred,
                        broadcast.is_none(),
                        next_id.clone(),
                    );
                    drop(lock);
                    for (path, run_id) in deferred.drain(..) {
                        self.spawn_next(next_comp, InputKind::Multiple(path), run_id, scope);
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
        run_id: RunId,
        scope: &rayon::Scope<'s>,
    ) where
        'r: 's,
    {
        let runner = self.runner;
        let callback = self.callback.clone();
        #[cfg(feature = "supply")]
        let context = self.context.clone();
        ComponentContextInner {
            input,
            runner,
            component,
            callback,
            #[cfg(feature = "supply")]
            context,
            run_id,
            branch_count: Mutex::new(LiteMap::new()),
        }
        .spawn(scope);
    }

    /// Mark this component as finished and cleans up resources.
    ///
    /// After calling this method, all of the inputs will be inaccessible and
    /// submitting a value will be a no-op.
    ///
    /// This happens automatically when the context is dropped,
    /// but can be called explicitly for more precise control.
    pub fn finish<'s>(&mut self, scope: &rayon::Scope<'s>)
    where
        'r: 's,
    {
        let _guard = tracing::error_span!("finish");
        if self.finished() {
            tracing::warn!("finish() was called twice for a component");
            return;
        }
        self.submit_impl("$finish", UNIT_ARC.clone(), scope);
        match (&self.component.input_mode, &self.input) {
            (InputMode::Single { .. }, InputKind::Single(_)) => {}
            (InputMode::Multiple { mutable, .. }, InputKind::Multiple(path)) => {
                fn cleanup(
                    inputs: &mut Vec<Option<InputTree>>,
                    remaining: &mut u32,
                    mut path: &[u32],
                ) -> bool {
                    let Some(&idx) = path.split_off_first() else {
                        *remaining = 0;
                        return true;
                    };
                    let idx = idx as usize;
                    let Some(tree) = inputs.get_mut(idx).and_then(Option::as_mut) else {
                        return false;
                    };
                    let unwind = cleanup(&mut tree.next, &mut tree.remaining_finish, path);
                    if !unwind
                        || tree.remaining_finish != 0
                        || tree.next.iter().any(Option::is_some)
                    {
                        return false;
                    }
                    inputs[idx] = None;
                    while inputs.pop_if(|x| x.is_none()).is_some() {}
                    true
                }
                let fst = path[0] as usize;
                if let Ok(mut lock) = mutable.lock() {
                    let mut remaining = u32::MAX;
                    let unwind = cleanup(&mut lock.inputs, &mut remaining, path);
                    if unwind && fst < lock.first {
                        lock.first = fst;
                    }
                }
            }
            (InputMode::Multiple { .. }, InputKind::Single(_)) => {} // already cleaned up
            _ => {
                tracing::error!(
                    id = %self.comp_id(),
                    "mismatched input mode and passed value"
                );
            }
        }
        self.runner.post_propagate(
            self.component,
            &self.run_id.0,
            true,
            &tracing::Span::current(),
            #[cfg(feature = "supply")]
            &self.context,
            &self.callback,
            scope,
        );
        self.input = InputKind::Empty;
        if let Some(callback) = self.callback.take() {
            tracing::trace!(
                count = Arc::strong_count(&callback) - 1,
                "decrementing refcount"
            );
            let is_last = callback.call_if_unique(CallbackContext {
                runner: self.runner,
                run_id: self.run_id.0[0],
                #[cfg(feature = "supply")]
                context: std::mem::take(&mut self.context),
            });
            if is_last {
                self.runner.running.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }

    /// Create a tracing span for this component execution.
    pub fn tracing_span(&self) -> tracing::Span {
        tracing::error_span!(
            target: crate::component_filter::COMPONENT_RUN_TARGET,
            "run",
            name = &**self.name(),
            run = %self.run_id,
            component = %self.comp_id(),
            "component.index" = self.comp_id().index()
        )
    }

    /// Run the component with tracing instrumentation.
    pub(super) fn spawn<'s>(self, scope: &rayon::Scope<'s>)
    where
        'r: 's,
    {
        self.runner
            .pre_propagate(self.component, &self.run_id.0, &tracing::Span::current());
        scope.spawn(move |scope| {
            self.tracing_span().in_scope(|| {
                self.component
                    .component
                    .run(ComponentContext { inner: self, scope })
            });
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
pub struct ComponentContext<'a, 's, 'r: 's> {
    pub inner: ComponentContextInner<'r>,
    pub scope: &'a rayon::Scope<'s>,
}
impl<'s, 'r: 's> Drop for ComponentContext<'_, 's, 'r> {
    fn drop(&mut self) {
        self.finish();
    }
}
impl<'r> Deref for ComponentContext<'_, '_, 'r> {
    type Target = ComponentContextInner<'r>;
    fn deref(&self) -> &Self::Target {
        &self.inner
    }
}
impl DerefMut for ComponentContext<'_, '_, '_> {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.inner
    }
}

impl<'a, 's, 'r: 's> ComponentContext<'a, 's, 'r> {
    /// Get the inner context, scope, and signal flag without calling the [`Drop`] implementation
    pub fn explode(self) -> (ComponentContextInner<'r>, &'a rayon::Scope<'s>) {
        let this = ManuallyDrop::new(self);
        unsafe { (std::ptr::read(&this.inner), this.scope) }
    }
    /// Publish a result on a given channel.
    #[inline(always)]
    pub fn submit(&self, channel: &str, data: impl IntoData) {
        self.inner.submit(channel, data, self.scope);
    }

    /// Publish a result on a given channel, if there's a listener.
    #[inline(always)]
    pub fn submit_if_listening<D: IntoData, F: FnOnce() -> D>(&self, channel: &str, create: F) {
        self.inner.submit_if_listening(channel, create, self.scope);
    }

    /// Finish this component.
    #[inline(always)]
    pub fn finish(&mut self) {
        self.inner.finish(self.scope);
    }

    /// Defer an operation to run later on the thread pool.
    pub fn defer(self, op: impl FnOnce(ComponentContext<'_, 's, 'r>) + Send + Sync + 'r) {
        let (inner, scope) = self.explode();
        scope.spawn(move |scope| op(ComponentContext { inner, scope }));
    }
}

/// ZST error for [`PipelineRunner::assert_clean`]
#[derive(Debug, Clone, Copy, PartialEq, Error)]
#[error("Runner has leaked inputs")]
pub struct LeakedInputs;

impl PipelineRunner {
    fn component_id(&self, component: &ComponentData) -> RunnerComponentId {
        RunnerComponentId::new(
            (component as *const ComponentData as usize - self.components.as_ptr() as usize)
                / size_of::<ComponentData>(),
        )
    }
    fn pre_propagate(&self, component: &ComponentData, run_id: &[u32], parent: &tracing::Span) {
        let _guard =
            tracing::error_span!(parent: parent, "pre_propagate", name = &*component.name, id = %self.component_id(component)).entered();
        for (id, index, _) in component.dependents.values().flatten() {
            let next = &self.components[id.index()];
            let _guard =
                tracing::error_span!("next", name = &*next.name, id = %id, ?index).entered();
            if let InputMode::Multiple {
                ref mutable,
                broadcast,
                ref tree_shape,
                ..
            } = next.input_mode
            {
                fn insert(
                    remaining: &mut u32,
                    inputs: &mut Vec<Option<InputTree>>,
                    idx: usize,
                    target: usize,
                    run_id: &[u32],
                    tree_shape: &[u32],
                ) {
                    let i = run_id[idx];
                    let sum = tree_shape[idx];
                    let size = sum - tree_shape.get(1).unwrap_or(&0);
                    if idx < target {
                        let mut empty = None;
                        for (n, opt) in inputs.iter_mut().enumerate() {
                            let Some(tree) = opt else { continue };
                            if i != tree.branch_id {
                                empty = Some(n);
                                continue;
                            }
                            insert(
                                &mut tree.remaining_finish,
                                &mut tree.next,
                                idx + 1,
                                target,
                                run_id,
                                tree_shape,
                            );
                            return;
                        }
                        let new = InputTree {
                            vals: smallvec::smallvec![PLACEHOLDER_DATA.clone(); size as _],
                            next: Vec::new(),
                            branch_id: i,
                            remaining_inputs: size,
                            remaining_finish: sum,
                        };
                        let tree = if let Some(n) = empty {
                            inputs[n] = Some(new);
                            inputs[n].as_mut().unwrap()
                        } else {
                            inputs.push(Some(new));
                            inputs.last_mut().unwrap().as_mut().unwrap()
                        };
                        insert(
                            &mut tree.remaining_finish,
                            &mut tree.next,
                            idx + 1,
                            target,
                            run_id,
                            tree_shape,
                        );
                    } else {
                        *remaining += 1;
                    }
                }
                let Ok(mut lock) = mutable.lock() else {
                    continue;
                };
                let mut remaining = 0;
                insert(
                    &mut remaining,
                    &mut lock.inputs,
                    0,
                    run_id.len() - 1,
                    run_id,
                    tree_shape,
                );
                if broadcast.is_some() {
                    continue;
                }
            }
            self.pre_propagate(next, run_id, parent);
        }
    }
    /// Clean up the state for a finished component
    #[allow(clippy::too_many_arguments)]
    fn post_propagate<'s, 'r: 's>(
        &'r self,
        component: &'r ComponentData,
        run_id: &[u32],
        can_extra: bool,
        parent: &tracing::Span,
        #[cfg(feature = "supply")] context: &Context<'r>,
        callback: &Option<Callback<'r>>,
        scope: &rayon::Scope<'s>,
    ) {
        let _guard =
            tracing::error_span!(parent: parent, "post_propagate", name = &*component.name, id = %self.component_id(component)).entered();
        for (id, index, push) in component.dependents.values().flatten() {
            let next = &self.components[id.index()];
            let _guard =
                tracing::error_span!("next", name = &*next.name, id = %id, ?index).entered();
            match &next.input_mode {
                InputMode::Single { .. } => {}
                InputMode::Multiple {
                    mutable, broadcast, ..
                } => {
                    fn tree_complete(tree: &InputTree) -> bool {
                        tree.remaining_finish == 0
                            && tree
                                .next
                                .iter()
                                .all(|opt| opt.as_ref().is_none_or(tree_complete))
                    }
                    fn cleanup<'s, 'r: 's>(
                        remaining: &mut u32,
                        inputs: &mut Vec<Option<InputTree>>,
                        idx: usize,
                        target: usize,
                        extra: usize,
                        run_id: &[u32],
                        runner: &'r PipelineRunner,
                        component: &'r ComponentData,
                        broadcast: Option<u32>,
                        #[cfg(feature = "supply")] context: &Context<'r>,
                        callback: &Option<Callback<'r>>,
                        scope: &rayon::Scope<'s>,
                    ) -> bool {
                        let i = run_id.get(idx).copied();
                        let mut all = true;
                        if idx < target {
                            for opt in &mut *inputs {
                                let Some(tree) = opt else { continue };
                                if i.is_some_and(|i| i != tree.branch_id) {
                                    continue;
                                }
                                if idx + extra >= target {
                                    tree.remaining_finish -= 1;
                                    if idx + extra == target {
                                        *remaining -= 1;
                                    }
                                }
                                let popped = cleanup(
                                    &mut tree.remaining_finish,
                                    &mut tree.next,
                                    idx + 1,
                                    target,
                                    extra,
                                    run_id,
                                    runner,
                                    component,
                                    broadcast,
                                    #[cfg(feature = "supply")]
                                    context,
                                    callback,
                                    scope,
                                );
                                if !popped
                                    || tree.remaining_finish > 0
                                    || (broadcast.is_none()
                                        && tree.next.iter().any(Option::is_some))
                                {
                                    all = false;
                                    continue;
                                }
                                if let Some(to) = broadcast {
                                    match idx.cmp(&(to as usize)) {
                                        std::cmp::Ordering::Less => {
                                            if tree_complete(tree) {
                                                *opt = None
                                            } else {
                                                all = false
                                            }
                                        }
                                        std::cmp::Ordering::Equal => {
                                            if tree_complete(tree) {
                                                let ctx = ComponentContextInner {
                                                    runner,
                                                    component,
                                                    input: InputKind::Single(Arc::new(
                                                        opt.take().unwrap(),
                                                    )),
                                                    callback: callback.clone(),
                                                    #[cfg(feature = "supply")]
                                                    context: context.clone(),
                                                    branch_count: Mutex::new(LiteMap::new()),
                                                    run_id: RunId(
                                                        run_id[..(to as usize + 1)].into(),
                                                    ),
                                                };
                                                ctx.spawn(scope);
                                            } else {
                                                all = false;
                                            }
                                        }
                                        std::cmp::Ordering::Greater => {}
                                    }
                                } else {
                                    *opt = None;
                                }
                            }
                        } else {
                            // *remaining = remaining.saturating_sub(1);
                            // *remaining -= 1;
                            // if *remaining > 0 {
                            //     return false;
                            // }
                            for opt in &mut *inputs {
                                let Some(tree) = opt else { continue };
                                if i.is_some_and(|i| i != tree.branch_id) {
                                    continue;
                                }
                                tree.remaining_finish -= 1;
                                if extra == 0 {
                                    *remaining -= 1;
                                }
                                if tree.remaining_finish > 0
                                    || (broadcast.is_none()
                                        && tree.next.iter().any(Option::is_some))
                                {
                                    all = false;
                                    continue;
                                }
                                let mut all_children = true;
                                for opt in &mut tree.next {
                                    let Some(input) = opt else { continue };
                                    let popped = dft(input, broadcast.is_none());
                                    if popped {
                                        if broadcast.is_none() {
                                            *opt = None;
                                        }
                                    } else {
                                        all_children = false;
                                        all = false;
                                        if broadcast.is_none() {
                                            break;
                                        }
                                    }
                                }
                                if all_children {
                                    if let Some(to) = broadcast {
                                        match idx.cmp(&(to as usize)) {
                                            std::cmp::Ordering::Less => {
                                                if tree_complete(tree) {
                                                    *opt = None
                                                } else {
                                                    all = false
                                                }
                                            }
                                            std::cmp::Ordering::Equal => {
                                                if tree_complete(tree) {
                                                    let ctx = ComponentContextInner {
                                                        runner,
                                                        component,
                                                        input: InputKind::Single(Arc::new(
                                                            opt.take().unwrap(),
                                                        )),
                                                        callback: callback.clone(),
                                                        #[cfg(feature = "supply")]
                                                        context: context.clone(),
                                                        branch_count: Mutex::new(LiteMap::new()),
                                                        run_id: RunId(
                                                            run_id[..(to as usize + 1)].into(),
                                                        ),
                                                    };
                                                    ctx.spawn(scope);
                                                } else {
                                                    all = false;
                                                }
                                            }
                                            std::cmp::Ordering::Greater => {}
                                        }
                                    } else {
                                        *opt = None;
                                    }
                                }
                            }
                        }
                        while inputs.pop_if(|x| x.is_none()).is_some() {}
                        all
                    }
                    fn dft<'s, 'r: 's>(input: &mut InputTree, broadcast: bool) -> bool {
                        if input.remaining_finish > 0 {
                            return false;
                        }
                        let mut all = true;
                        for opt in &mut input.next {
                            let Some(next) = opt else { continue };
                            let popped = dft(next, broadcast);
                            if popped {
                                if broadcast {
                                    *opt = None;
                                }
                            } else {
                                all = false;
                            }
                        }
                        all
                    }
                    let Ok(mut lock) = mutable.lock() else {
                        continue;
                    };
                    let mut remaining = u32::MAX;
                    let extra = if can_extra {
                        id.flag() as usize + push.is_some() as usize
                    } else {
                        0
                    };
                    let target = (run_id.len() + extra - 1).min(index.0 as _);
                    cleanup(
                        &mut remaining,
                        &mut lock.inputs,
                        0,
                        target,
                        target.saturating_sub(run_id.len() - 1),
                        run_id,
                        self,
                        next,
                        *broadcast,
                        #[cfg(feature = "supply")]
                        context,
                        callback,
                        scope,
                    );
                    if broadcast.is_some() {
                        continue; // skip the propagation
                    }
                }
            }
            if id.flag() {
                self.post_propagate(
                    next,
                    run_id,
                    false,
                    parent,
                    #[cfg(feature = "supply")]
                    context,
                    callback,
                    scope,
                );
            }
        }
    }
    /// Assert that the runner state is clean.
    ///
    /// This should return `Ok(())` when no pipelines are currently running. If inputs are leaked,
    /// error-level logs will be emitted describing the state.
    pub fn assert_clean(&self) -> Result<(), LeakedInputs> {
        let _guard = tracing::error_span!("assert_clean");
        let mut res = Ok(());
        for (n, comp) in self.components.iter().enumerate() {
            match &comp.input_mode {
                InputMode::Single { .. } => {}
                InputMode::Multiple { mutable, .. } => {
                    let lock = mutable.lock().unwrap_or_else(|err| {
                        tracing::error!(id = %RunnerComponentId::new(n), name = &*comp.name, "poisoned component lock");
                        err.into_inner()
                    });
                    if lock.inputs.iter().any(Option::is_some) {
                        tracing::error!(id = %RunnerComponentId::new(n), name = &*comp.name, inputs = ?lock.inputs, "leaked multi-input component");
                        res = Err(LeakedInputs);
                    }
                }
            }
        }
        res
    }
}
