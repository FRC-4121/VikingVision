use std::sync::Arc;
use vv_pipelines::components::utils::BroadcastVec;
use vv_pipelines::pipeline::prelude::*;

// see common.rs for more component definitions
mod common;
use common::*;

// Here we define the component types we'd like to use in our pipeline.
struct Print2;
struct CheckContains;

// Print2 is a simple component that takes a value on its "a" and "b" inputs and prints them.
impl Component for Print2 {
    fn inputs(&self) -> Inputs {
        Inputs::named(["a", "b"])
    }
    fn output_kind(&self, _name: &str) -> OutputKind {
        OutputKind::None
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(a) = context.get_res("a").and_log_err() else {
            return;
        };
        let Ok(b) = context.get_res("b").and_log_err() else {
            return;
        };
        tracing::info!(?a, ?b, "print");
    }
}

// CheckContains takes two named inputs rather than a primary one: a Vec<i32> and an i32 that might be in it. It then sends a single output on its output channel.
impl Component for CheckContains {
    fn inputs(&self) -> Inputs {
        Inputs::named(["vec", "elem"])
    }
    fn output_kind(&self, name: &str) -> OutputKind {
        if name.is_empty() {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(vec) = context.get_as::<Vec<i32>>("vec").and_log_err() else {
            return;
        };
        let Ok(elem) = context.get_as::<i32>("elem").inspect_err(|e| e.log_err()) else {
            return;
        };
        context.submit("", Arc::new(vec.contains(&elem)));
    }
}

fn main() -> anyhow::Result<()> {
    let _guard = setup()?;
    let mut graph = PipelineGraph::new();
    let broadcast = graph.add_named_component(Arc::new(BroadcastVec::<i32>::new()), "broadcast")?;
    let print = graph.add_named_component(Arc::new(Print), "print")?;
    let print2 = graph.add_named_component(Arc::new(Print2), "print2")?;
    let check_contains = graph.add_named_component(Arc::new(CheckContains), "check-contains")?;
    graph.add_dependency(broadcast, print)?;
    graph.add_dependency((broadcast, "elem"), print)?;
    graph.add_dependency(broadcast, (check_contains, "vec"))?;
    graph.add_dependency((broadcast, "elem"), (check_contains, "elem"))?;
    graph.add_dependency(check_contains, print)?;
    tracing::debug!("graph: {graph:#?}");
    let (resolver, runner) = graph.compile()?;
    tracing::debug!("remapping: {resolver:#?}");
    tracing::debug!("runner, before: {runner:#?}");
    let broadcast = resolver
        .get(broadcast)
        .ok_or_else(|| anyhow::anyhow!("couldn't find the remapped broadcast component"))?;
    let print2 = resolver
        .get(print2)
        .ok_or_else(|| anyhow::anyhow!("couldn't find the remapped print2 component"))?;

    let _guard = DropGuard(&runner);

    // We need a scope to spawn our tasks in to make sure they don't escape past the lifetime of the runner.
    rayon::scope(|scope| {
        // running multiple pipelines within a scope is fine, they all run concurrently
        runner.run((broadcast, vec![1i32, 2, 3]), scope).unwrap();
        runner
            .run((print2, [("a", 1i32), ("b", 2i32)]), scope)
            .unwrap();
    });
    tracing::debug!("runner, after: {runner:#?}");
    let running = runner.running();
    if running > 0 {
        tracing::error!(running, "processes are still counted as running!");
    }
    let _ = runner.assert_clean();
    Ok(())
}
