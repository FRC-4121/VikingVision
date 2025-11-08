use crate::pipeline::prelude::*;
use crate::pipeline::runner::IntoRunParams;
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};
use tracing_subscriber::util::SubscriberInitExt;

struct Cmp<T> {
    send: Sender<Option<T>>,
    msg: T,
}
impl<T> Cmp<T> {
    fn new(send: Sender<Option<T>>, msg: T) -> Arc<Self> {
        Arc::new(Self { send, msg })
    }
}
impl<T: Clone + Send + Sync + 'static> Component for Cmp<T> {
    fn inputs(&self) -> Inputs {
        Inputs::named(["in"])
    }
    fn can_take(&self, _input: &str) -> bool {
        true
    }
    fn output_kind(&self, name: &str) -> OutputKind {
        match name {
            "d1" => OutputKind::Multiple,
            "d2" => OutputKind::Multiple,
            "s1" => OutputKind::Single,
            "s2" => OutputKind::Single,
            _ => OutputKind::None,
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let _ = self.send.send(Some(self.msg.clone()));
        if let Some(msg) = context.get("in") {
            context.submit_if_listening("d1", || msg.clone());
            context.submit_if_listening("d1", || msg.clone());
            context.submit_if_listening("d2", || msg.clone());
            context.submit_if_listening("d2", || msg.clone());
            context.submit_if_listening("s1", || msg.clone());
            context.submit_if_listening("s2", || msg.clone());
        }
    }
}

struct Echo<T> {
    send: Sender<Option<T>>,
    transform: fn(Arc<dyn Data>) -> T,
}
impl<T> Echo<T> {
    fn new(send: Sender<Option<T>>, transform: fn(Arc<dyn Data>) -> T) -> Arc<Self> {
        Arc::new(Self { send, transform })
    }
}
impl<T: Send + Sync + 'static> Component for Echo<T> {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, _name: &str) -> OutputKind {
        OutputKind::None
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        if let Some(data) = context.get(None) {
            let _ = self.send.send(Some((self.transform)(data)));
        }
    }
}

fn assert_terminates<T: Debug>(recv: Receiver<Option<T>>) -> Vec<T> {
    let mut resp = Vec::new();
    let end = Instant::now() + Duration::from_secs(1);
    loop {
        match recv.recv_timeout(end - Instant::now()) {
            Ok(Some(msg)) => resp.push(msg),
            Ok(None) | Err(RecvTimeoutError::Disconnected) => return resp,
            Err(RecvTimeoutError::Timeout) => panic!("1s timeout exceeded, resp: {resp:?}"),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq)]
enum Msg2 {
    Send,
    Recv,
}

#[test]
fn simple() {
    let _guard = tracing_subscriber::fmt()
        .with_test_writer()
        .finish()
        .set_default();
    let mut graph = PipelineGraph::new();
    let (tx, rx) = channel();
    let prod = graph
        .add_named_component(Cmp::new(tx.clone(), Msg2::Send), "prod")
        .unwrap();
    let cons = graph
        .add_named_component(Cmp::new(tx.clone(), Msg2::Recv), "cons")
        .unwrap();
    graph.add_dependency((prod, "s1"), (cons, "in")).unwrap();
    println!("{graph:#?}");
    let (_remap, runner) = graph.compile().unwrap();
    println!("{runner:#?}");
    let resp = rayon::scope(|scope| {
        runner
            .run(
                ("prod", [("in", ())])
                    .into_run_params(&runner)
                    .unwrap()
                    .with_callback(|_| tx.send(None).unwrap()),
                scope,
            )
            .unwrap();
        assert_terminates(rx)
    });
    assert_eq!(resp, &[Msg2::Send, Msg2::Recv]);
    runner.assert_clean().unwrap();
}

#[test]
fn duplicating() {
    let _guard = tracing_subscriber::fmt()
        .with_test_writer()
        .finish()
        .set_default();
    let mut graph = PipelineGraph::new();
    let (tx, rx) = channel();
    let prod = graph
        .add_named_component(Cmp::new(tx.clone(), Msg2::Send), "prod")
        .unwrap();
    let cons = graph
        .add_named_component(Cmp::new(tx.clone(), Msg2::Recv), "cons")
        .unwrap();
    graph.add_dependency((prod, "d1"), (cons, "in")).unwrap();
    println!("{graph:#?}");
    let (remap, runner) = graph.compile().unwrap();
    let p2 = remap[prod];
    let resp = rayon::scope(|scope| {
        runner
            .run(
                (p2, [("in", ())])
                    .into_run_params(&runner)
                    .unwrap()
                    .with_callback(|_| tx.send(None).unwrap()),
                scope,
            )
            .unwrap();
        assert_terminates(rx)
    });
    assert_eq!(resp, &[Msg2::Send, Msg2::Recv, Msg2::Recv]);
    runner.assert_clean().unwrap();
}

#[test]
fn graph_mutation() {
    let _guard = tracing_subscriber::fmt()
        .with_test_writer()
        .finish()
        .set_default();
    let mut graph = PipelineGraph::new();
    let (tx, rx) = channel();
    let p1 = graph
        .add_named_component(Cmp::new(tx.clone(), Msg2::Send), "prod")
        .unwrap();
    let cons = graph
        .add_named_component(Cmp::new(tx.clone(), Msg2::Recv), "cons")
        .unwrap();
    graph.add_dependency((p1, "d2"), (cons, "in")).unwrap();
    graph.remove_component(p1).unwrap();
    let p2 = graph
        .add_named_component(Cmp::new(tx.clone(), Msg2::Send), "prod")
        .unwrap();
    graph.add_dependency((p2, "s1"), (cons, "in")).unwrap();
    println!("{graph:#?}");
    let (remap, runner) = graph.compile().unwrap();
    assert_eq!(runner.components().len(), 2);
    let p2 = remap[p2];
    let resp = rayon::scope(|scope| {
        runner
            .run(
                (p2, [("in", ())])
                    .into_run_params(&runner)
                    .unwrap()
                    .with_callback(|_| tx.send(None).unwrap()),
                scope,
            )
            .unwrap();
        assert_terminates(rx)
    });
    assert_eq!(resp, &[Msg2::Send, Msg2::Recv]);
    runner.assert_clean().unwrap();
}

#[test]
fn branching() {
    #[derive(Debug)]
    enum Message {
        Unsorted1(Vec<i32>),
        Sorted1(Vec<i32>),
        Unsorted2(Vec<i32>),
        Sorted2(Vec<i32>),
    }
    use crate::components::{collect::CollectVecComponent, utils::BroadcastVec};
    let _guard = tracing_subscriber::fmt()
        .with_test_writer()
        .finish()
        .set_default();
    let mut graph = PipelineGraph::new();
    let (tx, rx) = channel();
    let broadcast1 = graph
        .add_named_component(Arc::new(BroadcastVec::<Vec<i32>>::new()), "broadcast1")
        .unwrap();
    let broadcast2 = graph
        .add_named_component(Arc::new(BroadcastVec::<i32>::new()), "broadcast2")
        .unwrap();

    let collect1 = graph
        .add_named_component(Arc::new(CollectVecComponent::<i32>::new()), "collect1")
        .unwrap();
    let collect2 = graph
        .add_named_component(Arc::new(CollectVecComponent::<i32>::new()), "collect2")
        .unwrap();
    let print1s = graph
        .add_named_component(
            Echo::new(tx.clone(), |x| {
                Message::Sorted1(x.downcast().cloned().unwrap())
            }),
            "print-1-sorted",
        )
        .unwrap();
    let print2s = graph
        .add_named_component(
            Echo::new(tx.clone(), |x| {
                Message::Sorted2(x.downcast().cloned().unwrap())
            }),
            "print-2-sorted",
        )
        .unwrap();
    let print1u = graph
        .add_named_component(
            Echo::new(tx.clone(), |x| {
                Message::Unsorted1(x.downcast().cloned().unwrap())
            }),
            "print-1-unsorted",
        )
        .unwrap();
    let print2u = graph
        .add_named_component(
            Echo::new(tx.clone(), |x| {
                Message::Unsorted2(x.downcast().cloned().unwrap())
            }),
            "print-2-unsorted",
        )
        .unwrap();
    graph
        .add_dependency((broadcast1, "elem"), broadcast2)
        .unwrap();
    graph
        .add_dependency((broadcast2, "elem"), (collect1, "elem"))
        .unwrap();
    graph.add_dependency(broadcast2, (collect1, "ref")).unwrap();
    graph
        .add_dependency((broadcast2, "elem"), (collect2, "elem"))
        .unwrap();
    graph.add_dependency(broadcast1, (collect2, "ref")).unwrap();
    graph.add_dependency(collect1, print1u).unwrap();
    graph.add_dependency((collect1, "sorted"), print1s).unwrap();
    graph.add_dependency(collect2, print2u).unwrap();
    graph.add_dependency((collect2, "sorted"), print2s).unwrap();
    println!("{graph:#?}");
    let (resolver, runner) = graph.compile().unwrap();
    let broadcast = resolver.get(broadcast1).unwrap();
    let resp = rayon::scope(|scope| {
        runner
            .run(
                (
                    broadcast,
                    vec![vec![1i32, 2, 3], vec![4, 5, 6], vec![7, 8, 9]],
                )
                    .into_run_params(&runner)
                    .unwrap()
                    .with_callback(|_| tx.send(None).unwrap()),
                scope,
            )
            .unwrap();
        assert_terminates(rx)
    });
    println!("got response: {resp:?}");
    assert_eq!(resp.len(), 8);
    assert_eq!(
        resp.iter()
            .filter_map(|msg| if let Message::Sorted2(v) = msg {
                Some(v)
            } else {
                None
            })
            .collect::<Vec<_>>(),
        vec![&vec![1, 2, 3, 4, 5, 6, 7, 8, 9]],
    );
    let unsorted2 = resp
        .iter()
        .filter_map(|msg| {
            if let Message::Unsorted2(v) = msg {
                Some(v)
            } else {
                None
            }
        })
        .collect::<Vec<_>>();
    let [us2] = &*unsorted2 else {
        panic!("expected a single element in unsorted2, got {unsorted2:?}");
    };
    let mut us2 = us2.to_vec();
    us2.sort();
    assert_eq!(us2, &[1, 2, 3, 4, 5, 6, 7, 8, 9]);
    assert_eq!(
        resp.iter()
            .filter(|msg| matches!(msg, Message::Sorted1(_)))
            .count(),
        3
    );
    assert_eq!(
        resp.iter()
            .filter(|msg| matches!(msg, Message::Unsorted1(_)))
            .count(),
        3
    );
    for elem in &resp {
        let (Message::Sorted1(v) | Message::Unsorted1(v)) = elem else {
            continue;
        };
        assert_eq!(v.len(), 3);
    }
}
