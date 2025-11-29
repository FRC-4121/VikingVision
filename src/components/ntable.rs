#![cfg(feature = "ntable")]

use crate::pipeline::prelude::*;
use ntable::{GenericPublisher, NtHandle};
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::any::{Any, TypeId};
use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::{self, Debug, Formatter};
use std::sync::Arc;
use tokio::sync::Mutex;

thread_local! {
    static CLIENT_HANDLE: Cell<*const NtHandle> = const { Cell::new(std::ptr::null()) };
}

/// Call a given closure with a client handle available for the scope of a closure.
pub fn handle_in_scope<R, F: FnOnce() -> R>(handle: &NtHandle, f: F) -> R {
    struct DropGuard(*const NtHandle);
    impl Drop for DropGuard {
        fn drop(&mut self) {
            CLIENT_HANDLE.set(self.0);
        }
    }
    let _guard = DropGuard(CLIENT_HANDLE.replace(handle));
    f()
}
/// Access the client handle passed to [`handle_in_scope`] from inside the closure body.
pub fn with_handle<R, F: FnOnce(&NtHandle) -> R>(f: F) -> R {
    if let Some(client) =
        unsafe { CLIENT_HANDLE.get().as_ref() }.or_else(|| ntable::GLOBAL_HANDLE.get())
    {
        f(client)
    } else {
        panic!("No client handle available, either thread-local or global!")
    }
}
/// Shorthand for [`with_handle(Clone::clone)`](with_handle) to clone the current client handle.
#[inline(always)]
pub fn cloned_handle() -> NtHandle {
    with_handle(Clone::clone)
}

/// A component that publishes to a network table.
#[derive(Clone, Serialize, Deserialize)]
pub struct NtPrimitiveComponent {
    #[serde(skip, default = "cloned_handle")]
    pub nt_handle: NtHandle,
    /// Prefix of topics for the network table.
    ///
    /// Defaults to an empty string.
    #[serde(default)]
    pub prefix: SmolStr,
    /// Remapping of input channels to NT paths.
    ///
    /// If a topic isn't in the remapping table, it publishes to the same
    #[serde(default, skip_serializing_if = "HashMap::is_empty")]
    pub remapping: HashMap<SmolStr, SmolStr>,
    #[serde(skip)]
    pub publishers: Arc<Mutex<HashMap<String, GenericPublisher>>>,
}
impl Default for NtPrimitiveComponent {
    fn default() -> Self {
        Self {
            nt_handle: cloned_handle(),
            prefix: SmolStr::new_static(""),
            remapping: HashMap::new(),
            publishers: Arc::default(),
        }
    }
}
impl Debug for NtPrimitiveComponent {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("NtPublishComponent").finish_non_exhaustive()
    }
}
impl Component for NtPrimitiveComponent {
    fn inputs(&self) -> Inputs {
        Inputs::FullTree(Vec::new())
    }
    fn can_take(&self, _input: &str) -> bool {
        true
    }
    fn output_kind(&self, _name: &str) -> OutputKind {
        OutputKind::None
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let mut in_progress = Vec::new();
        let Ok(tree) = context.get_as::<InputTree>(None).and_log_err() else {
            return;
        };
        let Some(inputs) = context.input_indices() else {
            return;
        };
        macro_rules! define_ids {
            ($($type:ty => $name:ident as $nt:ty,)*) => {
                $(const $name: TypeId = TypeId::of::<$type>();)*
                fn walk_tree(tree: &InputTree, mut in_progress: &mut [Vec<(String, InProgressArray)>]) {
                    let ip = in_progress.split_off_first_mut().unwrap();
                    for (v, (chan, slot)) in tree.vals.iter().zip(ip) {
                        let va = &**v as &dyn Any;
                        let tid = va.type_id();
                        $(
                            if tid == $name {
                                let v = va.downcast_ref::<$type>().unwrap().clone() as $nt;
                                v.push_to(slot);
                                continue;
                            }
                        )*
                        tracing::error!(chan = &**chan, type = %disqualified::ShortName(v.type_name()), "unknown type ID");
                    }
                    for opt in &tree.next {
                        let Some(next) = opt else { continue };
                        walk_tree(next, in_progress);
                    }
                }
                for (chan, idx) in inputs {
                    let ch = self.prefix.to_string() + self.remapping.get(chan).unwrap_or(chan);
                    if idx.0 == 0 {
                        let v = &tree.vals[idx.1 as usize];
                        let va = &**v as &dyn Any;
                        let tid = va.type_id();
                        $(
                            if tid == $name {
                                let v = va.downcast_ref::<$type>().unwrap().clone() as $nt;
                                self.nt_handle.set(ch, v);
                                continue;
                            }
                        )*
                        tracing::error!(chan = &**chan, type = %disqualified::ShortName(v.type_name()), "unknown type ID");
                    } else {
                        in_progress.resize_with(idx.0 as _, Vec::new);
                        let row = &mut in_progress[idx.1 as usize];
                        let i = idx.1 as usize;
                        row.resize_with(i + 1, || (String::new(), InProgressArray::Unset));
                        row[i].0 = ch;
                    }
                }
            };
        }
        define_ids!(
            i8 => I8 as i64,
            u8 => U8 as i64,
            i16 => I16 as i64,
            u16 => U16 as i64,
            i32 => I32 as i64,
            u32 => U32 as i64,
            i64 => I64 as i64,
            u64 => U64 as i64,
            isize => ISIZE as i64,
            usize => USIZE as i64,
            f32 => F32 as f32,
            f64 => F64 as f64,
            String => STRING as String,
        );
        for opt in &tree.next {
            let Some(tree) = opt else { continue };
            walk_tree(tree, &mut in_progress);
        }
        for row in in_progress {
            for (chan, arr) in row {
                match arr {
                    InProgressArray::Unset => {}
                    InProgressArray::Bool(v) => self.nt_handle.set(chan, v),
                    InProgressArray::Int(v) => self.nt_handle.set(chan, v),
                    InProgressArray::Float(v) => self.nt_handle.set(chan, v),
                    InProgressArray::Double(v) => self.nt_handle.set(chan, v),
                    InProgressArray::String(v) => self.nt_handle.set(chan, v),
                }
            }
        }
    }
}

enum InProgressArray {
    Unset,
    Bool(Vec<bool>),
    Int(Vec<i64>),
    Float(Vec<f32>),
    Double(Vec<f64>),
    String(Vec<String>),
}

trait AddToInProgress {
    fn push_to(self, arr: &mut InProgressArray);
}
macro_rules! impl_atip {
    ($($self:ty, $pat:ident, $this:expr;)*) => {
        $(
            impl AddToInProgress for $self {
                fn push_to(self, arr: &mut InProgressArray) {
                    #[allow(unreachable_patterns)]
                    match arr {
                        InProgressArray::Unset => *arr = InProgressArray::$pat(vec![self]),
                        InProgressArray::$pat(vec) => vec.push(self),
                        InProgressArray::Bool(_) => {
                            tracing::error!(
                                array = "bool",
                                self = $this,
                                "attempted to build a heterogenous array"
                            );
                        }
                        InProgressArray::Int(_) => {
                            tracing::error!(
                                array = "int",
                                self = $this,
                                "attempted to build a heterogenous array"
                            );
                        }
                        InProgressArray::Float(_) => {
                            tracing::error!(
                                array = "float",
                                self = $this,
                                "attempted to build a heterogenous array"
                            );
                        }
                        InProgressArray::Double(_) => {
                            tracing::error!(
                                array = "double",
                                self = $this,
                                "attempted to build a heterogenous array"
                            );
                        }
                        InProgressArray::String(_) => {
                            tracing::error!(
                                array = "string",
                                self = $this,
                                "attempted to build a heterogenous array"
                            );
                        }
                    }
                }
            }
            impl AddToInProgress for Vec<$self> {
                fn push_to(mut self, arr: &mut InProgressArray) {
                    #[allow(unreachable_patterns)]
                    match arr {
                        InProgressArray::Unset => *arr = InProgressArray::$pat(self),
                        InProgressArray::$pat(vec) => vec.append(&mut self),
                        InProgressArray::Bool(_) => {
                            tracing::error!(
                                array = "bool",
                                self = $this,
                                "attempted to build a heterogenous array"
                            );
                        }
                        InProgressArray::Int(_) => {
                            tracing::error!(
                                array = "int",
                                self = $this,
                                "attempted to build a heterogenous array"
                            );
                        }
                        InProgressArray::Float(_) => {
                            tracing::error!(
                                array = "float",
                                self = $this,
                                "attempted to build a heterogenous array"
                            );
                        }
                        InProgressArray::Double(_) => {
                            tracing::error!(
                                array = "double",
                                self = $this,
                                "attempted to build a heterogenous array"
                            );
                        }
                        InProgressArray::String(_) => {
                            tracing::error!(
                                array = "string",
                                self = $this,
                                "attempted to build a heterogenous array"
                            );
                        }
                    }
                }
            }
        )*
    };
}
impl_atip!(
    bool, Bool, "bool";
    i64, Int, "int";
    f32, Float, "float";
    f64, Double, "double";
    String, String, "string";
);
