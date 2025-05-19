use std::sync::Arc;
use viking_vision::pipeline::prelude::*;

// Here we define the component types we'd like to use in our pipeline.
struct Print;
struct BroadcastVec;
struct CheckContains;

// Print is a simple component that takes a value on its input stream and prints it.
impl Component for Print {
    fn output_kind(&self, _name: Option<&str>) -> OutputKind {
        OutputKind::None // our printing component doesn't return anything on any streams
    }
    fn run<'a, 's, 'r: 's>(&self, context: ComponentContext<'r, 'a, 's>) {
        let Ok(val) = context.get_res(None).inspect_err(|e| e.log_err()) else {
            return;
        };
        tracing::info!("Got data: {val:?}");
    }
}
// BroadcastVec is a component that takes a Vec<i32> in (downcasting as necessary) and outputs the vector on its primary output stream, along with each element on its "elem" stream.
impl Component for BroadcastVec {
    fn output_kind(&self, name: Option<&str>) -> OutputKind {
        match name {
            None => OutputKind::Single, // on our primary output, we're going to send one value (the original vector)
            Some("elem") => OutputKind::Multiple, // on our secondary output, we're going to send multiple (the elements)
            _ => OutputKind::None,                // we won't send any other outputs
        }
    }
    fn run<'a, 's, 'r: 's>(&self, context: ComponentContext<'r, 'a, 's>) {
        let Ok(val) = context
            .get_as::<Vec<i32>>(None)
            .inspect_err(|e| e.log_err())
        else {
            return;
        };
        context.submit(None, val.clone()); // here we submit the vector on our primary output stream
        for &elem in &*val {
            context.submit(Some("elem"), Arc::new(elem)); // we can also call submit() multiple times, which will trigger any dependent components
        }
    }
}
// CheckContains takes two named inputs rather than a primary one: a Vec<i32> and an i32 that might be in it. It then sends a single output on its output stream.
impl Component for CheckContains {
    fn output_kind(&self, name: Option<&str>) -> OutputKind {
        if name.is_none() {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'a, 's, 'r: 's>(&self, context: ComponentContext<'r, 'a, 's>) {
        let Ok(vec) = context
            .get_as::<Vec<i32>>("vec")
            .inspect_err(|e| e.log_err())
        else {
            return;
        };
        let Ok(elem) = context.get_as::<i32>("elem").inspect_err(|e| e.log_err()) else {
            return;
        };
        context.submit(None, Arc::new(vec.contains(&elem)));
    }
}

fn main() {
    tracing_subscriber::fmt().init();
    let mut runner = PipelineRunner::new();
    let broadcast = runner
        .add_component("broadcast", Arc::new(BroadcastVec))
        .unwrap();
    let print = runner.add_component("print", Arc::new(Print)).unwrap();
    let check_contains = runner
        .add_component("check-contains", Arc::new(CheckContains))
        .unwrap();
    runner.add_dependency(broadcast, None, print, None).unwrap();
    runner
        .add_dependency(broadcast, Some("elem"), print, None)
        .unwrap();
    runner
        .add_dependency(broadcast, None, check_contains, Some("vec"))
        .unwrap();
    runner
        .add_dependency(broadcast, Some("elem"), check_contains, Some("elem"))
        .unwrap();
    runner
        .add_dependency(check_contains, None, print, None)
        .unwrap();
    // We need a scope to spawn our tasks in to make sure they don't escape past the lifetime of the runner.
    rayon::scope(|scope| {
        runner.run(broadcast, Arc::new(vec![1i32, 2, 3]), scope);
    });
    // println!("{runner:#?}"); // TODO: remove once I fix everything
}
