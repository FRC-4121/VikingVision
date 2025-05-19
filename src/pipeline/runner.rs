use super::component::{Component, Data};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::collections::hash_map::Entry;
use std::fmt::{self, Debug, Formatter};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use thiserror::Error;

/// Newtype wrapper around a component ID.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(transparent)]
pub struct ComponentId(pub usize);

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct RunId {
    pub invoc: u32,
    pub tree: Vec<SmallVec<[u32; 2]>>,
}
impl RunId {
    pub fn starts_with(&self, other: &RunId) -> bool {
        self.invoc == other.invoc && self.tree.starts_with(&other.tree)
    }
    pub fn is_pred_of(&self, next: &RunId) -> bool {
        self.tree.len() + 1 == next.tree.len() && next.starts_with(self)
    }
    pub fn push(&mut self, vals: SmallVec<[u32; 2]>) {
        self.tree.push(vals);
    }
}

struct PartialData {
    /// A vector of partial data. This should be chunked by the number of inputs
    data: Vec<Option<Arc<dyn Data>>>,
    /// Additional info for each chunk
    ids: Vec<Option<RunId>>,
    /// First open index
    first: usize,
}
impl PartialData {
    #[allow(clippy::type_complexity)]
    fn alloc(&mut self, len: usize) -> (usize, &mut Option<RunId>, &mut [Option<Arc<dyn Data>>]) {
        let idx = self.first;
        if self.first == self.ids.len() {
            self.first += 1;
            self.ids.push(None);
            self.data.resize(self.data.len() + len, None);
        } else {
            self.first = self.ids[idx..]
                .iter()
                .position(Option::is_none)
                .map_or(self.ids.len(), |i| i + idx);
        }
        (
            idx,
            &mut self.ids[idx],
            &mut self.data[(idx * len)..((idx + 1) * len)],
        )
    }
    fn free(&mut self, idx: usize, len: usize) {
        self.ids[idx] = None;
        self.data[(idx * len)..((idx + 1) * len)].fill(None);
        if idx < self.first {
            self.first = idx
        }
    }
}

struct ComponentData {
    /// The actual component
    component: Arc<dyn Component>,
    /// Components dependent on a primary stream
    primary_dependents: Vec<(ComponentId, Option<usize>)>,
    /// Components dependent on a secondary stream
    dependents: HashMap<String, Vec<(ComponentId, Option<usize>)>>,
    /// Lookups from the input names to the partial indices
    in_lookup: HashMap<String, usize>,
    /// Locked partial data
    partial: Mutex<PartialData>,
    /// This is true if we need to add a row to our run-id.
    multi_input: bool,
    /// This is true if a primary input has already been registered.
    has_single: bool,
}

struct DecrementRunCount<'a>(&'a PipelineRunner);
impl Drop for DecrementRunCount<'_> {
    fn drop(&mut self) {
        self.0.running.fetch_sub(1, Ordering::AcqRel);
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum AddComponentError {
    #[error("Name already exists with component ID {}", .0.0)]
    AlreadyExists(ComponentId),
    #[error("Empty component name")]
    EmptyName,
    #[error("Non-alphanumeric character in character {index} of {name:?}")]
    InvalidName { name: String, index: usize },
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum AddDependencyError<'a> {
    #[error("Publishing component {} doesn't exist", .0.0)]
    NoPublisher(ComponentId),
    #[error("Subscribing component {} doesn't exist", .0.0)]
    NoSubscriber(ComponentId),
    #[error("Can't create a self-loop")]
    SelfLoop,
    #[error("Publishing component doesn't have a {}", if let Some(name) = .0 { format!("named stream {name:?}") } else { "primary output stream".to_string() })]
    NoPubStream(Option<&'a str>),
    #[error("Input {0:?} has already been attached")]
    DuplicateNamedInput(&'a str),
    #[error("Primary input has already been attached")]
    DuplicatePrimaryInput,
    #[error("Components can't have both primary and named inputs")]
    InputTypeMix,
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
#[derive(Default)]
pub struct PipelineRunner {
    components: Vec<ComponentData>,
    lookup: HashMap<String, ComponentId>,
    running: AtomicUsize,
    run_id: AtomicU32,
}
impl Debug for PipelineRunner {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("PipelineRunner")
            .field("lookup", &self.lookup)
            .field("running", &self.running)
            .field("run_id", &self.run_id)
            .finish_non_exhaustive()
    }
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
    pub fn components(&self) -> &HashMap<String, ComponentId> {
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
    /// This is the main entry point for running components. This invokes the given component with some data, along with a [scope](rayon::Scope) to run it in.
    pub fn run<'s, 'a: 's>(
        &'a self,
        id: ComponentId,
        data: Arc<dyn Data>,
        scope: &rayon::Scope<'s>,
    ) {
        self.running.fetch_add(1, Ordering::AcqRel);
        self.run_impl(id, data, scope);
    }
    /// Same as [`run`](Self::run), but with a limit on how many pipelines can be running at once. If `limit` or more are already running, this'll return `Err` with the current number of running pipelines.
    pub fn run_limited<'s, 'a: 's>(
        &'a self,
        id: ComponentId,
        data: Arc<dyn Data>,
        scope: &rayon::Scope<'s>,
        limit: usize,
    ) -> Result<(), usize> {
        let old = self.running.fetch_add(1, Ordering::AcqRel);
        if old >= limit {
            self.running.fetch_sub(1, Ordering::AcqRel);
            return Err(old);
        }
        self.run_impl(id, data, scope);
        Ok(())
    }
    fn run_impl<'s, 'a: 's>(
        &'a self,
        id: ComponentId,
        data: Arc<dyn Data>,
        scope: &rayon::Scope<'s>,
    ) {
        let decr = Arc::new(DecrementRunCount(self));
        let run_id = RunId {
            invoc: self.run_id.fetch_add(1, Ordering::Relaxed),
            tree: Vec::new(),
        };
        scope.spawn(move |scope| {
            self.components[id.0].component.run(ComponentContext {
                runner: self,
                comp_id: id,
                input: InputKind::Single(data),
                scope,
                decr,
                run_id,
                invoc: AtomicU32::new(0),
            });
        });
    }
    /// Try to add a new component, returning the ID of one with the same name if there's a conflict
    pub fn add_component(
        &mut self,
        name: String,
        component: Arc<dyn Component>,
    ) -> Result<ComponentId, AddComponentError> {
        if name.is_empty() {
            return Err(AddComponentError::EmptyName);
        }
        if let Some((index, _)) = name.char_indices().find(|(_, c)| !c.is_alphanumeric()) {
            return Err(AddComponentError::InvalidName { name, index });
        }
        match self.lookup.entry(name) {
            Entry::Occupied(e) => Err(AddComponentError::AlreadyExists(*e.get())),
            Entry::Vacant(e) => {
                let value = ComponentId(self.components.len());
                self.components.push(ComponentData {
                    component,
                    primary_dependents: Vec::new(),
                    dependents: HashMap::new(),
                    in_lookup: HashMap::new(),
                    partial: Mutex::new(PartialData {
                        data: Vec::new(),
                        ids: Vec::new(),
                        first: 0,
                    }),
                    multi_input: false,
                    has_single: false,
                });
                e.insert(value);
                Ok(value)
            }
        }
    }
    /// Add a dependency between two components.
    pub fn add_dependency<'a>(
        &mut self,
        pub_id: ComponentId,
        pub_stream: Option<&'a str>,
        sub_id: ComponentId,
        sub_stream: Option<&'a str>,
    ) -> Result<(), AddDependencyError<'a>> {
        if pub_id.0 < self.components.len() {
            return Err(AddDependencyError::NoPublisher(pub_id));
        }
        if sub_id.0 < self.components.len() {
            return Err(AddDependencyError::NoSubscriber(pub_id));
        }
        if pub_id == sub_id {
            return Err(AddDependencyError::SelfLoop);
        }
        let [c1, c2] = self
            .components
            .get_disjoint_mut([pub_id.0, sub_id.0])
            .unwrap();
        let kind = c1.component.output_kind(pub_stream);
        if kind.is_none() {
            return Err(AddDependencyError::NoPubStream(pub_stream));
        }
        if kind.is_multi() {
            c2.multi_input = true;
        }
        #[allow(clippy::collapsible_else_if)]
        if let Some(name) = sub_stream {
            if c2.has_single {
                return Err(AddDependencyError::InputTypeMix);
            }
            let idx = c2.in_lookup.len();
            match c2.in_lookup.entry(name.to_string()) {
                Entry::Occupied(_) => return Err(AddDependencyError::DuplicateNamedInput(name)),
                Entry::Vacant(e) => e.insert(idx),
            };
            if let Some(name) = pub_stream {
                c1.dependents
                    .entry(name.to_string())
                    .or_default()
                    .push((sub_id, Some(idx)));
            } else {
                c1.primary_dependents.push((sub_id, Some(idx)))
            }
        } else {
            if c2.has_single {
                return Err(AddDependencyError::DuplicatePrimaryInput);
            }
            if !c2.in_lookup.is_empty() {
                return Err(AddDependencyError::InputTypeMix);
            }
            if let Some(name) = pub_stream {
                c1.dependents
                    .entry(name.to_string())
                    .or_default()
                    .push((sub_id, None));
            } else {
                c1.primary_dependents.push((sub_id, None))
            }
        }
        Ok(())
    }
}

enum InputKind {
    Single(Arc<dyn Data>),
    Multiple(usize),
}

/// Context passed to components.
pub struct ComponentContext<'r, 'a, 's> {
    runner: &'r PipelineRunner,
    comp_id: ComponentId,
    input: InputKind,
    scope: &'a rayon::Scope<'s>,
    decr: Arc<DecrementRunCount<'r>>,
    run_id: RunId,
    invoc: AtomicU32,
}
impl Debug for ComponentContext<'_, '_, '_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentContext")
            .field("runner", &(&self.runner as *const _))
            .field("comp_id", &self.comp_id)
            .field("run_id", &self.run_id)
            .field("invoc", &self.invoc)
            .finish_non_exhaustive()
    }
}
impl<'s, 'a, 'r: 's> ComponentContext<'r, 'a, 's> {
    /// Get the ID of this component. This is mostly useful for logging.
    pub fn comp_id(&self) -> ComponentId {
        self.comp_id
    }
    /// Get the ID of this run. This will be unique for each time this component is called.
    pub fn run_id(&self) -> &RunId {
        &self.run_id
    }
    /// Get the rayon [`Scope`](rayon::Scope) to allow spawning scoped threads.
    pub fn scope(&self) -> &'a rayon::Scope<'s> {
        self.scope
    }
    /// Get the [`PipelineRunner`] that's calling this component.
    pub fn runner(&self) -> &'r PipelineRunner {
        self.runner
    }
    /// Get the current result from a given stream.
    pub fn get<'b>(&self, stream: impl Into<Option<&'b str>>) -> Option<Arc<dyn Data>> {
        match (stream.into(), &self.input) {
            (Some(name), InputKind::Multiple(run_idx)) => {
                let component = &self.runner.components[self.comp_id.0];
                let field_idx = *component.in_lookup.get(name)?;
                let num_fields = component.in_lookup.len();
                let lock = component.partial.lock().unwrap_or_else(|e| e.into_inner());
                let index = run_idx * num_fields + field_idx;
                Some(
                    lock.data[index]
                        .as_ref()
                        .expect("All fields should be initialized here!")
                        .clone(),
                )
            }
            (None, InputKind::Single(data)) => Some(data.clone()),
            _ => None,
        }
    }
    /// Publish a result on a given stream.
    #[inline(always)]
    pub fn submit<'b>(&self, stream: impl Into<Option<&'b str>>, data: Arc<dyn Data>) {
        self.submit_impl(stream.into(), data);
    }
    fn submit_impl(&self, stream: Option<&str>, data: Arc<dyn Data>) {
        let component = &self.runner.components[self.comp_id.0];
        let dependents = stream.map_or_else(
            || component.primary_dependents.as_slice(),
            |name| component.dependents.get(name).map_or(&[], Vec::as_slice),
        );
        let run = self.invoc.fetch_add(1, Ordering::Relaxed);
        for &(comp_id, stream) in dependents {
            let next_comp = &self.runner.components[comp_id.0];
            if let Some(stream) = stream {
                let num_fields = next_comp.in_lookup.len();
                let mut lock = next_comp.partial.lock().unwrap_or_else(|e| e.into_inner());
                let lock = &mut *lock;
                let mut new_data = Vec::new();
                let mut new_ids = Vec::new();
                for (n, (in_data, id)) in lock
                    .data
                    .chunks_mut(num_fields)
                    .zip(&mut lock.ids)
                    .enumerate()
                {
                    let Some(id) = id else { continue };
                    if !next_comp.multi_input {
                        if self.run_id == *id {
                            in_data[stream] = Some(data.clone());
                            if in_data.iter().all(Option::is_some) {
                                self.spawn_next(component, comp_id, InputKind::Multiple(n), None);
                            }
                        }
                        continue;
                    }
                    if !self.run_id.is_pred_of(id) {
                        continue;
                    }
                    let last_row = id.tree.last_mut().expect("Partials should exist here!");
                    if last_row[stream] == u32::MAX {
                        last_row[stream] = run;
                        in_data[stream] = Some(data.clone());
                        if in_data.iter().all(Option::is_some) {
                            self.spawn_next(
                                component,
                                comp_id,
                                InputKind::Multiple(n),
                                Some(last_row.clone()),
                            );
                        }
                    } else if last_row[stream] == 0 {
                        let mut id = id.clone();
                        id.tree.last_mut().unwrap()[stream] = run;
                        new_ids.push(id);
                        let len = new_data.len();
                        new_data.resize(len + num_fields, None);
                        new_data[len + stream] = Some(data.clone());
                    }
                }
                for (data, id) in new_data.chunks(num_fields).zip(new_ids) {
                    let (n, id_ref, data_ref) = lock.alloc(num_fields);
                    let last = id.tree.last().unwrap().clone();
                    *id_ref = Some(id);
                    data_ref.clone_from_slice(data);
                    self.spawn_next(component, comp_id, InputKind::Multiple(n), Some(last));
                }
            } else {
                self.spawn_next(
                    component,
                    comp_id,
                    InputKind::Single(data.clone()),
                    next_comp.multi_input.then_some(smallvec::smallvec![run]),
                );
            }
        }
    }
    fn spawn_next(
        &self,
        component: &'r ComponentData,
        comp_id: ComponentId,
        input: InputKind,
        last_row: Option<SmallVec<[u32; 2]>>,
    ) {
        let cleanup_idx = if let InputKind::Multiple(idx) = input {
            Some(idx)
        } else {
            None
        };
        let runner = self.runner;
        let inner = component.component.clone();
        let decr = self.decr.clone();
        let mut run_id = self.run_id.clone();
        if let Some(row) = last_row {
            run_id.push(row);
        }
        self.scope.spawn(move |scope| {
            let cleanup_id = run_id.clone();
            inner.run(ComponentContext {
                input,
                runner,
                comp_id,
                scope,
                decr,
                run_id,
                invoc: AtomicU32::new(0),
            });
            if let Some(idx) = cleanup_idx {
                component
                    .partial
                    .lock()
                    .unwrap_or_else(|e| e.into_inner())
                    .free(idx, component.in_lookup.len());
            }
            let mut lock = component.partial.lock().unwrap_or_else(|e| e.into_inner());
            let lock = &mut *lock;
            for (n, (data, id)) in lock
                .data
                .chunks_mut(component.in_lookup.len())
                .zip(&mut lock.ids)
                .enumerate()
            {
                if id.as_ref().is_some_and(|i| cleanup_id.is_pred_of(i)) {
                    *id = None;
                    data.fill(None);
                    if n < lock.first {
                        lock.first = n;
                    }
                }
            }
        });
    }
}
