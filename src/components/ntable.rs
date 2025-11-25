#![cfg(feature = "ntable")]

use crate::pipeline::prelude::*;
use nt_client::ClientHandle;
use std::fmt::{self, Debug, Formatter};
use tokio::runtime::*;

#[derive(Clone)]
pub struct NtPublishComponent {
    pub tokio_handle: Handle,
    pub nt_handle: ClientHandle,
}
impl Debug for NtPublishComponent {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("NtPublishComponent").finish_non_exhaustive()
    }
}
impl Component for NtPublishComponent {
    fn inputs(&self) -> Inputs {
        Inputs::FullTree(Vec::new())
    }
    fn can_take(&self, _input: &str) -> bool {
        true
    }
    fn output_kind(&self, _name: &str) -> OutputKind {
        OutputKind::None
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {}
}
