use crate::pipeline::prelude::*;
use crate::pipeline::runner::IntoRunParams;
use std::fmt::Debug;
use std::sync::Arc;
use std::sync::mpsc::{Receiver, RecvTimeoutError, Sender, channel};
use std::time::{Duration, Instant};

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
    fn output_kind(&self, name: Option<&str>) -> OutputKind {
        match name {
            Some("d1") => OutputKind::Multiple,
            Some("d2") => OutputKind::Multiple,
            Some("s1") => OutputKind::Single,
            Some("s2") => OutputKind::Single,
            _ => OutputKind::None,
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'r, '_, 's>) {
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

#[test]
fn simple() {
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Msg {
        Send,
        Recv,
    }
    let mut runner = PipelineRunner::new();
    let (tx, rx) = channel();
    let prod = runner
        .add_component("prod", Cmp::new(tx.clone(), Msg::Send))
        .unwrap();
    let cons = runner
        .add_component("cons", Cmp::new(tx.clone(), Msg::Recv))
        .unwrap();
    runner.add_dependency(prod, "s1", cons, "in").unwrap();
    println!("{runner:#?}");
    let resp = rayon::scope(|scope| {
        runner
            .run(
                (prod, [("in", ())])
                    .into_run_params(&runner)
                    .unwrap()
                    .with_callback(|_| tx.send(None).unwrap()),
                scope,
            )
            .unwrap();
        assert_terminates(rx)
    });
    assert_eq!(resp, &[Msg::Send, Msg::Recv]);
}

#[test]
fn duplicating() {
    #[derive(Debug, Clone, Copy, PartialEq)]
    enum Msg {
        Send,
        Recv,
    }
    let mut runner = PipelineRunner::new();
    let (tx, rx) = channel();
    let prod = runner
        .add_component("prod", Cmp::new(tx.clone(), Msg::Send))
        .unwrap();
    let cons = runner
        .add_component("cons", Cmp::new(tx.clone(), Msg::Recv))
        .unwrap();
    runner.add_dependency(prod, "d1", cons, "in").unwrap();
    println!("{runner:#?}");
    let resp = rayon::scope(|scope| {
        runner
            .run(
                (prod, [("in", ())])
                    .into_run_params(&runner)
                    .unwrap()
                    .with_callback(|_| tx.send(None).unwrap()),
                scope,
            )
            .unwrap();
        assert_terminates(rx)
    });
    assert_eq!(resp, &[Msg::Send, Msg::Recv, Msg::Recv]);
}
