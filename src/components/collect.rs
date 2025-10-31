use crate::pipeline::prelude::*;
use crate::pipeline::runner::RunId;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;
use std::marker::PhantomData;

#[derive(Debug, Clone, Copy)]
pub struct CollectVec<T> {
    pub _marker: PhantomData<T>,
}
impl<T: Data + Clone> Component for CollectVec<T> {
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

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct SelectLast;
impl Component for SelectLast {
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
#[typetag::serde(name = "select-last")]
impl ComponentFactory for SelectLast {
    fn build(&self, _ctx: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(*self)
    }
}
