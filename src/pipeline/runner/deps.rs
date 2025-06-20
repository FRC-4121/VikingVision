use super::*;
use crate::pipeline::prelude::Inputs;

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
        attached: bool,
    },
    Multiple {
        lookup: HashMap<String, (usize, ComponentId)>,
        multi: Option<(String, ComponentId)>,
    },
}

pub(super) struct ComponentData {
    /// The actual component
    pub component: Arc<dyn Component>,
    /// Components dependent on a primary channel
    pub primary_dependents: Vec<(ComponentId, InputChannel)>,
    /// Components dependent on a secondary channel
    pub dependents: HashMap<String, Vec<(ComponentId, InputChannel)>>,
    /// Locked partial data
    pub partial: Mutex<MutableData>,
    /// Name of this component
    pub name: triomphe::Arc<str>,
    /// What inputs this component is expecting
    pub input_mode: InputMode,
    /// Where our multiple input came from
    pub multi_input_from: Option<ComponentId>,
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

/// An error that can occur from [`PipelineRunner::add_component`]
#[derive(Debug, Clone, PartialEq, Error)]
#[non_exhaustive]
pub enum AddComponentError {
    /// A component with the name already exits.
    #[error("Name already exists with component ID {}", .0.0)]
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
        tracing::info!(?name, hidden = true, "adding component");
        let value = ComponentId(self.components.len());
        let input_mode = match component.inputs() {
            Inputs::Primary => InputMode::Single {
                name: None,
                attached: false,
            },
            Inputs::Named(mut v) => {
                if v.len() == 1 {
                    InputMode::Single {
                        name: v.pop(),
                        attached: false,
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
        let span = tracing::info_span!("initializing component", %name, component = %value);
        self.components.push(ComponentData {
            component,
            primary_dependents: Vec::new(),
            dependents: HashMap::new(),
            partial: Mutex::new(MutableData {
                data: Vec::new(),
                per_run: Vec::new(),
                first: 0,
            }),
            name,
            input_mode,
            multi_input_from: None,
        });
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
        tracing::info!(?name, hidden = false, "adding component");
        match self.lookup.entry(name.clone()) {
            Entry::Occupied(e) => Err(AddComponentError::AlreadyExists(*e.get())),
            Entry::Vacant(e) => {
                let value = ComponentId(self.components.len());
                let input_mode = match component.inputs() {
                    Inputs::Primary => InputMode::Single {
                        name: None,
                        attached: false,
                    },
                    Inputs::Named(mut v) => {
                        if v.len() == 1 {
                            InputMode::Single {
                                name: v.pop(),
                                attached: false,
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
                let span = tracing::info_span!("initializing component", %name, component = %value);
                self.components.push(ComponentData {
                    component,
                    primary_dependents: Vec::new(),
                    dependents: HashMap::new(),
                    partial: Mutex::new(MutableData {
                        data: Vec::new(),
                        per_run: Vec::new(),
                        first: 0,
                    }),
                    name,
                    input_mode,
                    multi_input_from: None,
                });
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
        if pub_id.0 >= self.components.len() {
            return Err(AddDependencyError::NoPublisher(pub_id));
        }
        if sub_id.0 >= self.components.len() {
            return Err(AddDependencyError::NoSubscriber(pub_id));
        }
        if pub_id == sub_id {
            return Err(AddDependencyError::SelfLoop);
        }
        let [c1, c2] = self
            .components
            .get_disjoint_mut([pub_id.0, sub_id.0])
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
                    if ex_name.as_ref().is_none_or(|n| n != name) {
                        return Err(AddDependencyError::DoesntTakeInput {
                            component: sub_id,
                            channel: Some(name),
                        });
                    }
                    if *attached {
                        return Err(AddDependencyError::DuplicateNamedInput {
                            component: sub_id,
                            channel: name,
                        });
                    }
                    *attached = true;
                    if kind.is_multi() {
                        c2.multi_input_from = Some(pub_id);
                        InputChannel::Primary(true)
                    } else {
                        c2.multi_input_from = c1.multi_input_from;
                        InputChannel::Primary(false)
                    }
                }
                InputMode::Multiple { lookup, multi } => {
                    let Some((idx, comp)) = lookup.get_mut(name) else {
                        return Err(AddDependencyError::DoesntTakeInput {
                            component: sub_id,
                            channel: Some(name),
                        });
                    };
                    if comp.is_valid() {
                        return Err(AddDependencyError::DuplicateNamedInput {
                            component: sub_id,
                            channel: name,
                        });
                    }
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
            if *attached {
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
}
