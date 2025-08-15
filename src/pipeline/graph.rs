use super::{prelude::*, *};
use smallvec::SmallVec;
use smol_str::SmolStr;
use std::collections::hash_map::Entry;
use std::collections::{BTreeMap, HashMap};
use std::sync::{Arc, LazyLock};
use thiserror::Error;

/// Alias for component IDs used in a [`PipelineGraph`].
pub type GraphComponentId = ComponentId<PipelineGraph>;
type GraphComponentChannel = ComponentChannel<PipelineGraph>;

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
pub enum CompileError {}

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

#[derive(Debug)]
enum MultiKind {
    Cant,
    None,
    Some {
        chan: SmolStr,
        optional: bool,
        inputs: Vec<GraphComponentChannel>,
    },
}

#[derive(Debug)]
enum InputKind {
    Single(GraphComponentChannel),
    SingleMulti(SmallVec<[GraphComponentChannel; 1]>),
    Multiple {
        single: Vec<(SmolStr, GraphComponentChannel)>,
        multi: MultiKind,
    },
}
impl InputKind {
    fn can_output(&self) -> bool {
        matches!(
            self,
            Self::Single(_)
                | Self::Multiple {
                    multi: MultiKind::Cant,
                    ..
                }
        )
    }
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
pub struct ComponentData {
    pub component: Arc<dyn Component>,
    pub name: SmolStr,
    inputs: InputKind,
    outputs: BTreeMap<Option<SmolStr>, Vec<GraphComponentChannel>>,
    in_lookup: bool,
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
#[derive(Debug, Default)]
pub struct PipelineGraph {
    components: Vec<ComponentData>,
    lookup: HashMap<SmolStr, GraphComponentId>,
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
            Inputs::Primary => {
                if component.is_consumer() {
                    InputKind::SingleMulti(SmallVec::new())
                } else {
                    InputKind::Single(GraphComponentChannel::PLACEHOLDER)
                }
            }
            Inputs::Named(i) => InputKind::Multiple {
                single: i
                    .into_iter()
                    .map(|v| (v, GraphComponentChannel::PLACEHOLDER))
                    .collect(),
                multi: if component.is_consumer() {
                    MultiKind::None
                } else {
                    MultiKind::Cant
                },
            },
        };
        let new_data = ComponentData {
            component,
            name,
            inputs,
            outputs: BTreeMap::new(),
            in_lookup,
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
            if !src.inputs.can_output() {
                return Err(GenericAddDependecyError::NoOutputChannel {
                    src_id: s_id,
                    src_name: src.name.clone(),
                    src_chan: s_chan,
                });
            }
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
                    if v.is_valid() {
                        return Err(GenericAddDependecyError::OverloadedInputs {
                            dst_id: d_id,
                            dst_name: dst.name.clone(),
                            dst_chan: d_chan,
                            already_connected: None,
                        });
                    } else {
                        *v = ComponentChannel(s_id, s_chan);
                    }
                }
                InputKind::SingleMulti(v) => {
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
                        match multi {
                            MultiKind::Cant => {
                                for (ch, s) in &mut *single {
                                    if *ch == dc {
                                        if s.is_placeholder() {
                                            *s = ComponentChannel(s_id, s_chan);
                                            break 'search;
                                        } else {
                                            return Err(
                                                GenericAddDependecyError::OverloadedInputs {
                                                    dst_id: d_id,
                                                    dst_name: dst.name.clone(),
                                                    dst_chan: Some(dc),
                                                    already_connected: None,
                                                },
                                            );
                                        }
                                    }
                                }
                            }
                            MultiKind::None => {
                                for (n, (ch, s)) in single.iter_mut().enumerate() {
                                    if *ch == dc {
                                        if s.is_placeholder() {
                                            *s = ComponentChannel(s_id, s_chan);
                                            break 'search;
                                        } else {
                                            let (chan, s) = single.swap_remove(n);
                                            let (optional, s) = s.decompose();
                                            *multi = MultiKind::Some {
                                                chan,
                                                optional,
                                                inputs: vec![s, ComponentChannel(s_id, s_chan)],
                                            };
                                            break 'search;
                                        }
                                    }
                                }
                            }
                            MultiKind::Some { chan, inputs, .. } => {
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
                                            return Err(
                                                GenericAddDependecyError::OverloadedInputs {
                                                    dst_id: d_id,
                                                    dst_name: dst.name.clone(),
                                                    dst_chan: Some(dc),
                                                    already_connected: Some(chan.clone()),
                                                },
                                            );
                                        }
                                    }
                                }
                            }
                        }
                        if dst.component.can_take(&dc) {
                            single.push((dc, ComponentChannel(s_id, s_chan)));
                        }
                    }
                }
            }
            src.outputs
                .entry(scc)
                .or_default()
                .push(ComponentChannel(d_id.with_flag(is_multi), dcc));
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
            InputKind::Single(i) => {
                if i.is_valid() {
                    smallvec::smallvec![std::mem::take(i)]
                } else {
                    SmallVec::new()
                }
            }
            InputKind::SingleMulti(v) => std::mem::take(v),
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
                if let MultiKind::Some {
                    chan,
                    optional,
                    inputs,
                } = std::mem::replace(multi, MultiKind::None)
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
            match &mut self.components[ch.0.index()].inputs {
                InputKind::Single(v) => *v = ComponentChannel::PLACEHOLDER,
                InputKind::SingleMulti(v) => v.retain(|c| c.0 != id),
                InputKind::Multiple { single, multi } => {
                    let Some(dc) = ch.1 else { continue };
                    if let MultiKind::Some { chan, inputs, .. } = multi {
                        if *chan == dc {
                            inputs.retain(|c| c.0 != id);
                            match &mut **inputs {
                                [] => {
                                    let MultiKind::Some { chan, optional, .. } =
                                        std::mem::replace(multi, MultiKind::None)
                                    else {
                                        unreachable!()
                                    };
                                    if !optional {
                                        single.push((chan, ComponentChannel::PLACEHOLDER));
                                    }
                                }
                                [v] => {
                                    let ch = std::mem::take(v);
                                    let MultiKind::Some { chan, optional, .. } =
                                        std::mem::replace(multi, MultiKind::None)
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
                                break;
                            }
                        }
                    }
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
    pub fn compile(
        &self,
        include_lookup: bool,
    ) -> Result<(IdResolver, PipelineRunner), CompileError> {
        todo!()
    }
}
