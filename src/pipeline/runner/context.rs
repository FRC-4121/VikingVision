use super::*;
use crate::pipeline::component::{IntoData, OutputKind};
use litemap::LiteMap;
use std::mem::ManuallyDrop;
use std::num::NonZero;
use std::ops::{Deref, DerefMut};
use std::sync::LazyLock;

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
    pub context: Context<'r>,
}

impl Debug for ComponentContextInner<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentContextInner")
            .field("runner", &(&self.runner as *const _))
            .field("comp_id", &self.comp_id())
            .field("run_id", &self.run_id)
            .field("finished", &self.finished())
            .finish_non_exhaustive()
    }
}

impl Drop for ComponentContextInner<'_> {
    fn drop(&mut self) {
        if !self.finished() {
            self.finish(None);
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
        let _guard = tracing::info_span!("submit", ?channel).entered();
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
                                let done = prev_done && tree.remaining_inputs == 0;
                                if is_last {
                                    tree.vals[index] = data;
                                    tree.remaining_inputs -= 1;
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
                            remaining_inputs: remaining,
                            remaining_finish: shape.first().map_or(u32::MAX, |v| v - sum),
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
                            if tree.remaining_inputs > 0 {
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
    pub fn finish<'s>(&mut self, signal: Option<&rayon::Scope<'s>>)
    where
        'r: 's,
    {
        let _guard = tracing::info_span!("finish");
        if self.finished() {
            tracing::warn!("finish() was called twice for a component");
            return;
        }
        if let Some(scope) = signal {
            self.submit_impl("$finish", UNIT_ARC.clone(), scope);
        }
        let mut propagate = false;
        match (&self.component.input_mode, &self.input) {
            (InputMode::Single { refs, .. }, InputKind::Single(_)) => {
                let mut lock = refs.lock().unwrap();
                propagate = true;
                for (n, (k, v)) in lock.iter_mut().enumerate() {
                    if self.run_id.0.starts_with(k) {
                        if let Some(v2) = v.get().checked_sub(1).and_then(NonZero::new) {
                            *v = v2;
                            propagate = false;
                        } else {
                            lock.swap_remove(n);
                        }
                        break;
                    }
                }
            }
            (InputMode::Multiple { mutable, .. }, InputKind::Multiple(path)) => {
                fn cleanup(
                    inputs: &mut Vec<Option<InputTree>>,
                    remaining: &mut u32,
                    mut path: &[u32],
                ) -> (bool, bool) {
                    let Some(&idx) = path.split_off_first() else {
                        *remaining = 0;
                        return (true, false);
                    };
                    let idx = idx as usize;
                    let Some(tree) = inputs.get_mut(idx).and_then(Option::as_mut) else {
                        return (false, false);
                    };
                    let (unwind, propagate) =
                        cleanup(&mut tree.next, &mut tree.remaining_finish, path);
                    if !unwind
                        || tree.remaining_finish != 0
                        || tree.next.iter().any(Option::is_some)
                    {
                        return (false, propagate);
                    }
                    inputs[idx] = None;
                    while inputs.pop_if(|x| x.is_none()).is_some() {}
                    (true, true)
                }
                let fst = path[0] as usize;
                let mut lock = mutable.lock().unwrap();
                let unwind;
                let mut remaining = u32::MAX;
                (unwind, propagate) = cleanup(&mut lock.inputs, &mut remaining, path);
                if unwind && fst < lock.first {
                    lock.first = fst;
                }
            }
            _ => {
                tracing::error!(
                    id = %self.comp_id(),
                    "mismatched input mode and passed value"
                );
            }
        }
        if propagate {
            self.runner
                .propagate(self.component, &self.run_id.0, &tracing::Span::current());
        }
        self.input = InputKind::Empty;
        if let Some(callback) = self.callback.take() {
            tracing::trace!(
                count = Arc::strong_count(&callback) - 1,
                "decrementing refcount"
            );
            let is_last = callback.call_if_unique(CallbackContext {
                runner: self.runner,
                run_id: self.run_id.0[0],
                context: std::mem::take(&mut self.context),
            });
            if is_last {
                self.runner.running.fetch_sub(1, Ordering::AcqRel);
            }
        }
    }

    /// Create a tracing span for this component execution.
    pub fn tracing_span(&self) -> tracing::Span {
        tracing::info_span!("run", name = &**self.name(), run = %self.run_id, component = %self.comp_id())
    }

    /// Run the component with tracing instrumentation.
    pub(super) fn run<'s>(self, scope: &rayon::Scope<'s>)
    where
        'r: 's,
    {
        self.tracing_span().in_scope(|| {
            self.component.component.run(ComponentContext {
                inner: self,
                scope,
                signal: true,
            })
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
    pub signal: bool,
}
impl<'s, 'r: 's> Drop for ComponentContext<'_, 's, 'r> {
    fn drop(&mut self) {
        self.finish_signal(self.signal);
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
    pub fn explode(self) -> (ComponentContextInner<'r>, &'a rayon::Scope<'s>, bool) {
        let this = ManuallyDrop::new(self);
        unsafe { (std::ptr::read(&this.inner), this.scope, this.signal) }
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

    /// Finish, sending a signal
    #[inline(always)]
    pub fn finish(&mut self) {
        self.inner.finish(Some(self.scope));
    }

    #[inline(always)]
    pub fn finish_signal(&mut self, signal: bool) {
        self.inner.finish(signal.then_some(self.scope));
    }

    /// Defer an operation to run later on the thread pool.
    pub fn defer(self, op: impl FnOnce(ComponentContext<'_, 's, 'r>) + Send + Sync + 'r) {
        let (inner, scope, signal) = self.explode();
        scope.spawn(move |scope| {
            op(ComponentContext {
                inner,
                scope,
                signal,
            })
        });
    }
}

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
    /// Clean up the state for a finished component
    fn propagate(&self, component: &ComponentData, run_id: &[u32], parent: &tracing::Span) {
        let _guard =
            tracing::info_span!(parent: parent, "propagate", name = &*component.name, id = %self.component_id(component)).entered();
        let mut buf = Vec::new();
        for (id, index, _) in component.dependents.values().flatten() {
            let next = &self.components[id.index()];
            match &next.input_mode {
                InputMode::Single { refs, .. } => {
                    {
                        let mut lock = refs.lock().unwrap();
                        buf.extend(
                            lock.extract_if(.., |(k, v)| {
                                run_id.starts_with(k)
                                    && v.get()
                                        .checked_sub(1)
                                        .and_then(NonZero::new)
                                        .inspect(|v2| *v = *v2)
                                        .is_none()
                            })
                            .collect::<Vec<_>>(),
                        );
                    }
                    for (k, _) in buf.drain(..) {
                        self.propagate(next, &k, parent);
                    }
                }
                InputMode::Multiple { mutable, .. } => {
                    #[allow(clippy::too_many_arguments)]
                    fn cleanup(
                        remaining: &mut u32,
                        inputs: &mut Vec<Option<InputTree>>,
                        idx: usize,
                        target: usize,
                        run_id: &[u32],
                        runner: &PipelineRunner,
                        component: &ComponentData,
                        parent: &tracing::Span,
                    ) -> bool {
                        let i = run_id.get(idx).copied();
                        let mut all = true;
                        if idx < target {
                            for opt in &mut *inputs {
                                let Some(tree) = opt else { continue };
                                if i.is_some_and(|i| i != tree.iter) {
                                    continue;
                                }
                                if !cleanup(
                                    &mut tree.remaining_finish,
                                    &mut tree.next,
                                    idx + 1,
                                    target,
                                    run_id,
                                    runner,
                                    component,
                                    parent,
                                ) || tree.remaining_finish > 0
                                    || tree.next.iter().any(Option::is_some)
                                {
                                    all = false;
                                    continue;
                                }
                                *opt = None;
                            }
                        } else {
                            *remaining -= 1;
                            if *remaining > 0 {
                                return false;
                            }
                            for opt in &mut *inputs {
                                let Some(tree) = opt else { continue };
                                if i.is_some_and(|i| i != tree.iter) {
                                    continue;
                                }
                                if tree.remaining_finish > 0
                                    || tree.next.iter().any(Option::is_some)
                                {
                                    all = false;
                                    continue;
                                }
                                let mut all_children = true;
                                let mut any = false;
                                let mut run_id = run_id.to_vec();
                                for opt in &mut tree.next {
                                    let Some(input) = opt else { continue };
                                    let rem = dft(input, &mut run_id, runner, component, parent);
                                    if rem {
                                        *opt = None;
                                        any = true;
                                    } else {
                                        all_children = false;
                                    }
                                }
                                if !any {
                                    *opt = None;
                                }
                                all &= all_children;
                            }
                        }
                        while inputs.pop_if(|x| x.is_none()).is_some() {}
                        all
                    }
                    fn dft(
                        input: &mut InputTree,
                        run_id: &mut Vec<u32>,
                        runner: &PipelineRunner,
                        component: &ComponentData,
                        parent: &tracing::Span,
                    ) -> bool {
                        if input.remaining_finish > 0 {
                            return false;
                        }
                        run_id.push(input.iter);
                        let mut all = true;
                        let mut any = false;
                        for opt in &mut input.next {
                            let Some(next) = opt else { continue };
                            let rem = dft(next, run_id, runner, component, parent);
                            if rem {
                                *opt = None;
                                any = true;
                            } else {
                                all = false;
                            }
                        }
                        if !any {
                            runner.propagate(component, run_id, parent);
                        }
                        run_id.pop();
                        all
                    }
                    let mut lock = mutable.lock().unwrap();
                    let mut remaining = u32::MAX;
                    cleanup(
                        &mut remaining,
                        &mut lock.inputs,
                        0,
                        index.0 as _,
                        run_id,
                        self,
                        next,
                        parent,
                    );
                }
            }
        }
    }
    /// Assert that the runner state is clean.
    ///
    /// This should return `Ok(())` when no pipelines are currently running. If inputs are leaked,
    /// error-level logs will be emitted describing the state.
    pub fn assert_clean(&self) -> Result<(), LeakedInputs> {
        let _guard = tracing::info_span!("assert_clean");
        let mut res = Ok(());
        for (n, comp) in self.components.iter().enumerate() {
            match &comp.input_mode {
                InputMode::Single { refs, .. } => {
                    let lock = refs.lock().unwrap();
                    if !lock.is_empty() {
                        tracing::error!(id = %RunnerComponentId::new(n), name = &*comp.name, inputs = ?lock, "leaked single-input component");
                        res = Err(LeakedInputs);
                    }
                }
                InputMode::Multiple { mutable, .. } => {
                    let lock = mutable.lock().unwrap();
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
