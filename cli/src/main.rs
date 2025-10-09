use clap::{Parser, ValueEnum};
use std::fmt::{self, Display, Formatter};
use std::fs::File;
use std::io::IsTerminal;
use std::path::PathBuf;
use std::process::exit;
use tracing::{debug, error, info};
use tracing_subscriber::fmt::writer as tsfw;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use viking_vision::camera::Camera;

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

    let _guard = tracing::error_span!("main").entered();

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

    info!(cameras = ?config.cameras.keys().collect::<Vec<_>>(), "loading cameras");

    let cameras = config
        .cameras
        .into_iter()
        .filter_map(|(name, config)| {
            debug!(name, "loading camera");
            match config.camera.build_camera() {
                Ok(inner) => {
                    debug!(name, "loaded camera");
                    Some(Camera::new(name, inner))
                }
                Err(err) => {
                    error!(%err, "failed to load camera");
                    None
                }
            }
        })
        .collect::<Vec<_>>();

    info!(cameras = ?cameras.iter().map(Camera::name).collect::<Vec<_>>(), "loaded cameras");

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
}
