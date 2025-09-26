use std::sync::Arc;
use viking_vision::pipeline::prelude::*;

// Here we define the component types we'd like to use in our pipeline.
struct Print;
struct Print2;
struct BroadcastVec;
struct CheckContains;

// Print is a simple component that takes a value on its pimary input and prints it.
impl Component for Print {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, _name: Option<&str>) -> OutputKind {
        OutputKind::None // our printing component doesn't return anything on any channels
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'r, '_, 's>) {
        let Ok(val) = context.get_res(None).and_log_err() else {
            return;
        };
        tracing::info!(?val, "print");
    }
}
// Print2 is a simple component that takes a value on its "a" and "b" inputs and prints them.
impl Component for Print2 {
    fn inputs(&self) -> Inputs {
        Inputs::named(["a", "b"])
    }
    fn output_kind(&self, _name: Option<&str>) -> OutputKind {
        OutputKind::None
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'r, '_, 's>) {
        let Ok(a) = context.get_res("a").and_log_err() else {
            return;
        };
        let Ok(b) = context.get_res("b").and_log_err() else {
            return;
        };
        tracing::info!(?a, ?b, "print");
    }
}
// BroadcastVec is a component that takes a Vec<i32> in (downcasting as necessary) and outputs the vector on its primary output channel, along with each element on its "elem" channel.
impl Component for BroadcastVec {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, name: Option<&str>) -> OutputKind {
        match name {
            None => OutputKind::Single, // on our primary output, we're going to send one value (the original vector)
            Some("elem") => OutputKind::Multiple, // on our secondary output, we're going to send multiple (the elements)
            _ => OutputKind::None,                // we won't send any other outputs
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'r, '_, 's>) {
        let Ok(val) = context.get_as::<Vec<i32>>(None).and_log_err() else {
            return;
        };
        context.submit(None, val.clone()); // here we submit the vector on our primary output channel
        for &elem in &*val {
            context.submit(Some("elem"), Arc::new(elem)); // we can also call submit() multiple times, which will trigger any dependent components
        }
    }
}
// CheckContains takes two named inputs rather than a primary one: a Vec<i32> and an i32 that might be in it. It then sends a single output on its output channel.
impl Component for CheckContains {
    fn inputs(&self) -> Inputs {
        Inputs::named(["vec", "elem"])
    }
    fn output_kind(&self, name: Option<&str>) -> OutputKind {
        if name.is_none() {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'r, '_, 's>) {
        let Ok(vec) = context.get_as::<Vec<i32>>("vec").and_log_err() else {
            return;
        };
        let Ok(elem) = context.get_as::<i32>("elem").inspect_err(|e| e.log_err()) else {
            return;
        };
        context.submit(None, Arc::new(vec.contains(&elem)));
    }
}

fn main() -> anyhow::Result<()> {
    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();
    let _guard = tracing::info_span!("main").entered();
    let mut graph = PipelineGraph::new();
    let broadcast = graph.add_named_component(Arc::new(BroadcastVec), "broadcast")?;
    let print_a = graph.add_named_component(Arc::new(Print), "print/a")?;
    let print_b = graph.add_named_component(Arc::new(Print), "print/b")?;
    let print_c = graph.add_named_component(Arc::new(Print), "print/c")?;
    let print2 = graph.add_named_component(Arc::new(Print2), "print2")?;
    let check_contains = graph.add_named_component(Arc::new(CheckContains), "check-contains")?;
    graph.add_dependency(broadcast, print_a)?;
    graph.add_dependency((broadcast, "elem"), print_b)?;
    graph.add_dependency(broadcast, (check_contains, "vec"))?;
    graph.add_dependency((broadcast, "elem"), (check_contains, "elem"))?;
    graph.add_dependency(check_contains, print_c)?;
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

    // We need a scope to spawn our tasks in to make sure they don't escape past the lifetime of the runner.
    rayon::scope(|scope| {
        // running multiple pipelines within a scope is fine, they all run concurrently
        runner.run((broadcast, vec![1i32, 2, 3]), scope).unwrap();
        runner
            .run((print2, [("a", 1i32), ("b", 2i32)]), scope)
            .unwrap();
    });
    tracing::debug!("runner, after: {runner:#?}");
    Ok(())
}
