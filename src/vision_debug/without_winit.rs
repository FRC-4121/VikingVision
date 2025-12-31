use super::*;
use crate::buffer::PixelFormat;
use std::collections::HashMap;
use std::io::Write;
use std::process::{Child, Command, Stdio};
use std::sync::mpsc;

/// A sender to a handler.
///
/// This can be used from any thread, but the handler needs to be on the main thread.
#[derive(Clone)]
pub struct Sender(mpsc::Sender<Message>);
impl Sender {
    pub fn send(&self, msg: Message) {
        let _ = self.0.send(msg);
    }
}

pub struct FfmpegProcess {
    child: Child,
    format: PixelFormat,
    running: bool,
}
impl FfmpegProcess {
    pub fn accept(&mut self, mut image: Buffer<'_>) {
        if !self.running {
            return;
        }
        match self.child.try_wait() {
            Ok(None) => {}
            Ok(Some(status)) => {
                tracing::warn!(
                    code = status.code(),
                    id = self.child.id(),
                    "child ffmpeg process stopped"
                );
                self.running = false;
            }
            Err(err) => {
                tracing::error!(%err, id = self.child.id(), "failed to read child ffmpeg process's status");
                self.running = false;
            }
        }
        let Some(stdin) = &mut self.child.stdin else {
            return;
        };
        image.convert_inplace(self.format);
        if let Err(err) = stdin.write_all(&image.data) {
            tracing::error!(%err, id = self.child.id(), "failed to write image data to child ffmpeg process");
            self.running = false;
        }
    }
}

pub fn create_ffmpeg_command(
    path: Option<&str>,
    default_path: &str,
    name: &str,
    id: u128,
    width: u32,
    height: u32,
    mut format: PixelFormat,
) -> Option<FfmpegProcess> {
    let path = match path {
        None | Some("") => default_path,
        Some(s) => s,
    };
    if path.is_empty() {
        tracing::error!("no path set for debugging");
        return None;
    }
    let mut path = path
        .replace("%N", name)
        .replace("%i", &format!("{id:0>32x}"));
    let now = time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
    match time::format_description::parse_strftime_borrowed(&path) {
        Ok(desc) => match now.format(&desc) {
            Ok(fmt) => path = fmt,
            Err(err) => {
                tracing::error!(%err, %path, "failed to format path");
            }
        },
        Err(err) => {
            tracing::error!(%err, %path, "invalid format description for path");
        }
    };
    let pix_fmt = match format {
        PixelFormat::LUMA | PixelFormat::ANON_1 => "gray",
        PixelFormat::YCC => "yuv444p",
        PixelFormat::YUYV => "yuyv422",
        PixelFormat::RGBA => "rgba",
        _ => {
            format = if format.is_anon() {
                PixelFormat::ANON_3
            } else {
                PixelFormat::RGB
            };
            "rgb24"
        }
    };
    let mut cmd = Command::new("ffmpeg");
    cmd.stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::null());
    cmd.args([
        "-f",
        "rawvideo",
        "-pix_fmt",
        pix_fmt,
        "-s",
        &format!("{width}x{height}"),
        "-r",
        "30",
        "-i",
        "-",
        "-c:v",
        "libx264",
        "-crf",
        "23",
        &path,
    ]); // should probably change the framerate at some point
    let child = match cmd.spawn() {
        Ok(child) => {
            tracing::info!(pid = child.id(), "spawned child ffmpeg process");
            child
        }
        Err(err) => {
            tracing::error!(%err, "failed to spawn child process");
            return None;
        }
    };
    Some(FfmpegProcess {
        child,
        format,
        running: true,
    })
}

/// The handler for any incoming messages.
///
/// This should be run on the main thread through [`Self::run`], which will block it until it receives a [`Message::Shutdown`].
pub struct Handler {
    default: DefaultDebug,
    recv: mpsc::Receiver<Message>,
    debugs: HashMap<u128, Option<FfmpegProcess>>,
}
impl Handler {
    /// Create a new handler and a sender.
    #[cfg(not(feature = "debug-gui"))]
    pub fn new(default: DefaultDebug) -> HandlerWithSender {
        let (handler, sender) = Self::new_impl(default);
        HandlerWithSender { handler, sender }
    }
    pub fn new_impl(mut default: DefaultDebug) -> (Self, Sender) {
        if default.mode == Some(DefaultDebugMode::Show) {
            tracing::warn!("showing images isn't supported in this environment");
            default.mode = None;
        }
        let (send, recv) = mpsc::channel();
        (
            Self {
                default,
                recv,
                debugs: HashMap::new(),
            },
            Sender(send),
        )
    }
    /// Create a new handler that can't create windows.
    #[cfg(not(feature = "debug-gui"))]
    pub fn no_gui(default: DefaultDebug) -> HandlerWithSender {
        Self::new(default)
    }
    /// Run the given handler.
    ///
    /// This blocks until a [`Message::Shutdown`] is sent.
    pub fn run(mut self) {
        for msg in self.recv.iter() {
            match msg {
                Message::Shutdown => {
                    self.debugs.clear();
                    break;
                }
                Message::DebugImage(DebugImage {
                    image,
                    name,
                    id,
                    mut mode,
                }) => {
                    let cmd = self.debugs.entry(id).or_insert_with(|| {
                        match mode {
                            DebugMode::Auto => {
                                mode = self.default.mode.map_or(DebugMode::None, From::from);
                            }
                            DebugMode::Show { .. } => {
                                tracing::warn!(
                                    "showing images isn't supported in this environment"
                                );
                                mode = DebugMode::None;
                            }
                            _ => {}
                        }
                        if let DebugMode::Save { path } = mode {
                            create_ffmpeg_command(
                                path.as_deref(),
                                &self.default.default_path,
                                &name,
                                id,
                                image.width,
                                image.height,
                                image.format,
                            )
                        } else {
                            None
                        }
                    });
                    if let Some(proc) = cmd {
                        proc.accept(image);
                    }
                }
            }
        }
    }
}
