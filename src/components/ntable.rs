#![cfg(feature = "ntable")]

use crate::pipeline::PipelineId;
use crate::pipeline::PipelineName;
use crate::pipeline::prelude::*;
use ntable::NtHandle;
use serde::{Deserialize, Serialize};
use smol_str::SmolStr;
use std::any::{Any, TypeId};
use std::cell::Cell;
use std::collections::HashMap;
use std::fmt::Display;

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
pub fn with_handle<R, F: FnOnce(&NtHandle) -> R>(f: F) -> Option<R> {
    unsafe { CLIENT_HANDLE.get().as_ref() }
        .or_else(|| ntable::GLOBAL_HANDLE.get())
        .map(f)
}
/// Shorthand for [`with_handle(Clone::clone)`](with_handle) to clone the current client handle.
#[inline(always)]
pub fn cloned_handle() -> Option<NtHandle> {
    with_handle(Clone::clone)
}

fn add_to_vec(out: &mut Vec<u8>, src: &str, id: &dyn Display, name: &dyn Display) {
    use std::io::Write;
    let mut it = src.as_bytes().iter();
    while let Some(&ch) = it.next() {
        if ch == b'%' {
            match it.next() {
                Some(&b'%') => out.push(b'%'),
                Some(&b'i') => {
                    let _ = write!(out, "{id}");
                }
                Some(&b'N') => {
                    let _ = write!(out, "{name}");
                }
                Some(_) => {
                    let mid = src.len() - it.len();
                    let start = mid - 1;
                    let end = src.ceil_char_boundary(mid);
                    tracing::error!(spec = &src[start..end], "invalid format specifier");
                }
                None => {
                    tracing::error!(src, "expected a format specifier after '%' in string");
                }
            }
        } else {
            out.push(ch);
        }
    }
}

/// Format a prefix and channel as a String, replacing any known format codes.
///
/// Only `%i`, `%N`, and `%%` are known, for the pipeline ID, pipeline name, and literal `%`, respectively.
fn format_channel(prefix: &str, chan: &str, id: &dyn Display, name: &dyn Display) -> String {
    let mut out = Vec::with_capacity(prefix.len() + chan.len());
    if !prefix.starts_with('/') {
        out.push(b'/');
    }
    add_to_vec(&mut out, prefix, id, name);
    if !prefix.ends_with('/') {
        out.push(b'/');
    }
    add_to_vec(&mut out, chan, id, name);
    unsafe { String::from_utf8_unchecked(out) }
}

/// A component that publishes to a network table.
#[derive(Debug, Clone)]
pub struct NtPrimitiveComponent {
    pub nt_handle: Option<NtHandle>,
    /// Prefix of topics for the network table.
    ///
    /// Defaults to an empty string.
    pub prefix: SmolStr,
    /// Remapping of input channels to NT paths.
    ///
    /// If a topic isn't in the remapping table, it publishes to the same
    pub remapping: HashMap<SmolStr, SmolStr>,
}
impl Default for NtPrimitiveComponent {
    fn default() -> Self {
        Self {
            nt_handle: cloned_handle(),
            prefix: SmolStr::new_static(""),
            remapping: HashMap::new(),
        }
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
        let Some(handle) = &self.nt_handle else {
            return;
        };
        let id = context.context.request::<PipelineId>();
        let id = id.as_ref().map_or(&"anon" as _, |v| v as &dyn Display);
        let name = context
            .context
            .request::<PipelineName>()
            .map(|n| n.0)
            .unwrap_or(id);

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
                    let ch = format_channel(&self.prefix, self.remapping.get(chan).unwrap_or(chan), id, name);
                    if idx.0 == 0 {
                        let v = &tree.vals[idx.1 as usize];
                        let va = &**v as &dyn Any;
                        let tid = va.type_id();
                        $(
                            if tid == $name {
                                let v = va.downcast_ref::<$type>().unwrap().clone() as $nt;
                                handle.set(ch, v);
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
                    InProgressArray::Bool(v) => handle.set(chan, v),
                    InProgressArray::Int(v) => handle.set(chan, v),
                    InProgressArray::Float(v) => handle.set(chan, v),
                    InProgressArray::Double(v) => handle.set(chan, v),
                    InProgressArray::String(v) => handle.set(chan, v),
                }
            }
        }
    }
    fn initialize(&self, _graph: &mut PipelineGraph, _self_id: GraphComponentId) {
        if self.nt_handle.is_none() {
            tracing::error!("attempted to initialize a NT component without a handle");
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
macro_rules! impl_for_geom {
    ($($name:ident)*) => {
        $(
            impl AddToInProgress for crate::geom::$name {
                fn push_to(self, arr: &mut InProgressArray) {
                    match arr {
                        InProgressArray::Unset => *arr = InProgressArray::Double(self.0.to_vec()),
                        InProgressArray::Double(vec) => vec.extend_from_slice(&self.0),
                        InProgressArray::Bool(_) => {
                            tracing::error!(
                                array = "bool",
                                self = "double",
                                "attempted to build a heterogenous array"
                            );
                        }
                        InProgressArray::Int(_) => {
                            tracing::error!(
                                array = "int",
                                self = "double",
                                "attempted to build a heterogenous array"
                            );
                        }
                        InProgressArray::Float(_) => {
                            tracing::error!(
                                array = "float",
                                self = "double",
                                "attempted to build a heterogenous array"
                            );
                        }
                        InProgressArray::String(_) => {
                            tracing::error!(
                                array = "string",
                                self = "double",
                                "attempted to build a heterogenous array"
                            );
                        }
                    }
                }
            }
        )*
    };
}
impl_for_geom!(Vec3 Mat3 Quat EulerXYZ);

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NtPrimitiveFactory {
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
}

#[typetag::serde(name = "ntable")]
impl ComponentFactory for NtPrimitiveFactory {
    fn build(&self, _ctx: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(NtPrimitiveComponent {
            nt_handle: cloned_handle(),
            prefix: self.prefix.clone(),
            remapping: self.remapping.clone(),
        })
    }
}
