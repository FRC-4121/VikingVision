use std::fs::File;
use std::io::Stderr;
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

    std::panic::set_hook(Box::new(|panic_info| {
        tracing::error!(
            "panic: {panic_info}\n{}",
            std::backtrace::Backtrace::capture()
        )
    }));

    Ok(tracing::info_span!("main").entered())
}
