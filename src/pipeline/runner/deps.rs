use super::*;
use std::sync::LazyLock;

#[derive(Debug)]
pub(super) struct InputTree {
    vals: SmallVec<[Arc<dyn Data>; 2]>,
    next: Vec<InputTree>,
    first_open: u32,
    invoc: u32,
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub(crate) struct InputIndex(pub u32, pub u32);

#[derive(Debug)]
pub(crate) struct MutableData {
    pub inputs: Vec<InputTree>,
    /// First open index
    pub first: usize,
}

#[derive(Debug)]
pub(super) enum InputMode {
    Single {
        name: Option<SmolStr>,
    },
    Multiple {
        lookup: HashMap<SmolStr, InputIndex>,
        tree_shape: SmallVec<[u32; 2]>,
    },
}
struct PlaceholderData;
impl Data for PlaceholderData {}
static PLACEHOLDER_DATA: LazyLock<Arc<dyn Data>> = LazyLock::new(|| Arc::new(PlaceholderData));

/// Data associated with components.
pub struct ComponentData {
    pub component: Arc<dyn Component>,
    pub name: SmolStr,
    pub(crate) mutable: Mutex<MutableData>,
    pub(crate) dependents: HashMap<Option<SmolStr>, Vec<(RunnerComponentId, InputIndex)>>,
    pub(super) input_mode: InputMode,
}
impl Debug for ComponentData {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("ComponentData")
            .field("dependents", &self.dependents)
            .field("mutable", &self.mutable)
            .field("name", &self.name)
            .field("input_mode", &self.input_mode)
            .finish_non_exhaustive()
    }
}
