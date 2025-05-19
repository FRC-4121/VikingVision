use std::any::Any;
use std::fmt::Debug;
use std::marker::PhantomData;
use std::sync::Arc;
use tracing::{error, info};
use viking_vision::pipeline::component::{Component, OutputKind};
use viking_vision::pipeline::runner::{ComponentContext, PipelineRunner};

struct Print<T>(PhantomData<T>);
struct BroadcastVec;
struct CheckContains;

impl<T: Debug + Any + Send + Sync> Component for Print<T> {
    fn output_kind(&self, _name: Option<&str>) -> OutputKind {
        OutputKind::None
    }
    fn run<'a, 's, 'r: 's>(&self, context: ComponentContext<'r, 'a, 's>) {
        if let Some(data) = context.get(None) {
            if let Some(val) = data.as_any().downcast_ref::<T>() {
                info!("Got data: {val:?}");
            } else {
                error!("Data wasn't a {}", disqualified::ShortName::of::<T>());
            }
        } else {
            error!("No primary data");
        }
    }
}
impl Component for BroadcastVec {
    fn output_kind(&self, name: Option<&str>) -> OutputKind {
        match name {
            None => OutputKind::Single,
            Some("elem") => OutputKind::Multiple,
            _ => OutputKind::None,
        }
    }
    fn run<'a, 's, 'r: 's>(&self, context: ComponentContext<'r, 'a, 's>) {
        if let Some(data) = context.get(None) {
            if let Some(val) = data.as_any().downcast_ref::<Vec<i32>>() {
                context.submit(None, data.clone());
                for &elem in val {
                    context.submit(Some("elem"), Arc::new(elem));
                }
            } else {
                error!(id = ?data.as_any().type_id(), "Data wasn't a Vec<i32>");
            }
        } else {
            error!("No primary data");
        }
    }
}
impl Component for CheckContains {
    fn output_kind(&self, name: Option<&str>) -> OutputKind {
        if name.is_none() {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'a, 's, 'r: 's>(&self, context: ComponentContext<'r, 'a, 's>) {
        let Some(data) = context.get("elem") else {
            error!("No elem stream");
            return;
        };
        let Some(elem) = data.as_any().downcast_ref::<i32>() else {
            error!(id = ?data.as_any().type_id(), "Data wasn't an i32");
            return;
        };
        let Some(data) = context.get("vec") else {
            error!("No vec stream");
            return;
        };
        let Some(vec) = data.as_any().downcast_ref::<Vec<i32>>() else {
            error!(id = ?data.as_any().type_id(), "Data wasn't an Vec<i32>");
            return;
        };
        context.submit(None, Arc::new(vec.contains(elem)));
    }
}

fn main() {
    tracing_subscriber::fmt().init();
    let mut runner = PipelineRunner::new();
    let broadcast = runner
        .add_component("broadcast", Arc::new(BroadcastVec))
        .unwrap();
    let print_num = runner
        .add_component("print-num", Arc::new(Print::<i32>(PhantomData)))
        .unwrap();
    let print_vec = runner
        .add_component("print-vec", Arc::new(Print::<Vec<i32>>(PhantomData)))
        .unwrap();
    let print_bool = runner
        .add_component("print-bool", Arc::new(Print::<bool>(PhantomData)))
        .unwrap();
    let check_contains = runner
        .add_component("check-contains", Arc::new(CheckContains))
        .unwrap();
    runner
        .add_dependency(broadcast, None, print_vec, None)
        .unwrap();
    runner
        .add_dependency(broadcast, Some("elem"), print_num, None)
        .unwrap();
    runner
        .add_dependency(broadcast, None, check_contains, Some("vec"))
        .unwrap();
    runner
        .add_dependency(broadcast, Some("elem"), check_contains, Some("elem"))
        .unwrap();
    runner
        .add_dependency(check_contains, None, print_bool, None)
        .unwrap();
    rayon::scope(|scope| {
        runner.run(broadcast, Arc::new(vec![1i32, 2, 3]), scope);
    });
    println!("{runner:#?}");
}
