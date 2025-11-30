use clap::{Parser, ValueEnum};
use std::fmt::{self, Display, Formatter};
use std::fs::File;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::exit;
use tracing::{debug, error, error_span, info};
use tracing_subscriber::fmt::writer as tsfw;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use viking_vision::camera::Camera;
use viking_vision::pipeline::prelude::*;

#[cfg(not(windows))]
fn env_allows_color() -> bool {
    match std::env::var_os("TERM") {
        // If TERM isn't set, then we are in a weird environment that
        // probably doesn't support colors.
        None => return false,
        Some(k) => {
            if k == "dumb" {
                return false;
            }
        }
    }
    // If TERM != dumb, then the only way we don't allow colors at this
    // point is if NO_COLOR is set.
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    true
}

#[cfg(windows)]
fn env_allows_color() -> bool {
    // On Windows, if TERM isn't set, then we shouldn't automatically
    // assume that colors aren't allowed. This is unlike Unix environments
    // where TERM is more rigorously set.
    if let Some(k) = std::env::var_os("TERM") {
        if k == "dumb" {
            return false;
        }
    }
    // If TERM != dumb, then the only way we don't allow colors at this
    // point is if NO_COLOR is set.
    if std::env::var_os("NO_COLOR").is_some() {
        return false;
    }
    true
}

#[derive(Debug, Clone, Copy, ValueEnum)]
enum Color {
    Auto,
    Always,
    Never,
}
impl Color {
    const fn to_str(self) -> &'static str {
        match self {
            Self::Auto => "auto",
            Self::Always => "always",
            Self::Never => "never",
        }
    }
    fn use_ansi(&self) -> bool {
        match self {
            Self::Always => true,
            Self::Never => false,
            Self::Auto => env_allows_color() && std::io::stdout().is_terminal(),
        }
    }
}
impl Display for Color {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str(self.to_str())
    }
}

#[derive(Parser)]
struct Cli {
    /// The TOML config file to load
    config: PathBuf,
    /// The log file to write to
    ///
    /// This can be formatted with strftime placeholders.
    log_file: Option<String>,
    /// Whether or not colored output should be used
    ///
    /// Log files never have color, and by default, color support is auto-detected.
    #[arg(short, long, default_value_t = Color::Auto)]
    color: Color,
    /// Regex to match cameras against
    ///
    /// If unspecified, all available cameras will be used
    #[arg(short, long)]
    filter: Option<String>,
}

fn format_log_file(arg: &str, now: time::OffsetDateTime) -> String {
    match time::format_description::parse_strftime_borrowed(arg) {
        Ok(desc) => match now.format(&desc) {
            Ok(fmt) => fmt,
            Err(err) => {
                eprintln!("failed to format argument: {err}");
                exit(1);
            }
        },
        Err(err) => {
            eprintln!("invalid format description: {err}");
            exit(1);
        }
    }
}

struct Writer(Option<File>);
impl<'a> tsfw::MakeWriter<'a> for Writer {
    type Writer = tsfw::OptionalWriter<&'a File>;

    fn make_writer(&'a self) -> Self::Writer {
        self.0
            .as_ref()
            .map_or_else(tsfw::OptionalWriter::none, tsfw::OptionalWriter::some)
    }
}

fn main() {
    let args = Cli::parse();
    let startup_time =
        time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    let path = args
        .log_file
        .as_ref()
        .map(|a| format_log_file(a, startup_time));
    let log_file = path.as_ref().map(|path| {
        File::options()
            .append(true)
            .create(true)
            .open(path)
            .unwrap_or_else(|err| {
                eprintln!("failed to open log file at {path}: {err}");
                exit(2);
            })
    });

    tracing_subscriber::registry()
        .with(tracing_subscriber::EnvFilter::from_default_env())
        .with(tracing_subscriber::fmt::layer().with_ansi(args.color.use_ansi()))
        .with(
            tracing_subscriber::fmt::layer()
                .with_ansi(false)
                .with_writer(Writer(log_file)),
        )
        .init();

    let _guard = error_span!("main").entered();

    info!(path = path.as_deref(), "starting logging at {startup_time}");

    let config_file = match std::fs::read(&args.config) {
        Ok(file) => {
            info!(path = ?args.config, "loaded config file");
            file
        }
        Err(err) => {
            error!(path = ?args.config, %err, "failed to load config file");
            exit(2);
        }
    };
    let mut config = match toml::from_slice::<viking_vision::serialized::ConfigFile>(&config_file) {
        Ok(config) => {
            info!(
                cameras = config.cameras.len(),
                components = config.components.0.len(),
                "loaded config file"
            );
            config
        }
        Err(err) => {
            error!(%err, "failed to parse config file");
            exit(3);
        }
    };

    if let Some(nt) = config.ntable {
        nt.init();
    }

    if let Some(filter) = &args.filter {
        info!(filter, "filtering cameras with regex");
        match matchers::Pattern::new(filter) {
            Ok(pat) => {
                debug!("compiled pattern");
                config.cameras.retain(|k, _| pat.matches(k));
            }
            Err(err) => {
                error!(%err, "failed to compile pattern, no cameras will be matched");
                config.cameras.clear();
            }
        }
    } else {
        info!("no filter specified, using all available cameras");
    }

    let graph = match config
        .components
        .build_graph(&mut viking_vision::utils::NoContext)
    {
        Ok(graph) => {
            info!("built pipeline graph");
            graph
        }
        Err(err) => {
            error!(%err, "failed to build pipeline graph");
            exit(3);
        }
    };
    let (_, runner) = match graph.compile() {
        Ok(runner) => {
            info!("built pipeline runner");
            runner
        }
        Err(err) => {
            error!(%err, "failed to compile runner");
            exit(3);
        }
    };

    info!(cameras = ?config.cameras.keys().collect::<Vec<_>>(), "loading cameras");

    let cameras = config
        .cameras
        .into_iter()
        .filter_map(|(name, mut config)| {
            let _guard = error_span!("load", name).entered();
            debug!("loading camera");
            match config.camera.build_camera() {
                Ok(inner) => {
                    debug!("loaded camera");
                    config.outputs.extend(config.output);
                    let targets = config
                        .outputs
                        .into_iter()
                        .filter_map(|ch| {
                            let opt = runner.lookup.get(&ch.component);
                            if let Some(&id) = opt {
                                debug!(name = &*ch.component, %id, "found component to send frames to");
                                let Some(comp) = runner.component(id) else {
                                    error!("lookup table points to a nonexistent component");
                                    return None;
                                };
                                let inputs = comp.component.inputs();
                                if inputs.expecting() > 1 {
                                    error!(expecting = inputs.expecting(), "sending input to a component that doesn't expect one input");
                                    return None;
                                }
                                if !inputs.can_take(ch.channel.as_deref(), Some(&*comp.component)) {
                                    error!(channel = ?ch.channel, ?inputs, "component can't take input on the expected channel");
                                    return None;
                                }
                                Some((id, ch.channel))
                            } else {
                                error!(name = &*ch.component, "couldn't find a component with the given name");
                                None
                            }
                        })
                        .collect::<Vec<_>>();
                    Some((Camera::new(name, inner), targets))
                }
                Err(err) => {
                    error!(%err, "failed to load camera");
                    None
                }
            }
        })
        .collect::<Vec<_>>();

    info!(cameras = ?cameras.iter().map(|c| c.0.name()).collect::<Vec<_>>(), "loaded cameras");

    let runner = &runner;
    let mut refs = Vec::new();
    refs.resize_with(cameras.len(), || None);

    rayon::scope(|rscope| {
        std::thread::scope(|tscope| {
            for ((mut cam, next), provider) in cameras.into_iter().zip(&mut refs) {
                let builder = std::thread::Builder::new().name(format!("camera-{}", cam.name()));
                let cam_name = cam.name().to_string();
                let provider = &*provider
                    .get_or_insert(PipelineProvider::from_ptr(cam.inner(), cam_name.clone()));
                let res = builder.spawn_scoped(tscope, move || {
                    use viking_vision::pipeline::runner::{
                        RunError, RunErrorCause, RunErrorWithParams,
                    };
                    loop {
                        if let Ok(frame) = cam.read() {
                            let arg = ComponentArgs::single(frame.into_static());
                            for (id, _chan) in &next {
                                let res = runner.run(
                                    RunParams::new(*id)
                                        .with_args(arg.clone())
                                        .with_context(provider)
                                        .with_max_running(config.config.max_running),
                                    rscope,
                                );
                                if let Err(RunError::WithParams(RunErrorWithParams {
                                    cause, ..
                                })) = res
                                {
                                    match cause {
                                        RunErrorCause::ArgsMismatch { expected, given } => {
                                            error!(expected, given, "argument length mismatch");
                                            return;
                                        }
                                        RunErrorCause::NoComponent(id) => {
                                            error!(%id, "missing component");
                                            return;
                                        }
                                        _ => {}
                                    }
                                }
                            }
                        }
                    }
                });
                if let Err(err) = res {
                    error!(camera = cam_name, %err, "failed to spawn thread");
                }
            }
        });
    });
}
