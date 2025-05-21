use super::*;
use crate::pipeline::prelude::Inputs;

const UNBOUND_NAMED_INPUT_MASK: usize = 1 << (usize::BITS - 1);

#[derive(Debug)]
pub(super) struct MutableData {
    /// A vector of partial data. This should be chunked by the number of inputs
    pub data: Vec<Option<Arc<dyn Data>>>,
    /// Additional info for each chunk
    pub ids: Vec<Option<RunId>>,
    /// First open index
    pub first: usize,
}
impl MutableData {
    #[allow(clippy::type_complexity)]
    pub fn alloc(
        &mut self,
        len: usize,
    ) -> (usize, &mut Option<RunId>, &mut [Option<Arc<dyn Data>>]) {
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
    pub fn free(&mut self, idx: usize, len: usize) {
        self.ids[idx] = None;
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
    Multiple(HashMap<String, usize>),
}

pub(super) struct ComponentData {
    /// The actual component
    pub component: Arc<dyn Component>,
    /// Components dependent on a primary stream
    pub primary_dependents: Vec<(ComponentId, Option<usize>)>,
    /// Components dependent on a secondary stream
    pub dependents: HashMap<String, Vec<(ComponentId, Option<usize>)>>,
    /// Locked partial data
    pub partial: Mutex<MutableData>,
    /// Name of this component
    pub name: triomphe::Arc<str>,
    /// What inputs this component is expecting
    pub input_mode: InputMode,
    /// This is true if we expect one of our inputs to return multiple times.
    pub multi_input: bool,
}
impl Debug for ComponentData {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentData")
            .field("primary_dependents", &self.primary_dependents)
            .field("dependents", &self.dependents)
            .field("partial", &self.partial)
            .field("name", &self.name)
            .field("multi_input", &self.multi_input)
            .field("input_mode", &self.input_mode)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum AddComponentError {
    #[error("Name already exists with component ID {}", .0.0)]
    AlreadyExists(ComponentId),
    #[error("Empty component name")]
    EmptyName,
    #[error("Non-alphanumeric character in character {index} of {name:?}")]
    InvalidName {
        name: triomphe::Arc<str>,
        index: usize,
    },
}

#[derive(Debug, Clone, PartialEq, Error)]
pub enum AddDependencyError<'a> {
    #[error("Publishing component {0} doesn't exist")]
    NoPublisher(ComponentId),
    #[error("Subscribing component {0} doesn't exist")]
    NoSubscriber(ComponentId),
    #[error("Can't create a self-loop")]
    SelfLoop,
    #[error("Publishing component {component} doesn't have a {}", if let Some(name) = .stream { format!("named stream {name:?}") } else { "primary output stream".to_string() })]
    NoPubStream {
        component: ComponentId,
        stream: Option<&'a str>,
    },
    #[error("Input {stream:?} has already been attached to subscribing component {component}")]
    DuplicateNamedInput {
        component: ComponentId,
        stream: &'a str,
    },
    #[error("Primary input has already been attached to subscribing component {component}")]
    DuplicatePrimaryInput { component: ComponentId },
    #[error("Attempted to mix primary and named inputs for subscribing component {component}")]
    InputTypeMix { component: ComponentId },
    #[error("Subscribing component {component} doesn't take input on a {}", if let Some(name) = .stream { format!("named stream {name:?}") } else { "primary input stream".to_string() })]
    DoesntTakeInput {
        component: ComponentId,
        stream: Option<&'a str>,
    },
}

impl PipelineRunner {
    /// Try to add a new component, returning the ID of one with the same name if there's a conflict
    pub fn add_component(
        &mut self,
        name: impl Into<triomphe::Arc<str>>,
        component: Arc<dyn Component>,
    ) -> Result<ComponentId, AddComponentError> {
        let name = name.into();
        tracing::info!(?name, "adding component");
        if name.is_empty() {
            return Err(AddComponentError::EmptyName);
        }
        if let Some((index, _)) = name
            .char_indices()
            .find(|&(_, c)| !(c == '-' || c == '_' || c.is_alphanumeric()))
        {
            return Err(AddComponentError::InvalidName { name, index });
        }
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
                            InputMode::Multiple(
                                v.into_iter()
                                    .enumerate()
                                    .map(|(v, k)| (k, v | UNBOUND_NAMED_INPUT_MASK))
                                    .collect(),
                            )
                        }
                    }
                };
                self.components.push(ComponentData {
                    component,
                    primary_dependents: Vec::new(),
                    dependents: HashMap::new(),
                    partial: Mutex::new(MutableData {
                        data: Vec::new(),
                        ids: Vec::new(),
                        first: 0,
                    }),
                    name,
                    input_mode,
                    multi_input: false,
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
        tracing::info!(
            "subscribing {sub_id} ({} output) to {pub_id} ({} input)",
            if let Some(name) = pub_stream {
                format!("{name:?}")
            } else {
                "primary".to_string()
            },
            if let Some(name) = sub_stream {
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
        let kind = c1.component.output_kind(pub_stream);
        if kind.is_none() {
            return Err(AddDependencyError::NoPubStream {
                component: pub_id,
                stream: pub_stream,
            });
        }
        if kind.is_multi() {
            c2.multi_input = true;
        }
        #[allow(clippy::collapsible_else_if)]
        if let Some(name) = sub_stream {
            let idx = match &mut c2.input_mode {
                InputMode::Single {
                    name: ex_name,
                    attached,
                } => {
                    if !ex_name.as_ref().is_some_and(|n| n == name) {
                        return Err(AddDependencyError::DoesntTakeInput {
                            component: sub_id,
                            stream: Some(name),
                        });
                    }
                    if *attached {
                        return Err(AddDependencyError::DuplicateNamedInput {
                            component: sub_id,
                            stream: name,
                        });
                    }
                    *attached = true;
                    None
                }
                InputMode::Multiple(m) => {
                    let Some(idx) = m.get_mut(name) else {
                        return Err(AddDependencyError::DoesntTakeInput {
                            component: sub_id,
                            stream: Some(name),
                        });
                    };
                    if *idx & UNBOUND_NAMED_INPUT_MASK == 0 {
                        return Err(AddDependencyError::DuplicateNamedInput {
                            component: sub_id,
                            stream: name,
                        });
                    }
                    *idx &= !UNBOUND_NAMED_INPUT_MASK;
                    Some(*idx)
                }
            };
            if let Some(name) = pub_stream {
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
                    stream: None,
                });
            };
            if *attached {
                return Err(AddDependencyError::DuplicatePrimaryInput { component: sub_id });
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
