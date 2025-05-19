use super::component::{Component, Data};
use smallvec::SmallVec;
use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::sync::atomic::{AtomicU32, AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};

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
    /// Components dependent on a primary channel
    primary_dependents: Vec<(ComponentId, Option<usize>)>,
    /// Components dependent on a secondary channel
    dependents: HashMap<String, Vec<(ComponentId, Option<usize>)>>,
    /// Lookups from the input names to the partial indices
    in_lookup: HashMap<String, usize>,
    /// Locked partial data
    partial: Mutex<PartialData>,
    /// This is true if we need to add a row to our run-id.
    multi_input: bool,
}

struct DecrementRunCount<'a>(&'a PipelineRunner);
impl Drop for DecrementRunCount<'_> {
    fn drop(&mut self) {
        self.0.running.fetch_sub(1, Ordering::AcqRel);
    }
}

pub struct PipelineRunner {
    components: Vec<ComponentData>,
    lookup: HashMap<String, ComponentId>,
    running: AtomicUsize,
    run: AtomicU32,
}
impl PipelineRunner {
    /// Get a map from the registered component names to their IDs.
    pub fn components(&self) -> &HashMap<String, ComponentId> {
        &self.lookup
    }
    /// Get the number of running pipelines.
    pub fn running(&self) -> usize {
        self.running.load(Ordering::Relaxed)
    }
    pub fn run<'s, 'a: 's>(
        &'a self,
        id: ComponentId,
        data: Arc<dyn Data>,
        scope: &rayon::Scope<'s>,
    ) {
        let input = ComponentInput {
            runner: self,
            comp_id: id,
            input: InputKind::Single(data),
        };
        self.running.fetch_add(1, Ordering::AcqRel);
        let decr = Arc::new(DecrementRunCount(self));
        let run_id = RunId {
            invoc: self.run.fetch_add(1, Ordering::Relaxed),
            tree: Vec::new(),
        };
        scope.spawn(move |scope| {
            self.components[id.0].component.run(
                input,
                ComponentOutput {
                    runner: self,
                    comp_id: id,
                    scope,
                    decr,
                    run_id,
                    invoc: AtomicU32::new(0),
                },
            );
        });
    }
}

enum InputKind {
    Single(Arc<dyn Data>),
    Multiple(usize),
}

/// Input to a component
pub struct ComponentInput<'a> {
    runner: &'a PipelineRunner,
    comp_id: ComponentId,
    input: InputKind,
}
impl Debug for ComponentInput<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentInput")
            .field("runner", &(&self.runner as *const _))
            .field("comp_id", &self.comp_id)
            .finish_non_exhaustive()
    }
}
impl ComponentInput<'_> {
    /// Get the current result from a given channel.
    pub fn get<'a>(&self, channel: impl Into<Option<&'a str>>) -> Option<Arc<dyn Data>> {
        match (channel.into(), &self.input) {
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
}

/// Output for a component
pub struct ComponentOutput<'r, 'a, 's> {
    runner: &'r PipelineRunner,
    comp_id: ComponentId,
    scope: &'a rayon::Scope<'s>,
    decr: Arc<DecrementRunCount<'r>>,
    run_id: RunId,
    invoc: AtomicU32,
}
impl Debug for ComponentOutput<'_, '_, '_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentOutput")
            .field("runner", &(&self.runner as *const _))
            .field("comp_id", &self.comp_id)
            .field("run_id", &self.run_id)
            .field("invoc", &self.invoc)
            .finish_non_exhaustive()
    }
}
impl<'s, 'r: 's> ComponentOutput<'r, '_, 's> {
    /// Publish a result on a given channel.
    #[inline(always)]
    pub fn submit<'b>(&self, channel: impl Into<Option<&'b str>>, data: Arc<dyn Data>) {
        self.submit_impl(channel.into(), data);
    }
    fn submit_impl(&self, channel: Option<&str>, data: Arc<dyn Data>) {
        let component = &self.runner.components[self.comp_id.0];
        let dependents = channel.map_or_else(
            || component.primary_dependents.as_slice(),
            |name| component.dependents.get(name).map_or(&[], Vec::as_slice),
        );
        let run = self.invoc.fetch_add(1, Ordering::Relaxed);
        for &(comp_id, channel) in dependents {
            let next_comp = &self.runner.components[comp_id.0];
            if let Some(channel) = channel {
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
                            in_data[channel] = Some(data.clone());
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
                    if last_row[channel] == u32::MAX {
                        last_row[channel] = run;
                        in_data[channel] = Some(data.clone());
                        if in_data.iter().all(Option::is_some) {
                            self.spawn_next(
                                component,
                                comp_id,
                                InputKind::Multiple(n),
                                Some(last_row.clone()),
                            );
                        }
                    } else if last_row[channel] == 0 {
                        let mut id = id.clone();
                        id.tree.last_mut().unwrap()[channel] = run;
                        new_ids.push(id);
                        let len = new_data.len();
                        new_data.resize(len + num_fields, None);
                        new_data[len + channel] = Some(data.clone());
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
        let input = ComponentInput {
            runner: self.runner,
            comp_id,
            input,
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
            inner.run(
                input,
                ComponentOutput {
                    runner,
                    comp_id,
                    scope,
                    decr,
                    run_id,
                    invoc: AtomicU32::new(0),
                },
            );
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
