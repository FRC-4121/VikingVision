use super::*;
use crate::pipeline::prelude::{Inputs, OutputKind};
use std::sync::LazyLock;

#[derive(Debug, Clone, Copy, PartialEq)]
pub(super) enum InputChannel {
    Primary(bool),
    Multiple,
    Numbered(usize),
}

#[derive(Debug, Default)]
pub(super) struct PerRunData {
    pub id: Option<RunId>,
    pub multi: Vec<Arc<dyn Data>>,
    pub invoc: u32,
    pub refs: u32,
}
impl PerRunData {
    pub fn is_empty(&self) -> bool {
        self.id.is_none()
    }
    pub fn clear(&mut self) {
        self.id = None;
        self.multi.clear();
        self.invoc = 0;
        self.refs = 0;
    }
}

#[derive(Debug)]
pub(super) struct MutableData {
    /// A vector of partial data. This should be chunked by the number of inputs
    pub data: Vec<Option<Arc<dyn Data>>>,
    /// Additional info for each chunk: both the run ID and
    pub per_run: Vec<PerRunData>,
    /// First open index
    pub first: usize,
}
impl MutableData {
    #[allow(clippy::type_complexity)]
    pub fn alloc(&mut self, len: usize) -> (usize, &mut PerRunData, &mut [Option<Arc<dyn Data>>]) {
        let idx = self.first;
        if self.first == self.per_run.len() {
            self.first += 1;
            self.per_run.push(PerRunData::default());
            self.data.resize(self.data.len() + len, None);
        } else {
            self.first = self.per_run[idx..]
                .iter()
                .position(PerRunData::is_empty)
                .map_or(self.per_run.len(), |i| i + idx);
        }
        (
            idx,
            &mut self.per_run[idx],
            &mut self.data[(idx * len)..((idx + 1) * len)],
        )
    }
    pub fn free(&mut self, idx: usize, len: usize) {
        self.per_run[idx].clear();
        self.data[(idx * len)..((idx + 1) * len)].fill(None);
        if idx < self.first {
            self.first = idx
        }
    }
}

#[derive(Debug)]
pub(super) enum InputMode {
    Single {
        name: Option<String>,
        attached: ComponentId,
    },
    Multiple {
        lookup: HashMap<String, (usize, ComponentId)>,
        multi: Option<(String, ComponentId)>,
    },
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
static DEFAULT_NAME: LazyLock<triomphe::Arc<str>> =
    LazyLock::new(|| triomphe::Arc::from("<placeholder>"));

/// Data associated with components.
pub struct ComponentData {
    /// The actual component
    pub component: Arc<dyn Component>,
    /// Components dependent on a primary channel
    pub(super) primary_dependents: Vec<(ComponentId, InputChannel)>,
    /// Components dependent on a secondary channel
    pub(super) dependents: HashMap<String, Vec<(ComponentId, InputChannel)>>,
    /// Locked partial data
    pub(super) partial: Mutex<MutableData>,
    /// Name of this component
    pub name: triomphe::Arc<str>,
    /// What inputs this component is expecting
    pub(super) input_mode: InputMode,
    /// Where our multiple input came from
    pub(super) multi_input_from: Option<ComponentId>,
}
impl Debug for ComponentData {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentData")
            .field("primary_dependents", &self.primary_dependents)
            .field("dependents", &self.dependents)
            .field("partial", &self.partial)
            .field("name", &self.name)
            .field("multi_input_from", &self.multi_input_from)
            .field("input_mode", &self.input_mode)
            .finish_non_exhaustive()
    }
}
impl ComponentData {
    /// Whether this component data is a placeholder.
    ///
    /// When a component is removed, it will be replaced with a placeholder value and can be overwritten.
    #[inline(always)]
    pub fn is_placeholder(&self) -> bool {
        Arc::ptr_eq(&self.component, &DEFAULT_COMPONENT)
    }

    fn placeholder() -> Self {
        Self {
            component: DEFAULT_COMPONENT.clone(),
            primary_dependents: Vec::new(),
            dependents: HashMap::new(),
            partial: Mutex::new(MutableData {
                data: Vec::new(),
                per_run: Vec::new(),
                first: 0,
            }),
            name: DEFAULT_NAME.clone(),
            input_mode: InputMode::Single {
                name: None,
                attached: ComponentId::PLACEHOLDER,
            },
            multi_input_from: None,
        }
    }
}

/// An error that can occur from [`PipelineRunner::add_component`]
#[derive(Debug, Clone, PartialEq, Error)]
#[non_exhaustive]
pub enum AddComponentError {
    /// A component with the name already exits.
    #[error("Name already exists with component ID {0}")]
    AlreadyExists(ComponentId),
}

/// An error that can occur from [`PipelineRunner::add_dependency`]
#[derive(Debug, Clone, PartialEq, Error)]
#[non_exhaustive]
pub enum AddDependencyError<'a> {
    /// The publishing component's ID was out of range.
    #[error("Publishing component {0} doesn't exist")]
    NoPublisher(ComponentId),
    /// The subscribing component's ID was out of range.
    #[error("Subscribing component {0} doesn't exist")]
    NoSubscriber(ComponentId),
    /// The publishing and subscribing component were the same.
    #[error("Can't create a self-loop")]
    SelfLoop,
    /// The publishing component doesn't output on the requested channel.
    #[error("Publishing component {component} doesn't have a {}", if let Some(name) = .channel { format!("named output {name:?}") } else { "primary output".to_string() })]
    NoPubChannel {
        component: ComponentId,
        channel: Option<&'a str>,
    },
    /// A dependency was already created for this named input.
    #[error("Input {channel:?} has already been attached to subscribing component {component}")]
    DuplicateNamedInput {
        component: ComponentId,
        channel: &'a str,
    },
    /// A dependency was already created for the primary input.
    #[error("Primary input has already been attached to subscribing component {component}")]
    DuplicatePrimaryInput { component: ComponentId },
    /// The subscribing component doesn't take the requested input.
    #[error("Subscribing component {component} doesn't take input on a {}", if let Some(name) = .channel { format!("named input {name:?}") } else { "primary input".to_string() })]
    DoesntTakeInput {
        component: ComponentId,
        channel: Option<&'a str>,
    },
    /// A component will get multiple inputs that give multiple values.
    #[error(
        "Component {component} will have multiple inputs that give multiple values (already from {old_multi_pub}, now from {new_multi_pub})"
    )]
    MultipleMultiInputs {
        component: ComponentId,
        old_multi_pub: ComponentId,
        new_multi_pub: ComponentId,
    },
}

impl PipelineRunner {
    /// Add a component without adding it to the lookup table.
    ///
    /// Hidden components can only be referenced by their [`ComponentId`] but still need a name for logging purposes.
    /// They participate in dependencies like normal components, making them useful for internal components that shouldn't
    /// be publicly accessible, dynamically generated components, or components with non-unique names.
    #[inline(always)]
    pub fn add_hidden_component(
        &mut self,
        name: impl Into<triomphe::Arc<str>>,
        component: Arc<dyn Component>,
    ) -> ComponentId {
        self.add_hidden_component_impl(name.into(), component)
    }
    fn add_hidden_component_impl(
        &mut self,
        name: triomphe::Arc<str>,
        component: Arc<dyn Component>,
    ) -> ComponentId {
        tracing::info!(name = &*name, hidden = true, "adding component");
        let input_mode = match component.inputs() {
            Inputs::Primary => InputMode::Single {
                name: None,
                attached: ComponentId::PLACEHOLDER,
            },
            Inputs::Named(mut v) => {
                if v.len() == 1 {
                    InputMode::Single {
                        name: v.pop(),
                        attached: ComponentId::PLACEHOLDER,
                    }
                } else {
                    InputMode::Multiple {
                        lookup: v
                            .into_iter()
                            .enumerate()
                            .map(|(v, k)| (k, (v, ComponentId::PLACEHOLDER)))
                            .collect(),
                        multi: None,
                    }
                }
            }
        };
        let component_clone = component.clone();
        let (data, value) = Self::push_data(&mut self.components, &mut self.first_open);
        let span = tracing::info_span!("initializing component", %name, component = %value);
        data.component = component;
        data.name = name;
        data.input_mode = input_mode;
        span.in_scope(|| component_clone.initialize(self, value));
        value
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
    pub fn add_component(
        &mut self,
        name: impl Into<triomphe::Arc<str>>,
        component: Arc<dyn Component>,
    ) -> Result<ComponentId, AddComponentError> {
        self.add_component_impl(name.into(), component)
    }
    fn add_component_impl(
        &mut self,
        name: triomphe::Arc<str>,
        component: Arc<dyn Component>,
    ) -> Result<ComponentId, AddComponentError> {
        tracing::info!(name = &*name, hidden = false, "adding component");
        match self.lookup.entry(name.clone()) {
            Entry::Occupied(e) => Err(AddComponentError::AlreadyExists(*e.get())),
            Entry::Vacant(e) => {
                let input_mode = match component.inputs() {
                    Inputs::Primary => InputMode::Single {
                        name: None,
                        attached: ComponentId::PLACEHOLDER,
                    },
                    Inputs::Named(mut v) => {
                        if v.len() == 1 {
                            InputMode::Single {
                                name: v.pop(),
                                attached: ComponentId::PLACEHOLDER,
                            }
                        } else {
                            InputMode::Multiple {
                                lookup: v
                                    .into_iter()
                                    .enumerate()
                                    .map(|(v, k)| (k, (v, ComponentId::PLACEHOLDER)))
                                    .collect(),
                                multi: None,
                            }
                        }
                    }
                };
                let component_clone = component.clone();
                let (data, value) = Self::push_data(&mut self.components, &mut self.first_open);
                let span = tracing::info_span!("initializing component", %name, component = %value);
                data.component = component;
                data.name = name;
                data.input_mode = input_mode;
                e.insert(value);
                span.in_scope(|| component_clone.initialize(self, value));
                Ok(value)
            }
        }
    }
    /// Add a dependency between two components.
    ///
    /// Each input can only have one component, and only one input can give multiple values.
    pub fn add_dependency<'a>(
        &mut self,
        pub_id: ComponentId,
        pub_channel: Option<&'a str>,
        sub_id: ComponentId,
        sub_channel: Option<&'a str>,
    ) -> Result<(), AddDependencyError<'a>> {
        pub_id.assert_normal();
        sub_id.assert_normal();
        let pub_id = pub_id.drop_flag();
        let sub_id = sub_id.drop_flag();
        tracing::info!(
            "subscribing {sub_id} ({} output) to {pub_id} ({} input)",
            if let Some(name) = pub_channel {
                format!("{name:?}")
            } else {
                "primary".to_string()
            },
            if let Some(name) = sub_channel {
                format!("{name:?}")
            } else {
                "primary".to_string()
            },
        );
        if pub_id.index() >= self.components.len() {
            return Err(AddDependencyError::NoPublisher(pub_id));
        }
        if sub_id.index() >= self.components.len() {
            return Err(AddDependencyError::NoSubscriber(pub_id));
        }
        if pub_id == sub_id {
            return Err(AddDependencyError::SelfLoop);
        }
        let [c1, c2] = self
            .components
            .get_disjoint_mut([pub_id.index(), sub_id.index()])
            .unwrap();
        let kind = c1.component.output_kind(pub_channel);
        if kind.is_none() {
            return Err(AddDependencyError::NoPubChannel {
                component: pub_id,
                channel: pub_channel,
            });
        }
        #[allow(clippy::collapsible_else_if)]
        if let Some(name) = sub_channel {
            let idx = match &mut c2.input_mode {
                InputMode::Single {
                    name: ex_name,
                    attached,
                } => {
                    if let Some(ex) = ex_name {
                        if ex == name {
                            if attached.is_valid() {
                                return Err(AddDependencyError::DuplicateNamedInput {
                                    component: sub_id,
                                    channel: name,
                                });
                            }
                            *attached = pub_id;
                            if kind.is_multi() {
                                c2.multi_input_from = Some(pub_id);
                                InputChannel::Primary(true)
                            } else {
                                c2.multi_input_from = c1.multi_input_from;
                                InputChannel::Primary(false)
                            }
                        } else {
                            c2.input_mode = InputMode::Multiple {
                                lookup: [
                                    (std::mem::take(ex), (0, ComponentId::PLACEHOLDER)),
                                    (name.to_string(), (1, pub_id)),
                                ]
                                .into(),
                                multi: kind.is_multi().then(|| (name.to_string(), pub_id)),
                            };
                            InputChannel::Numbered(1)
                        }
                    } else {
                        return Err(AddDependencyError::DoesntTakeInput {
                            component: sub_id,
                            channel: Some(name),
                        });
                    }
                }
                InputMode::Multiple { lookup, multi } => {
                    let (idx, comp) = match lookup.get_mut(name) {
                        Some(v) => {
                            if v.1.is_valid() {
                                return Err(AddDependencyError::DuplicateNamedInput {
                                    component: sub_id,
                                    channel: name,
                                });
                            }
                            v
                        }
                        None => {
                            if c2.component.can_take(name) {
                                let idx = lookup.len();
                                lookup
                                    .entry(name.into())
                                    .insert_entry((idx, pub_id.with_flag()))
                                    .into_mut()
                            } else {
                                return Err(AddDependencyError::DoesntTakeInput {
                                    component: sub_id,
                                    channel: Some(name),
                                });
                            }
                        }
                    };
                    if kind.is_multi() {
                        if let Some((_, id)) = multi {
                            return Err(AddDependencyError::MultipleMultiInputs {
                                component: sub_id,
                                old_multi_pub: *id,
                                new_multi_pub: pub_id,
                            });
                        } else {
                            if let Some(from) = c2.multi_input_from {
                                if !(from == pub_id || Some(from) == c1.multi_input_from) {
                                    return Err(AddDependencyError::MultipleMultiInputs {
                                        component: sub_id,
                                        old_multi_pub: from,
                                        new_multi_pub: pub_id,
                                    });
                                }
                            }
                            c2.multi_input_from = Some(pub_id);
                        }
                        let idx = *idx;
                        lookup.retain(|k, (v, _)| {
                            (k != name) && {
                                if *v > idx {
                                    *v -= 1;
                                }
                                true
                            }
                        });
                        *multi = Some((name.to_string(), pub_id));
                        InputChannel::Multiple
                    } else {
                        *comp = pub_id;
                        InputChannel::Numbered(*idx)
                    }
                }
            };
            if let Some(name) = pub_channel {
                c1.dependents
                    .entry(name.to_string())
                    .or_default()
                    .push((sub_id, idx));
            } else {
                c1.primary_dependents.push((sub_id, idx))
            }
        } else {
            let InputMode::Single {
                name: None,
                attached,
            } = &mut c2.input_mode
            else {
                return Err(AddDependencyError::DoesntTakeInput {
                    component: sub_id,
                    channel: None,
                });
            };
            if attached.is_valid() {
                return Err(AddDependencyError::DuplicatePrimaryInput { component: sub_id });
            }
            if let Some(name) = pub_channel {
                c1.dependents
                    .entry(name.to_string())
                    .or_default()
                    .push((sub_id, InputChannel::Primary(kind.is_multi())));
            } else {
                c1.primary_dependents
                    .push((sub_id, InputChannel::Primary(kind.is_multi())))
            }
        }
        Ok(())
    }

    fn push_data<'a>(
        components: &'a mut Vec<ComponentData>,
        first_open: &mut usize,
    ) -> (&'a mut ComponentData, ComponentId) {
        let len = components.len();
        if *first_open == len {
            *first_open += 1;
            components.push(ComponentData::placeholder());
            (components.last_mut().unwrap(), ComponentId::new(len))
        } else {
            let idx = *first_open;
            *first_open = components[idx..]
                .iter()
                .position(ComponentData::is_placeholder)
                .map_or(components.len(), |i| i + idx);
            let data = &mut components[idx];
            (data, ComponentId::new(idx))
        }
    }

    /// Remove a component from the pipeline.
    ///
    /// Any [`ComponentId`]s pointing to this component will be invalidated.
    pub fn remove_component(&mut self, id: ComponentId) -> Option<Arc<dyn Component>> {
        tracing::info!(%id, "removing component");
        let data = self.components.get_mut(id.index())?;
        if data.is_placeholder() {
            return None;
        }
        data.name = DEFAULT_NAME.clone();
        data.multi_input_from = None;
        let component = std::mem::replace(&mut data.component, DEFAULT_COMPONENT.clone());
        let input = std::mem::replace(
            &mut data.input_mode,
            InputMode::Single {
                name: None,
                attached: ComponentId::PLACEHOLDER,
            },
        );
        let refs = data
            .dependents
            .drain()
            .flat_map(|(_, v)| v)
            .chain(data.primary_dependents.drain(..))
            .collect::<Vec<_>>();
        for (component, channel) in refs {
            let data = &mut self.components[component.index()];
            match (channel, &mut data.input_mode) {
                (InputChannel::Primary(_), InputMode::Single { attached, .. }) => {
                    *attached = ComponentId::PLACEHOLDER
                }
                (InputChannel::Numbered(idx), InputMode::Multiple { lookup, .. }) => {
                    let (_, &mut (idx, ref mut id)) =
                        lookup.iter_mut().find(|x| x.1.0 == idx).unwrap();
                    if id.flag() {
                        lookup.retain(|_, (i, _)| {
                            use std::cmp::Ordering;
                            match idx.cmp(i) {
                                Ordering::Less => {
                                    *i -= 1;
                                    true
                                }
                                Ordering::Greater => true,
                                Ordering::Equal => false,
                            }
                        })
                    } else {
                        *id = ComponentId::PLACEHOLDER;
                    }
                }
                (InputChannel::Multiple, InputMode::Multiple { lookup, multi }) => {
                    let (channel, _) = multi.take().unwrap();
                    let idx = lookup.len();
                    lookup.insert(channel, (idx, ComponentId::PLACEHOLDER));
                }
                _ => unreachable!(),
            }
        }
        match input {
            InputMode::Multiple { lookup, multi } => {
                for id2 in lookup
                    .into_iter()
                    .map(|(_, (_, x))| x)
                    .chain(multi.map(|m| m.1))
                {
                    let data = &mut self.components[id2.index()];
                    for vec in data
                        .dependents
                        .values_mut()
                        .chain(std::iter::once(&mut data.primary_dependents))
                    {
                        vec.retain(|(i, _)| *i != id);
                    }
                }
            }
            InputMode::Single { attached, .. } => {
                let data = &mut self.components[attached.index()];
                for vec in data
                    .dependents
                    .values_mut()
                    .chain(std::iter::once(&mut data.primary_dependents))
                {
                    vec.retain(|(i, _)| *i != id);
                }
            }
        }
        Some(component)
    }
    /// Disconnect a component without removing it.
    pub fn disconnect_component(&mut self, id: ComponentId) -> bool {
        tracing::info!(%id, "removing component");
        let Some(data) = self.components.get_mut(id.index()) else {
            return false;
        };
        if data.is_placeholder() {
            return false;
        }
        let refs = data
            .dependents
            .drain()
            .flat_map(|(_, v)| v)
            .chain(data.primary_dependents.drain(..))
            .collect::<Vec<_>>();
        for (component, channel) in refs {
            let data = &mut self.components[component.index()];
            match (channel, &mut data.input_mode) {
                (InputChannel::Primary(_), InputMode::Single { attached, .. }) => {
                    *attached = ComponentId::PLACEHOLDER
                }
                (InputChannel::Numbered(idx), InputMode::Multiple { lookup, .. }) => {
                    let (_, &mut (idx, ref mut id)) =
                        lookup.iter_mut().find(|x| x.1.0 == idx).unwrap();
                    if id.flag() {
                        lookup.retain(|_, (i, _)| {
                            use std::cmp::Ordering;
                            match idx.cmp(i) {
                                Ordering::Less => {
                                    *i -= 1;
                                    true
                                }
                                Ordering::Greater => true,
                                Ordering::Equal => false,
                            }
                        })
                    } else {
                        *id = ComponentId::PLACEHOLDER;
                    }
                }
                (InputChannel::Multiple, InputMode::Multiple { lookup, multi }) => {
                    let (channel, _) = multi.take().unwrap();
                    let idx = lookup.len();
                    lookup.insert(channel, (idx, ComponentId::PLACEHOLDER));
                }
                _ => unreachable!(),
            }
        }
        match &mut self.components[id.index()].input_mode {
            InputMode::Multiple { lookup, multi } => {
                if let Some((channel, component)) = multi.take() {
                    let idx = lookup.len();
                    lookup.insert(channel, (idx, component));
                }
                let ids = lookup
                    .iter_mut()
                    .map(|(_, (_, i))| std::mem::replace(i, ComponentId::PLACEHOLDER))
                    .collect::<Vec<_>>();
                for id2 in ids {
                    let data = &mut self.components[id2.index()];
                    for vec in data
                        .dependents
                        .values_mut()
                        .chain(std::iter::once(&mut data.primary_dependents))
                    {
                        vec.retain(|(i, _)| *i != id);
                    }
                }
            }
            InputMode::Single { attached, .. } => {
                let idx = std::mem::replace(attached, ComponentId::PLACEHOLDER).index();
                let data = &mut self.components[idx];
                for vec in data
                    .dependents
                    .values_mut()
                    .chain(std::iter::once(&mut data.primary_dependents))
                {
                    vec.retain(|(i, _)| *i != id);
                }
            }
        }
        false
    }
}
