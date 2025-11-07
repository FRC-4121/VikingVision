use std::fs::File;
use std::io::Stderr;
use std::marker::PhantomData;
use std::sync::Arc;
use tracing_subscriber::fmt::writer::{MakeWriter, OptionalWriter, Tee};
use viking_vision::pipeline::prelude::*;

pub struct Print;
// Print is a simple component that takes a value on its pimary input and prints it.
impl Component for Print {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, _name: &str) -> OutputKind {
        OutputKind::None // our printing component doesn't return anything on any channels
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(val) = context.get_res(None).and_log_err() else {
            return;
        };
        tracing::info!(?val, "print");
    }
}

pub struct BroadcastVec<T> {
    _marker: PhantomData<T>,
}
impl<T> BroadcastVec<T> {
    #[allow(clippy::new_without_default)]
    pub const fn new() -> Self {
        Self {
            _marker: PhantomData,
        }
    }
}
// BroadcastVec is a component that takes a Vec<i32> in (downcasting as necessary) and outputs the vector on its primary output channel, along with each element on its "elem" channel.
impl<T: Data + Clone> Component for BroadcastVec<T> {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, name: &str) -> OutputKind {
        match name {
            "" => OutputKind::Single, // on our primary output, we're going to send one value (the original vector)
            "elem" => OutputKind::Multiple, // on our secondary output, we're going to send multiple (the elements)
            _ => OutputKind::None,          // we won't send any other outputs
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(val) = context.get_as::<Vec<T>>(None).and_log_err() else {
            return;
        };
        context.submit("", val.clone()); // here we submit the vector on our primary output channel
        for elem in &*val {
            context.submit("elem", Arc::new(elem.clone())); // we can also call submit() multiple times, which will trigger any dependent components
        }
    }
}

struct Writer(Option<File>);
impl<'a> MakeWriter<'a> for Writer {
    type Writer = Tee<OptionalWriter<&'a File>, Stderr>;

    fn make_writer(&'a self) -> Self::Writer {
        Tee::new(
            self.0
                .as_ref()
                .map_or_else(OptionalWriter::none, OptionalWriter::some),
            std::io::stderr(),
        )
    }
}

pub struct DropGuard<'a>(pub &'a PipelineRunner);
impl Drop for DropGuard<'_> {
    fn drop(&mut self) {
        if std::thread::panicking() {
            tracing::debug!("panicking: {:#?}", self.0);
        }
    }
}

pub fn setup() -> std::io::Result<tracing::span::EnteredSpan> {
    let file = std::env::args_os()
        .nth(1)
        .map(std::fs::File::create)
        .transpose()?;

    tracing_subscriber::fmt()
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .with_writer(Writer(file))
        .init();
    Ok(tracing::info_span!("main").entered())
}
