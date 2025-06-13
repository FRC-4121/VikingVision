use crate::buffer::Buffer;
use crate::buffer::PixelFormat;
use crate::pipeline::PipelineId;
use crate::pipeline::PipelineName;
use crate::pipeline::prelude::*;
use serde::Deserialize;
use serde::Serialize;
use std::borrow::Cow;
use std::collections::HashMap;
use std::fmt::{self, Debug, Display, Formatter};
use std::io::Write;
use std::process::Stdio;
use std::process::{Child, ChildStdin, Command};
use std::sync::PoisonError;
use std::sync::{Mutex, RwLock};
use tracing::error;
use tracing::info;
use tracing::warn;

#[derive(Debug)]
pub struct FfmpegComponent {
    ffmpeg: Cow<'static, str>,
    framerate: f64,
    args: Vec<String>,
    #[allow(clippy::type_complexity)]
    running: RwLock<HashMap<Option<PipelineId>, (Mutex<Child>, Mutex<ChildStdin>)>>,
}
impl FfmpegComponent {
    /// Create a new component, ready to output to `ffmpeg``.
    ///
    /// `ffmpeg` is the name of the command, and `Cow::Borrowed("ffmpeg")` works as a default.
    /// `args` go *after* the input is specified, and the only control over the input format is the expected framerate.
    /// The arguments can be formatted with the `strftime` format,
    pub fn new(ffmpeg: Cow<'static, str>, args: Vec<String>, framerate: f64) -> Self {
        Self {
            ffmpeg,
            args,
            framerate,
            running: RwLock::new(HashMap::new()),
        }
    }
    pub fn validate_args(args: &[String]) -> Result<(), time::error::InvalidFormatDescription> {
        args.iter().try_for_each(|a| {
            time::format_description::parse_strftime_borrowed(
                &a.replace("%i", "").replace("%N", ""),
            )
            .map(drop)
        })
    }
    fn format_args(
        cmd: &mut Command,
        args: &[String],
        id: Option<PipelineId>,
        name: Option<&dyn Display>,
    ) {
        let now =
            time::OffsetDateTime::now_local().unwrap_or_else(|_| time::OffsetDateTime::now_utc());
        for arg in args {
            let id = id.as_ref().map_or("anon".to_string(), ToString::to_string);
            let mut arg = arg
                .replace("%i", &id)
                .replace("%N", &name.map_or(id, ToString::to_string));
            match time::format_description::parse_strftime_borrowed(&arg) {
                Ok(desc) => match now.format(&desc) {
                    Ok(fmt) => arg = fmt,
                    Err(err) => {
                        error!(%err, "failed to format argument");
                    }
                },
                Err(err) => {
                    error!(%err, "invalid format description");
                }
            }
            cmd.arg(arg);
        }
    }
    fn prep_command(cmd: &mut Command, buffer: Buffer<'_>, framerate: f64) {
        let pix_fmt = match buffer.format {
            PixelFormat::Gray | PixelFormat::GrayA | PixelFormat::Luma | PixelFormat::LumaA => {
                "gray"
            }
            PixelFormat::Rgb | PixelFormat::Hsv => "rgb24",
            PixelFormat::Rgba | PixelFormat::Hsva => "rgba",
            PixelFormat::YCbCr | PixelFormat::YCbCrA => "yuv444p",
            PixelFormat::Yuyv => "yuyv422",
        };
        cmd.args(["-f", "rawvideo", "-pix_fmt", pix_fmt, "-s"]);
        cmd.arg(format!("{}x{}", buffer.width, buffer.height));
        cmd.arg("-r");
        cmd.arg(framerate.to_string());
        cmd.args(["-i", "-"]);
    }
}
impl Component for FfmpegComponent {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, _name: Option<&str>) -> OutputKind {
        OutputKind::None
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'r, '_, 's>) {
        let Ok(frame) = context.get_as::<Buffer>(None).and_log_err() else {
            return;
        };
        let converted = match frame.format {
            PixelFormat::Gray | PixelFormat::GrayA | PixelFormat::Luma | PixelFormat::LumaA => {
                frame.convert(PixelFormat::Luma)
            }
            PixelFormat::Hsv => frame.convert(PixelFormat::Rgb),
            PixelFormat::Hsva => frame.convert(PixelFormat::Rgba),
            PixelFormat::YCbCrA => frame.convert(PixelFormat::YCbCr),
            _ => frame.borrow(),
        };
        let id = context.context.request::<PipelineId>();
        let name = context.context.request::<PipelineName>().map(|n| n.0);
        {
            let read_lock = self
                .running
                .read()
                .inspect_err(|_| warn!("poisoned FFMPEG component lock"))
                .unwrap_or_else(PoisonError::into_inner);
            if let Some((child, stdin)) = read_lock.get(&id) {
                let Ok(mut lock) = child.lock() else {
                    error!("poisoned FFMPEG child lock");
                    return;
                };
                match lock.try_wait() {
                    Ok(Some(status)) => {
                        if status.success() {
                            warn!("child process exited successfully but unexpectedly");
                        } else {
                            #[cfg(unix)]
                            {
                                use std::os::unix::process::ExitStatusExt;
                                error!(
                                    code = status.code(),
                                    signal = status.signal(),
                                    "child process exited with an error"
                                );
                            }
                            #[cfg(not(unix))]
                            error!(code = status.code(), "child process exited with an error");
                        }
                    }
                    Ok(None) => {
                        drop(lock);
                        if let Err(err) = stdin.lock().unwrap().write_all(&converted.data) {
                            error!(%err, "error writing data to stream");
                        }
                    }
                    Err(err) => {
                        error!(%err, "failed to get child status");
                    }
                }
                return;
            }
        }
        let mut cmd = Command::new(&*self.ffmpeg);
        Self::prep_command(&mut cmd, frame.borrow(), self.framerate);
        Self::format_args(&mut cmd, &self.args, id, name);
        match cmd
            .stdin(Stdio::piped())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
        {
            Ok(mut child) => {
                info!(cmd = ?DebugCommand(&cmd), pid = child.id(), "spawning new FFMPEG process");
                let mut stdin = child.stdin.take().unwrap();
                if let Err(err) = stdin.write_all(&converted.data) {
                    error!(%err, "error writing data to stream");
                }
                let mut lock = self
                    .running
                    .write()
                    .inspect_err(|_| warn!("poisoned FFMPEG component lock"))
                    .unwrap_or_else(PoisonError::into_inner);
                if let Some((mut child, _)) =
                    lock.insert(id, (Mutex::new(child), Mutex::new(stdin)))
                {
                    let child = child
                        .get_mut()
                        .inspect_err(|_| warn!("poisoned FFMPEG child lock"))
                        .unwrap_or_else(PoisonError::into_inner);
                    warn!(
                        ?id,
                        name = name.map(tracing::field::display),
                        pid = child.id(),
                        "duplicate running process"
                    );
                    if let Err(err) = child.kill() {
                        error!(%err, id = child.id(), "failed to kill child");
                    }
                }
            }
            Err(err) => {
                error!(%err, cmd = ?DebugCommand(&cmd), "failed to spawn a child process");
            }
        }
    }
}
impl Drop for FfmpegComponent {
    fn drop(&mut self) {
        for (_, (mut child, _)) in self
            .running
            .get_mut()
            .inspect_err(|_| warn!("poisoned FFMPEG component lock"))
            .unwrap_or_else(PoisonError::into_inner)
            .drain()
        {
            match child
                .get_mut()
                .inspect_err(|_| warn!("poisoned FFMPEG child lock"))
                .unwrap_or_else(PoisonError::into_inner)
                .wait()
            {
                Ok(status) => {
                    if !status.success() {
                        #[cfg(unix)]
                        {
                            use std::os::unix::process::ExitStatusExt;
                            warn!(
                                code = status.code(),
                                signal = status.signal(),
                                "child process exited with an error"
                            );
                        }
                        #[cfg(not(unix))]
                        warn!(code = status.code(), "child process exited with an error");
                    }
                }
                Err(err) => {
                    error!(%err, "failed to kill child process");
                }
            }
        }
    }
}

struct DebugCommand<'a>(&'a Command);
impl Debug for DebugCommand<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_list()
            .entry(&self.0.get_program())
            .entries(self.0.get_args())
            .finish()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "FfmpegShim")]
pub struct FfmpegFactory {
    /// The target framerate to use to encode video.
    pub fps: f64,
    /// Arguments to be passed to `ffmpeg` for the output format.
    ///
    /// This accepts `strftime` format for time, along with `%i` for the pipeline ID and `%N` for the pipeline name.
    pub args: Vec<String>,
    /// The path to `ffmpeg`. Defaults to just `ffmpeg` if not specified.
    pub ffmpeg: Cow<'static, str>,
}

#[derive(Deserialize)]
struct FfmpegShim {
    fps: f64,
    args: Vec<String>,
    ffmpeg: Option<String>,
}
impl TryFrom<FfmpegShim> for FfmpegFactory {
    type Error = time::error::InvalidFormatDescription;

    fn try_from(value: FfmpegShim) -> Result<Self, Self::Error> {
        FfmpegComponent::validate_args(&value.args)?;
        Ok(FfmpegFactory {
            fps: value.fps,
            args: value.args,
            ffmpeg: value.ffmpeg.map_or(Cow::Borrowed("ffmpeg"), Cow::Owned),
        })
    }
}
#[typetag::serde(name = "ffmpeg")]
impl ComponentFactory for FfmpegFactory {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(FfmpegComponent::new(
            self.ffmpeg.clone(),
            self.args.clone(),
            self.fps,
        ))
    }
}
