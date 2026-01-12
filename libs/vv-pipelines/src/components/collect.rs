use crate::pipeline::prelude::*;
use crate::pipeline::runner::RunId;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::marker::PhantomData;
#[cfg(all(feature = "vision", feature = "serde"))]
use vv_vision::buffer::Buffer;
#[cfg(all(feature = "vision", feature = "serde"))]
use vv_vision::vision::Blob;

#[derive(Debug, Default, Clone, Copy)]
pub struct CollectVecComponent<T> {
    pub _marker: PhantomData<T>,
}
impl<T: Data + Clone> CollectVecComponent<T> {
    pub const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
    pub fn new_boxed() -> Box<dyn Component> {
        Box::new(Self::new())
    }
}
impl<T: Data + Clone> Component for CollectVecComponent<T> {
    fn inputs(&self) -> Inputs {
        Inputs::min_tree(["ref", "elem"])
    }
    fn output_kind(&self, name: &str) -> OutputKind {
        match name {
            "" | "sorted" => OutputKind::Single,
            _ => OutputKind::None,
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(tree) = context.get_as::<InputTree>(None).and_log_err() else {
            return;
        };
        let Some(idx) = context.input_indices().and_then(|m| m.get("elem")) else {
            return;
        };
        if context.listening("sorted") {
            let mut vec = tree
                .indexed_iter(idx, RunId(SmallVec::new()))
                .filter_map(|(i, r)| Some((i.downcast::<T>().ok()?.clone(), r)))
                .collect::<Vec<_>>();
            vec.sort_unstable_by(|a, b| a.1.cmp(&b.1));
            context.submit("sorted", vec.into_iter().map(|x| x.0).collect::<Vec<_>>());
        }
        if context.listening("") {
            context.submit(
                "",
                tree.iter(idx)
                    .filter_map(|i| i.downcast::<T>().ok().cloned())
                    .collect::<Vec<_>>(),
            );
        }
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "CVFShim"))]
pub struct CollectVecFactory {
    /// The inner type.
    ///
    /// Currently supported types are:
    /// - integer types
    /// - `f32` and `f64`
    /// - [`String`] as `string`
    /// - [`Buffer`] as `buffer`
    /// - a [`Vec`] of any of the previous types, as the previous wrapped in brackets e.g. `[string]` for `Vec<String>`
    pub inner: String,
    /// The actual construction function.
    ///
    /// This is skipped in de/serialization, and looked up based on the type name
    #[cfg_attr(feature = "serde", serde(skip))]
    pub factory: fn() -> Box<dyn Component>,
}
#[cfg_attr(feature = "serde", typetag::serde(name = "collect-vec"))]
impl ComponentFactory for CollectVecFactory {
    fn build(&self) -> Box<dyn Component> {
        (self.factory)()
    }
}

#[cfg(feature = "serde")]
#[derive(Deserialize)]
struct CVFShim {
    inner: String,
}
#[cfg(feature = "serde")]
impl TryFrom<CVFShim> for CollectVecFactory {
    type Error = String;

    fn try_from(value: CVFShim) -> Result<Self, Self::Error> {
        let factory = match &*value.inner {
            "i8" => CollectVecComponent::<i8>::new_boxed,
            "i16" => CollectVecComponent::<i16>::new_boxed,
            "i32" => CollectVecComponent::<i32>::new_boxed,
            "i64" => CollectVecComponent::<i64>::new_boxed,
            "isize" => CollectVecComponent::<isize>::new_boxed,
            "u8" => CollectVecComponent::<u8>::new_boxed,
            "u16" => CollectVecComponent::<u16>::new_boxed,
            "u32" => CollectVecComponent::<u32>::new_boxed,
            "u64" => CollectVecComponent::<u64>::new_boxed,
            "usize" => CollectVecComponent::<usize>::new_boxed,
            "f32" => CollectVecComponent::<f32>::new_boxed,
            "f64" => CollectVecComponent::<f64>::new_boxed,
            #[cfg(feature = "vision")]
            "blob" => CollectVecComponent::<Blob>::new_boxed,
            #[cfg(feature = "apriltag")]
            "apriltag" => CollectVecComponent::<vv_apriltag::Detection>::new_boxed,
            #[cfg(feature = "vision")]
            "buffer" => CollectVecComponent::<Buffer>::new_boxed,
            "string" => CollectVecComponent::<String>::new_boxed,
            "[i8]" => CollectVecComponent::<Vec<i8>>::new_boxed,
            "[i16]" => CollectVecComponent::<Vec<i16>>::new_boxed,
            "[i32]" => CollectVecComponent::<Vec<i32>>::new_boxed,
            "[i64]" => CollectVecComponent::<Vec<i64>>::new_boxed,
            "[isize]" => CollectVecComponent::<Vec<isize>>::new_boxed,
            "[u8]" => CollectVecComponent::<Vec<u8>>::new_boxed,
            "[u16]" => CollectVecComponent::<Vec<u16>>::new_boxed,
            "[u32]" => CollectVecComponent::<Vec<u32>>::new_boxed,
            "[u64]" => CollectVecComponent::<Vec<u64>>::new_boxed,
            "[usize]" => CollectVecComponent::<Vec<usize>>::new_boxed,
            "[f32]" => CollectVecComponent::<Vec<f32>>::new_boxed,
            "[f64]" => CollectVecComponent::<Vec<f64>>::new_boxed,
            #[cfg(feature = "vision")]
            "[blob]" => CollectVecComponent::<Vec<Blob>>::new_boxed,
            #[cfg(feature = "apriltag")]
            "[apriltag]" => CollectVecComponent::<Vec<vv_apriltag::Detection>>::new_boxed,
            #[cfg(feature = "vision")]
            "[buffer]" => CollectVecComponent::<Vec<Buffer>>::new_boxed,
            "[string]" => CollectVecComponent::<Vec<String>>::new_boxed,
            name => return Err(format!("Unrecognized type {name:?}")),
        };
        Ok(CollectVecFactory {
            inner: value.inner,
            factory,
        })
    }
}

#[derive(Debug, Default, Clone, Copy)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct SelectLastComponent;
impl Component for SelectLastComponent {
    fn inputs(&self) -> Inputs {
        Inputs::min_tree(["ref", "elem"])
    }
    fn output_kind(&self, name: &str) -> OutputKind {
        if name.is_empty() {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(tree) = context.get_as::<InputTree>(None).and_log_err() else {
            return;
        };
        let Some(idx) = context.input_indices().and_then(|m| m.get("elem")) else {
            return;
        };
        if let Some(last) = tree.last(idx) {
            context.submit("", last.clone());
        }
    }
}
#[cfg_attr(feature = "serde", typetag::serde(name = "select-last"))]
impl ComponentFactory for SelectLastComponent {
    fn build(&self) -> Box<dyn Component> {
        Box::new(*self)
    }
}
