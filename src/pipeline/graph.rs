use super::{prelude::*, *};
use litemap::LiteMap;
use smallvec::SmallVec;
use smol_str::SmolStr;
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::sync::atomic::{AtomicU32, AtomicUsize};
use std::sync::{Arc, LazyLock, Mutex};
use thiserror::Error;

/// Alias for component IDs used in a [`PipelineGraph`].
pub type GraphComponentId = ComponentId<PipelineGraph>;
/// Alias for component channels used in a [`PipelineGraph`].
pub type GraphComponentChannel = ComponentChannel<PipelineGraph>;

mod trait_impls {
    use super::*;

    impl IntoOptStr for () {
        fn into_opt_str(self) -> Option<SmolStr> {
            None
        }
    }
    impl IntoOptStr for &str {
        fn into_opt_str(self) -> Option<SmolStr> {
            Some(self.into())
        }
    }
    impl IntoOptStr for String {
        fn into_opt_str(self) -> Option<SmolStr> {
            Some(self.into())
        }
    }
    impl IntoOptStr for &String {
        fn into_opt_str(self) -> Option<SmolStr> {
            Some(self.as_str().into())
        }
    }
    impl IntoOptStr for SmolStr {
        fn into_opt_str(self) -> Option<SmolStr> {
            Some(self)
        }
    }
    impl IntoOptStr for &SmolStr {
        fn into_opt_str(self) -> Option<SmolStr> {
            Some(self.clone())
        }
    }
    impl<S: IntoOptStr> IntoOptStr for Option<S> {
        fn into_opt_str(self) -> Option<SmolStr> {
            self.and_then(S::into_opt_str)
        }
    }

    impl ComponentSpecifier<PipelineGraph> for GraphComponentId {
        type Error = InvalidComponentId<PipelineGraph>;

        fn resolve(&self, graph: &PipelineGraph) -> Result<GraphComponentId, Self::Error> {
            if self.is_placeholder() {
                return Err(InvalidComponentId(*self));
            }
            let this = self.unflagged();
            let data = graph
                .components
                .get(this.index())
                .ok_or(InvalidComponentId(this))?;
            if data.is_placeholder() {
                return Err(InvalidComponentId(*self));
            }
            Ok(this)
        }
    }
    impl ComponentSpecifier<PipelineGraph> for str {
        type Error = UnknownComponentName;

        fn resolve(&self, graph: &PipelineGraph) -> Result<GraphComponentId, Self::Error> {
            graph
                .lookup
                .get(self)
                .copied()
                .ok_or_else(|| UnknownComponentName(self.into()))
        }
    }
    impl ComponentSpecifier<PipelineGraph> for String {
        type Error = UnknownComponentName;

        fn resolve(&self, graph: &PipelineGraph) -> Result<GraphComponentId, Self::Error> {
            graph
                .lookup
                .get(self.as_str())
                .copied()
                .ok_or_else(|| UnknownComponentName(self.into()))
        }
    }
    impl ComponentSpecifier<PipelineGraph> for SmolStr {
        type Error = UnknownComponentName;

        fn resolve(&self, graph: &PipelineGraph) -> Result<GraphComponentId, Self::Error> {
            graph
                .lookup
                .get(self)
                .copied()
                .ok_or_else(|| UnknownComponentName(self.clone()))
        }
    }

    impl<C: ComponentSpecifier<PipelineGraph>> ComponentWithChannel for C {
        type Error = <C as ComponentSpecifier<PipelineGraph>>::Error;

        fn resolve(
            self,
            graph: &PipelineGraph,
        ) -> Result<(GraphComponentId, Option<SmolStr>), Self::Error> {
            Ok((ComponentSpecifier::resolve(&self, graph)?, None))
        }
    }
    impl<C: ComponentSpecifier<PipelineGraph>, S: IntoOptStr> ComponentWithChannel for (C, S) {
        type Error = <C as ComponentSpecifier<PipelineGraph>>::Error;

        fn resolve(
            self,
            graph: &PipelineGraph,
        ) -> Result<(GraphComponentId, Option<SmolStr>), Self::Error> {
            Ok((
                ComponentSpecifier::resolve(&self.0, graph)?,
                self.1.into_opt_str(),
            ))
        }
    }
}

/// A type that can be converted to `Option<SmolStr>`.
///
/// This works better than relying on `Into` or similar, and allows direct conversion from `()` (to `None`), string slices, and options.
pub trait IntoOptStr {
    fn into_opt_str(self) -> Option<SmolStr>;
}

pub trait ComponentWithChannel {
    type Error;

    fn resolve(
        self,
        graph: &PipelineGraph,
    ) -> Result<(GraphComponentId, Option<SmolStr>), Self::Error>;
}

#[derive(Debug, Clone, PartialEq, Error)]
#[error("Duplicate component name {name:?} (previously {old})")]
pub struct DuplicateNamedComponent {
    pub old: GraphComponentId,
    pub name: SmolStr,
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum AddDependencyError<E1, E2> {
    #[error("Missing source component: {0}")]
    MissingSource(E1),
    #[error("Missing destination component: {0:?}")]
    MissingDest(E2),
    #[error(transparent)]
    Generic(GenericAddDependecyError),
}
#[derive(Debug, Clone, PartialEq, Error)]
pub enum GenericAddDependecyError {
    #[error("Source and destination components cannot be the same")]
    SelfLoop,
    #[error("Component {src_id} ({src_name:?}) doesn't take an input on channel {src_chan:?}")]
    NoOutputChannel {
        src_id: GraphComponentId,
        src_name: SmolStr,
        src_chan: Option<SmolStr>,
    },
    #[error("Component {dst_id} ({dst_name:?}) can't take input on channel {dst_chan:?}")]
    DoesntTakeInput {
        dst_id: GraphComponentId,
        dst_name: SmolStr,
        dst_chan: Option<SmolStr>,
    },
    #[error(
        "Component {dst_id} ({dst_name:?}) can't take input from multiple sources on channel {dst_chan:?} because {}",
        if let Some(s) = .already_connected { format!("it's already overloaded on channel {s:?}") }
        else { "it can't take multiple inputs".to_string() }
    )]
    OverloadedInputs {
        dst_id: GraphComponentId,
        dst_name: SmolStr,
        dst_chan: Option<SmolStr>,
        already_connected: Option<SmolStr>,
    },
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum CompileError {
    #[error("Graph contains at least one cycle")]
    ContainsCycle(Vec<(GraphComponentId, SmolStr)>),
    #[error("Component {comp} has multiple branching paths that lead to it: {} and {}", ChainFormatter(.branch_1), ChainFormatter(.branch_2))]
    BranchMismatch {
        comp: SmolStr,
        branch_1: Vec<(SmolStr, Option<SmolStr>)>,
        branch_2: Vec<(SmolStr, Option<SmolStr>)>,
    },
    #[error("Component {comp} is missing an input on channel {chan:?}")]
    MissingInput { comp: SmolStr, chan: SmolStr },
}

fn show_pair((name, chan): &(SmolStr, Option<SmolStr>), f: &mut Formatter) -> fmt::Result {
    f.write_str(name)?;
    if let Some(chan) = chan {
        f.write_str("/")?;
        f.write_str(chan)?;
    }
    Ok(())
}

struct ChainFormatter<'a>(&'a Vec<(SmolStr, Option<SmolStr>)>);
impl Display for ChainFormatter<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some((first, rest)) = self.0.split_first() {
            show_pair(first, f)?;
            for elem in rest {
                f.write_str(" -> ")?;
                show_pair(elem, f)?;
            }
        }
        Ok(())
    }
}

static DEFAULT_COMPONENT: LazyLock<Arc<dyn Component>> = LazyLock::new(|| {
    struct Placeholder;
    impl Component for Placeholder {
        fn inputs(&self) -> Inputs {
            Inputs::none()
        }
        fn output_kind(&self, _name: Option<&str>) -> OutputKind {
            OutputKind::None
        }
        fn run<'s, 'r: 's>(&self, _context: ComponentContext<'r, '_, 's>) {
            tracing::error!("called a placeholder component");
        }
    }
    Arc::new(Placeholder)
});
const DEFAULT_NAME: SmolStr = smol_str::SmolStr::new_static("<placeholder>");

#[derive(Debug, Clone)]
struct MultiData {
    chan: SmolStr,
    optional: bool,
    inputs: Vec<GraphComponentChannel>,
}

#[derive(Debug, Clone)]
enum InputKind {
    Single(SmallVec<[GraphComponentChannel; 1]>),
    Multiple {
        single: Vec<(SmolStr, GraphComponentChannel)>,
        multi: Option<MultiData>,
    },
}

#[derive(Debug, Clone, PartialEq)]
pub struct IdResolver(pub Vec<RunnerComponentId>);
impl IdResolver {
    pub fn get(&self, id: GraphComponentId) -> Option<RunnerComponentId> {
        let idx = self.0.get(id.index())?;
        idx.is_valid().then_some(*idx)
    }
}
impl std::ops::Index<GraphComponentId> for IdResolver {
    type Output = RunnerComponentId;

    fn index(&self, index: GraphComponentId) -> &Self::Output {
        let Some(idx) = self.0.get(index.index()) else {
            panic!("component ID {index} wasn't present in the graph that created this resolver");
        };
        if idx.is_placeholder() {
            panic!("component ID {index} wasn't present in the graph that created this resolver");
        }
        idx
    }
}

/// Associated data for a component in the graph.
///
/// More may become public if there's a need for it.
#[derive(Clone)]
pub struct ComponentData {
    pub component: Arc<dyn Component>,
    pub name: SmolStr,
    inputs: InputKind,
    outputs: BTreeMap<Option<SmolStr>, Vec<GraphComponentChannel>>,
    in_lookup: bool,
    input_count: LiteMap<GraphComponentId, usize>,
}
impl ComponentData {
    /// To keep stable indices, removed components are replaced with a placeholder value.
    ///
    /// This is a pointer equality check to see if this component is a placeholder.
    #[inline(always)]
    pub fn is_placeholder(&self) -> bool {
        Arc::ptr_eq(&self.component, &DEFAULT_COMPONENT)
    }
}
impl Debug for ComponentData {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentData")
            .field("name", &self.name)
            .field("inputs", &self.inputs)
            .field("outputs", &self.outputs)
            .field("in_lookup", &self.in_lookup)
            .field("is_placeholder", &self.is_placeholder())
            .finish_non_exhaustive()
    }
}

/// An incomplete graph for a pipeline.
///
/// This is used to build a pipeline. In order to run a pipeline, [`Self::compile`] needs to be called, which builds
/// a [`PipelineRunner`]. The compiled runner can't have its structure changed, so validation of the graph can be handled
/// solely by the [`PipelineGraph`].
#[derive(Debug, Default, Clone)]
pub struct PipelineGraph {
    components: Vec<ComponentData>,
    pub lookup: HashMap<SmolStr, GraphComponentId>,
    first_free: usize,
}
impl PipelineGraph {
    /// Create a new, empty pipeline graph.
    pub fn new() -> Self {
        Self {
            components: Vec::new(),
            lookup: HashMap::new(),
            first_free: 0,
        }
    }
    /// Access the component data as a slice.
    pub fn components(&self) -> &[ComponentData] {
        &self.components
    }
    /// Access the lookup table for the graph.
    pub fn lookup(&self) -> &HashMap<SmolStr, GraphComponentId> {
        &self.lookup
    }
    /// Add a new component to the pipeline with a unique name.
    ///
    /// The component is registered in the lookup table and assigned a unique [`ComponentId`] for referencing.
    /// During registration, the component's [`initialize`](Component::initialize) method is called to perform any necessary setup.
    ///
    /// # Example
    ///
    /// ```rust
    /// # use viking_vision::pipeline::prelude::for_test::{*, ProduceComponent as ImageProcessor, ConsumeComponent as OtherProcessor};
    ///
    /// let mut runner = PipelineRunner::new();
    ///
    /// // Add a component
    /// let processor = runner.add_component(
    ///     "image_processor",
    ///     Arc::new(ImageProcessor::new())
    /// ).unwrap();
    ///
    /// // Adding with same name fails
    /// assert!(runner.add_component("image_processor", Arc::new(OtherProcessor)).is_err());
    /// ```
    #[inline(always)]
    pub fn add_named_component(
        &mut self,
        component: Arc<dyn Component>,
        name: impl Into<SmolStr>,
    ) -> Result<GraphComponentId, DuplicateNamedComponent> {
        let name = name.into();
        let comp = match self.lookup.entry(name.clone()) {
            Entry::Occupied(e) => {
                return Err(DuplicateNamedComponent {
                    old: *e.get(),
                    name,
                });
            }
            Entry::Vacant(e) => {
                let comp = Self::add_component_impl(
                    &mut self.components,
                    &mut self.first_free,
                    component.clone(),
                    name,
                    true,
                );
                e.insert(comp);
                comp
            }
        };
        component.initialize(self, comp);
        Ok(comp)
    }
    /// Add a component without adding it to the lookup table.
    ///
    /// Hidden components can only be referenced by their [`ComponentId`] but still need a name for logging purposes.
    /// They participate in dependencies like normal components, making them useful for internal components that shouldn't
    /// be publicly accessible, dynamically generated components, or components with non-unique names.
    #[inline(always)]
    pub fn add_hidden_component(
        &mut self,
        component: Arc<dyn Component>,
        name: impl Into<SmolStr>,
    ) -> GraphComponentId {
        let comp = Self::add_component_impl(
            &mut self.components,
            &mut self.first_free,
            component.clone(),
            name.into(),
            false,
        );
        component.initialize(self, comp);
        comp
    }
    fn add_component_impl(
        components: &mut Vec<ComponentData>,
        first_free: &mut usize,
        component: Arc<dyn Component>,
        name: SmolStr,
        in_lookup: bool,
    ) -> GraphComponentId {
        let inputs = match component.inputs() {
            Inputs::Primary => InputKind::Single(SmallVec::new()),
            Inputs::Named(i) => InputKind::Multiple {
                single: i
                    .into_iter()
                    .map(|v| (v, GraphComponentChannel::PLACEHOLDER))
                    .collect(),
                multi: None,
            },
        };
        let new_data = ComponentData {
            component,
            name,
            inputs,
            outputs: BTreeMap::new(),
            in_lookup,
            input_count: LiteMap::new(),
        };
        let idx = *first_free;
        let out = GraphComponentId::new(idx);
        if idx == components.len() {
            *first_free += 1;
            components.push(new_data);
        } else {
            components[idx] = new_data;
            *first_free = components[(idx + 1)..]
                .iter()
                .position(|c| !c.is_placeholder())
                .map_or(components.len(), |n| n + idx);
        }
        out
    }

    /// Add a dependency to the graph.
    ///
    /// This doesn't check all of the invariants that are required, only that the requested components
    /// exist and have the necessary input and output channels.
    #[inline(always)]
    pub fn add_dependency<S: ComponentWithChannel, D: ComponentWithChannel>(
        &mut self,
        src: S,
        dst: D,
    ) -> Result<(), AddDependencyError<S::Error, D::Error>> {
        fn inner(
            this: &mut PipelineGraph,
            s_id: GraphComponentId,
            s_chan: Option<SmolStr>,
            d_id: GraphComponentId,
            d_chan: Option<SmolStr>,
        ) -> Result<(), GenericAddDependecyError> {
            let [src, dst] = this
                .components
                .get_disjoint_mut([s_id.index(), d_id.index()])
                .map_err(|_| GenericAddDependecyError::SelfLoop)?;
            let is_multi = match src.component.output_kind(s_chan.as_deref()) {
                OutputKind::None => {
                    return Err(GenericAddDependecyError::NoOutputChannel {
                        src_id: s_id,
                        src_name: src.name.clone(),
                        src_chan: s_chan,
                    });
                }
                OutputKind::Single => false,
                OutputKind::Multiple => true,
            };
            let scc = s_chan.clone();
            let dcc = d_chan.clone();
            match &mut dst.inputs {
                InputKind::Single(v) => {
                    if d_chan.is_some() {
                        return Err(GenericAddDependecyError::DoesntTakeInput {
                            dst_id: d_id,
                            dst_name: dst.name.clone(),
                            dst_chan: d_chan,
                        });
                    }
                    v.push(ComponentChannel(s_id, s_chan))
                }
                InputKind::Multiple { single, multi } => {
                    let Some(dc) = d_chan else {
                        return Err(GenericAddDependecyError::DoesntTakeInput {
                            dst_id: d_id,
                            dst_name: dst.name.clone(),
                            dst_chan: None,
                        });
                    };
                    'search: {
                        if let Some(MultiData { chan, inputs, .. }) = multi {
                            if *chan == dc {
                                inputs.push(ComponentChannel(s_id, s_chan));
                                break 'search;
                            }
                            for (ch, s) in &mut *single {
                                if *ch == dc {
                                    if s.is_placeholder() {
                                        *s = ComponentChannel(s_id, s_chan);
                                        break 'search;
                                    } else {
                                        return Err(GenericAddDependecyError::OverloadedInputs {
                                            dst_id: d_id,
                                            dst_name: dst.name.clone(),
                                            dst_chan: Some(dc),
                                            already_connected: Some(chan.clone()),
                                        });
                                    }
                                }
                            }
                        } else {
                            for (n, (ch, s)) in single.iter_mut().enumerate() {
                                if *ch == dc {
                                    if s.is_placeholder() {
                                        *s = ComponentChannel(s_id, s_chan);
                                        break 'search;
                                    } else {
                                        let (chan, s) = single.swap_remove(n);
                                        let (optional, s) = s.decompose();
                                        *multi = Some(MultiData {
                                            chan,
                                            optional,
                                            inputs: vec![s, ComponentChannel(s_id, s_chan)],
                                        });
                                        break 'search;
                                    }
                                }
                            }
                        }
                        if dst.component.can_take(&dc) {
                            single.push((dc, ComponentChannel(s_id.flagged(), s_chan)));
                        }
                    }
                }
            }
            src.outputs
                .entry(scc)
                .or_default()
                .push(ComponentChannel(d_id.with_flag(is_multi), dcc));
            *dst.input_count.entry(s_id).or_insert(0) += 1;
            Ok(())
        }
        let (s_id, s_chan) = src
            .resolve(self)
            .map_err(AddDependencyError::MissingSource)?;
        let (d_id, d_chan) = dst.resolve(self).map_err(AddDependencyError::MissingDest)?;
        inner(self, s_id, s_chan, d_id, d_chan).map_err(AddDependencyError::Generic)
    }

    /// Detach a component's inputs and outputs.
    pub fn detach_component<C: ComponentSpecifier<Self>>(&mut self, id: C) -> Result<(), C::Error> {
        self.detach_impl(ComponentSpecifier::resolve(&id, self)?);
        Ok(())
    }

    /// Remove a component from the pipeline.
    ///
    /// This component's ID may be reused, but all other IDs will remain valid.
    pub fn remove_component<C: ComponentSpecifier<Self>>(&mut self, id: C) -> Result<(), C::Error> {
        let id = ComponentSpecifier::resolve(&id, self)?;
        self.detach_impl(id);
        let comp = &mut self.components[id.index()];
        if comp.in_lookup {
            self.lookup.remove(&comp.name);
        }
        comp.component = DEFAULT_COMPONENT.clone();
        comp.name = DEFAULT_NAME;
        Ok(())
    }
    fn detach_impl(&mut self, id: GraphComponentId) {
        let comp = &mut self.components[id.index()];
        let inputs = match &mut comp.inputs {
            InputKind::Single(v) => std::mem::take(v),
            InputKind::Multiple { single, multi } => {
                let mut buf = SmallVec::<[_; 1]>::new();
                let mut v = single
                    .extract_if(.., |(_, c)| {
                        if c.is_placeholder() {
                            false
                        } else if c.0.flag() {
                            true
                        } else {
                            buf.push(std::mem::take(c));
                            false
                        }
                    })
                    .map(|x| x.1)
                    .collect::<SmallVec<[_; 1]>>();
                v.append(&mut buf);
                if let Some(MultiData {
                    chan,
                    optional,
                    inputs,
                }) = multi.take()
                {
                    v.extend(inputs);
                    if !optional {
                        single.push((chan, ComponentChannel::PLACEHOLDER));
                    }
                }
                v
            }
        };
        let outputs = std::mem::take(&mut comp.outputs);
        for input in inputs {
            use std::collections::btree_map::Entry;
            match self.components[input.0.index()].outputs.entry(input.1) {
                Entry::Occupied(mut e) => {
                    let v = e.get_mut();
                    v.retain(|c| c.0 != id);
                    if v.is_empty() {
                        e.remove();
                    }
                }
                Entry::Vacant(_) => {}
            }
        }
        for ch in outputs.into_values().flatten() {
            let c2 = &mut self.components[ch.0.index()];
            let mut count = 0;
            match &mut c2.inputs {
                InputKind::Single(v) => {
                    count = v.drain_filter(|c| c.0 == id).count();
                }
                InputKind::Multiple { single, multi } => {
                    let Some(dc) = ch.1 else { continue };
                    if let Some(MultiData { chan, inputs, .. }) = multi {
                        if *chan == dc {
                            count = inputs.extract_if(.., |c| c.0 == id).count();
                            match &mut **inputs {
                                [] => {
                                    let Some(MultiData { chan, optional, .. }) = multi.take()
                                    else {
                                        unreachable!()
                                    };
                                    if !optional {
                                        single.push((chan, ComponentChannel::PLACEHOLDER));
                                    }
                                }
                                [v] => {
                                    let ch = std::mem::take(v);
                                    let Some(MultiData { chan, optional, .. }) = multi.take()
                                    else {
                                        unreachable!()
                                    };
                                    single.push((chan, ch.with_flag(optional)));
                                }
                                _ => {}
                            }
                        }
                    } else {
                        for (n, (_, s)) in single.iter_mut().enumerate() {
                            if s.0 == id {
                                if s.0.flag() {
                                    single.swap_remove(n);
                                } else {
                                    *s = ComponentChannel::PLACEHOLDER;
                                }
                                count = 1;
                                break;
                            }
                        }
                    }
                }
            }
            if let litemap::Entry::Occupied(mut e) = c2.input_count.entry(id) {
                let r = e.get_mut();
                *r -= count;
                if *r == 0 {
                    e.remove();
                }
            }
        }
    }
    /// Remove all components from the graph.
    pub fn clear(&mut self) {
        self.components.clear();
        self.lookup.clear();
        self.first_free = 0;
    }
    /// Get the component with a given ID.
    pub fn component(&self, id: GraphComponentId) -> Option<&Arc<dyn Component>> {
        let c = self.components.get(id.index())?;
        (!c.is_placeholder()).then_some(&c.component)
    }
    /// Compile this graph into a pipeline runner.
    ///
    /// This remaps the component IDs into a topologically-sorted order and verifies additional invariants:
    /// - The graph must be acyclic.
    /// - Any component must have a single chain of "branch points" such that there's no ambiguity in which inputs should be broadcast.
    pub fn compile(mut self) -> Result<(IdResolver, PipelineRunner), CompileError> {
        let _guard = tracing::info_span!("compile").entered();

        tracing::info!(
            "compiling a pipeline graph with {} nodes",
            self.components.len()
        );
        let mut mapping = vec![ComponentId::<PipelineRunner>::PLACEHOLDER; self.components.len()];
        let mut components = Vec::with_capacity(self.components.len());
        let mut auxiliary = Vec::with_capacity(self.components.len());
        let mut extracted = Vec::new();
        let mut iters = 0;
        loop {
            iters += 1;
            extracted.clear();
            for (i, component) in self.components.iter_mut().enumerate() {
                if component.is_placeholder() {
                    continue;
                }
                if component.input_count.is_empty() {
                    extracted.push(i);
                    let rid = components.len();
                    mapping[i] = ComponentId::new(rid);
                    let input = std::mem::replace(
                        &mut component.inputs,
                        InputKind::Single(SmallVec::new()),
                    );
                    components.push(runner::ComponentData {
                        component: std::mem::replace(
                            &mut component.component,
                            DEFAULT_COMPONENT.clone(),
                        ),
                        name: std::mem::replace(&mut component.name, DEFAULT_NAME.clone()),
                        dependents: HashMap::new(),
                        input_mode: match &input {
                            InputKind::Single(_) => runner::InputMode::Single { name: None },
                            InputKind::Multiple {
                                single,
                                multi: Some(MultiData { chan, .. }),
                            } => {
                                if single.is_empty() {
                                    runner::InputMode::Single {
                                        name: Some(chan.clone()),
                                    }
                                } else {
                                    runner::InputMode::Multiple {
                                        lookup: HashMap::new(),
                                        tree_shape: SmallVec::new(),
                                        mutable: Mutex::default(),
                                    }
                                }
                            }
                            InputKind::Multiple { single, .. } => {
                                if let [(name, _)] = &**single {
                                    runner::InputMode::Single {
                                        name: Some(name.clone()),
                                    }
                                } else {
                                    runner::InputMode::Multiple {
                                        lookup: HashMap::new(),
                                        tree_shape: SmallVec::new(),
                                        mutable: Mutex::default(),
                                    }
                                }
                            }
                        },
                    });
                    auxiliary.push((
                        match input {
                            InputKind::Multiple { single, multi } => {
                                (single, multi.map(|m| (m.chan, m.inputs)))
                            }
                            InputKind::Single(v) => (
                                v.into_iter()
                                    .map(|c| (SmolStr::new_static(""), c))
                                    .collect(),
                                None,
                            ),
                        },
                        Vec::<runner::RunnerComponentChannel>::new(),
                        std::mem::take(&mut component.outputs),
                    ));
                }
            }
            for component in &mut self.components {
                component
                    .input_count
                    .retain(|k, _| extracted.binary_search(&k.index()).is_err());
            }
            if extracted.is_empty() {
                break;
            }
        }

        tracing::debug!(iters, "finished topological sort");

        if components.len() < self.components.len() {
            return Err(CompileError::ContainsCycle(
                self.components
                    .into_iter()
                    .enumerate()
                    .filter_map(|(n, c)| {
                        (!c.is_placeholder()).then_some((ComponentId::new(n), c.name))
                    })
                    .collect(),
            ));
        }

        for i in 0..auxiliary.len() {
            let (_, branch, out) =
                unsafe { &mut *std::ptr::from_mut(auxiliary.get_unchecked_mut(i)) }; // we need to detach the lifetime here, we check safety later
            for (chan, deps) in out {
                let flag = deps[0].0.flag();
                if flag {
                    branch.push(ComponentChannel(ComponentId::new(i), chan.clone()));
                }
                for dep in deps {
                    let idx = mapping[dep.0.index()].index();
                    assert_ne!(idx, i, "a self-loop in this graph would cause unsoundness!");
                    let b2 = &mut auxiliary[idx].1;
                    if let Some(first) = branch.iter().zip(&*b2).position(|(a, b)| a != b) {
                        return Err(CompileError::BranchMismatch {
                            comp: components[i].name.clone(),
                            branch_1: branch
                                .drain(first..)
                                .map(|chan| (components[chan.0.index()].name.clone(), chan.1))
                                .collect(),
                            branch_2: b2
                                .drain(first..)
                                .map(|chan| (components[chan.0.index()].name.clone(), chan.1))
                                .collect(),
                        });
                    }
                    if let Some(rem) = branch.get(b2.len()..) {
                        b2.extend_from_slice(rem);
                    }
                }
                if flag {
                    branch.pop();
                }
            }
        }

        for (n, (component, ((single, multi), ..))) in unsafe {
            (*(&mut *components as *mut [runner::ComponentData]))
                .iter_mut()
                .zip(&auxiliary)
                .enumerate()
        } {
            if single.iter().all(|x| x.1.is_placeholder()) {
                if let runner::InputMode::Multiple {
                    lookup, tree_shape, ..
                } = &mut component.input_mode
                {
                    tree_shape.push(single.len() as u32);
                    lookup.extend(
                        single
                            .iter()
                            .enumerate()
                            .map(|(n, i)| (i.0.clone(), runner::InputIndex(0, n as _))),
                    );
                }
                continue;
            }
            for (name, comp) in single {
                if comp.0.is_placeholder() {
                    return Err(CompileError::MissingInput {
                        comp: component.name.clone(),
                        chan: name.clone(),
                    });
                }
                let idx = mapping[comp.0.index()].index();
                let aux = &auxiliary[idx];
                let branches = aux
                    .2
                    .get(&comp.1.clone())
                    .and_then(|v| v.first())
                    .is_some_and(|c| c.0.flag());
                let depth = aux.1.len() + branches as usize;

                let iidx = if let runner::InputMode::Multiple {
                    lookup, tree_shape, ..
                } = &mut component.input_mode
                {
                    if tree_shape.len() <= depth {
                        tree_shape.resize(depth + 1, 0);
                    }
                    let i = &mut tree_shape[depth];
                    *i += 1;
                    let iidx = runner::InputIndex(depth as _, *i - 1);
                    lookup.insert(name.clone(), iidx);
                    iidx
                } else {
                    runner::InputIndex(0, 0)
                };
                components[idx]
                    .dependents
                    .entry(comp.1.clone())
                    .or_default()
                    .push((ComponentId::new(n), iidx));
            }
            if let Some((name, from)) = multi {
                let depth = from
                    .iter()
                    .map(|ch| {
                        let aux = &auxiliary[mapping[ch.0.index()].index()];
                        let branches = aux
                            .2
                            .get(&ch.1.clone())
                            .and_then(|v| v.first())
                            .is_some_and(|c| c.0.flag());
                        aux.1.len() + branches as usize
                    })
                    .fold(0, usize::max);
                let iidx = if let runner::InputMode::Multiple {
                    lookup, tree_shape, ..
                } = &mut component.input_mode
                {
                    if tree_shape.len() <= depth {
                        tree_shape.resize(depth + 1, 0);
                    }
                    let i = &mut tree_shape[depth];
                    *i += 1;
                    let iidx = runner::InputIndex(depth as _, *i - 1);
                    lookup.insert(name.clone(), iidx);
                    iidx
                } else {
                    runner::InputIndex(0, 0)
                };
                for ch in from {
                    components[mapping[ch.0.index()].index()]
                        .dependents
                        .entry(ch.1.clone())
                        .or_default()
                        .push((ComponentId::new(n), iidx));
                }
            }
        }

        // make the tree shapes of multi-input components cumulative
        for comp in &mut components {
            if let runner::InputMode::Multiple { tree_shape, .. } = &mut comp.input_mode {
                let mut last = 0;
                for elem in tree_shape {
                    last += *elem;
                    *elem = last;
                }
            }
        }

        tracing::trace!("auxiliary data: {auxiliary:#?}");

        // to remap the lookup, we can just reinterpret the map and then lookup the new values
        let mut lookup = unsafe {
            std::mem::transmute::<
                HashMap<SmolStr, ComponentId<PipelineGraph>>,
                HashMap<SmolStr, ComponentId<PipelineRunner>>,
            >(self.lookup)
        };
        for v in lookup.values_mut() {
            *v = mapping[v.index()];
        }
        Ok((
            IdResolver(mapping),
            PipelineRunner {
                components,
                lookup,
                running: AtomicUsize::new(0),
                run_id: AtomicU32::new(0),
            },
        ))
    }
}
