use std::sync::Arc;
use viking_vision::pipeline::prelude::*;

mod common;
use common::*;

// use mocks::*;
// mock components that don't take trees
#[allow(dead_code)]
mod mocks {
    use std::marker::PhantomData;
    use viking_vision::pipeline::prelude::*;
    pub struct CollectVec<T> {
        _marker: PhantomData<T>,
    }
    impl<T> CollectVec<T> {
        pub const fn new() -> Self {
            Self {
                _marker: PhantomData,
            }
        }
    }
    impl<T: Data + Clone> Component for CollectVec<T> {
        fn inputs(&self) -> Inputs {
            Inputs::named(["ref", "elem"])
        }
        fn output_kind(&self, name: &str) -> OutputKind {
            match name {
                "" | "sorted" => OutputKind::Single,
                _ => OutputKind::None,
            }
        }
        fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
            context.submit("sorted", ());
            context.submit("", ());
        }
    }

    pub struct SelectLast;
    impl Component for SelectLast {
        fn inputs(&self) -> Inputs {
            Inputs::named(["ref", "elem"])
        }
        fn output_kind(&self, name: &str) -> OutputKind {
            if name.is_empty() {
                OutputKind::Single
            } else {
                OutputKind::None
            }
        }
        fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
            context.submit("", ());
        }
    }
}

fn main() -> anyhow::Result<()> {
    let _guard = setup()?;
    let mock = std::env::args().nth(2).as_deref() == Some("mock");
    let mut graph = PipelineGraph::new();
    let broadcast1 =
        graph.add_named_component(Arc::new(BroadcastVec::<Vec<i32>>::new()), "broadcast1")?;
    let broadcast2 =
        graph.add_named_component(Arc::new(BroadcastVec::<i32>::new()), "broadcast2")?;

    let (collect1, collect2, last);
    if mock {
        use mocks::*;
        collect1 = graph.add_named_component(Arc::new(CollectVec::<i32>::new()), "collect1")?;
        collect2 = graph.add_named_component(Arc::new(CollectVec::<i32>::new()), "collect2")?;
        last = graph.add_named_component(Arc::new(SelectLast), "last")?;
    } else {
        use viking_vision::components::prelude::*;
        collect1 = graph.add_named_component(Arc::new(CollectVec::<i32>::new()), "collect1")?;
        collect2 = graph.add_named_component(Arc::new(CollectVec::<i32>::new()), "collect2")?;
        last = graph.add_named_component(Arc::new(SelectLast), "last")?;
    };
    let print = graph.add_named_component(Arc::new(Print), "print")?;
    graph.add_dependency((broadcast1, "elem"), broadcast2)?;
    graph.add_dependency((broadcast2, "elem"), (collect1, "elem"))?;
    graph.add_dependency(broadcast2, (collect1, "ref"))?;
    graph.add_dependency((broadcast2, "elem"), (collect2, "elem"))?;
    graph.add_dependency(broadcast1, (collect2, "ref"))?;
    graph.add_dependency((broadcast2, "elem"), (last, "elem"))?;
    graph.add_dependency(broadcast2, (last, "ref"))?;
    graph.add_dependency(collect1, print)?;
    graph.add_dependency((collect1, "sorted"), print)?;
    graph.add_dependency(collect2, print)?;
    graph.add_dependency((collect2, "sorted"), print)?;
    graph.add_dependency(last, print)?;
    tracing::debug!("graph: {graph:#?}");
    let (resolver, runner) = graph.compile()?;
    tracing::debug!("remapping: {resolver:#?}");
    tracing::debug!("runner, before: {runner:#?}");
    let broadcast = resolver
        .get(broadcast1)
        .ok_or_else(|| anyhow::anyhow!("couldn't find the remapped broadcast component"))?;

    let _guard = DropGuard(&runner);

    rayon::scope(|scope| {
        runner
            .run(
                (
                    broadcast,
                    vec![vec![1i32, 2, 3], vec![4, 5, 6], vec![7, 8, 9]],
                ),
                scope,
            )
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
