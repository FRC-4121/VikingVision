use crate::Monochrome;
use crate::daemon::{DaemonHandle, Worker};
use eframe::egui;
#[cfg(feature = "v4l")]
use egui_extras::{Column, TableBuilder};
use serde::ser::SerializeStruct;
use serde::{Deserialize, Serialize};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::error;
#[cfg(feature = "v4l")]
use v4l::{Device, FourCC, video::Capture};
#[cfg(feature = "v4l")]
use vv_camera::capture::CaptureCamera;
use vv_camera::{Camera, CameraImpl};
use vv_utils::common_types::FrameSize;
use vv_utils::utils::FpsCounter;
use vv_vision::broadcast::par_broadcast1;
use vv_vision::buffer::{Buffer, PixelFormat};

struct MonoCamera {
    color: Option<[u8; 3]>,
    buffer: Buffer<'static>,
}
impl CameraImpl for MonoCamera {
    fn frame_size(&self) -> FrameSize {
        FrameSize {
            width: self.buffer.width,
            height: self.buffer.height,
        }
    }
    fn load_frame(&mut self) -> std::io::Result<()> {
        Ok(())
    }
    fn get_frame(&self) -> Buffer<'_> {
        self.buffer.borrow()
    }
}

#[cfg(feature = "v4l")]
fn enum_cams() -> Vec<PathBuf> {
    v4l::context::enum_devices()
        .iter()
        .map(|n| n.path().to_path_buf())
        .collect()
}

#[cfg(feature = "v4l")]
pub fn path_index(path: &Path) -> Option<usize> {
    let name = path.file_name()?.to_str()?;
    if let Ok(dev) = path.strip_prefix("/dev/")
        && dev.parent().is_none_or(|p| p == Path::new(""))
        && let Some(idx_str) = name.strip_prefix("video")
        && let Ok(idx) = idx_str.parse()
    {
        return Some(idx);
    }
    None
}

#[cfg(feature = "v4l")]
pub fn dev_name(path: &Path) -> Option<String> {
    let index = path_index(path)?;
    let mut buf = PathBuf::from("/sys/class/video4linux");
    buf.push(format!("video{index}"));
    buf.push("name");
    let mut name = String::from_utf8_lossy(&std::fs::read(buf).ok()?).into_owned();
    if name.ends_with('\n') {
        name.pop();
    }
    Some(name)
}

pub fn open_from_img_path(path: PathBuf) -> Option<CameraData> {
    spawn_from_img_path(&path).map(|handle| CameraData {
        name: path.display().to_string(),
        handle,
        egui_id: egui::Id::new(("img", &path)),
        state: State {
            ident: Ident::Img(path),
        },
    })
}
pub fn open_from_mono(mono: &Monochrome) -> Option<CameraData> {
    spawn_from_mono(mono).map(|handle| {
        let id = mono.id;
        CameraData {
            name: format!("Monochrome {id}"),
            handle,
            egui_id: egui::Id::new(("monochrome", id)),
            state: State {
                ident: Ident::Mono(mono.id),
            },
        }
    })
}

#[cfg(feature = "v4l")]
pub fn show_cams(cams: &mut Vec<CameraData>) -> impl FnOnce(&mut egui::Ui) {
    move |ui| {
        let refresh = ui.button("Refresh").clicked();
        let mut rows = ui.memory_mut(|mem| {
            let id = egui::Id::new("V4L nodes");
            if refresh {
                let rows = enum_cams();
                mem.data.insert_temp(id, rows.clone());
                rows
            } else {
                mem.data.get_temp_mut_or_insert_with(id, enum_cams).clone()
            }
        });
        let heading_height = ui.text_style_height(&egui::TextStyle::Heading);
        let row_height = ui.text_style_height(&egui::TextStyle::Body);
        TableBuilder::new(ui)
            .id_salt("cameras")
            .resizable(true)
            .column(Column::auto_with_initial_suggestion(200.0))
            .column(Column::auto_with_initial_suggestion(60.0))
            .column(Column::auto())
            .column(Column::exact(40.0))
            .header(heading_height, |mut row| {
                row.col(|ui| {
                    ui.heading("Name");
                });
                row.col(|ui| {
                    ui.heading("Index");
                });
                row.col(|ui| {
                    ui.heading("Path");
                });
            })
            .body(|body| {
                body.rows(row_height, rows.len(), |mut row| {
                    let path = &mut rows[row.index()];
                    row.col(|ui| {
                        ui.label(dev_name(path).unwrap_or_else(|| "unknown".to_string()));
                    });
                    row.col(|ui| {
                        ui.label(
                            path_index(path).map_or_else(|| "?".to_string(), |i| i.to_string()),
                        );
                    });
                    row.col(|ui| {
                        ui.code(path.display().to_string());
                    });
                    row.col(|ui| {
                        #[allow(clippy::collapsible_if)]
                        if ui.button("Add").clicked() {
                            if let Some(handle) = spawn_from_v4l_path(path) {
                                let mut name =
                                    dev_name(path).unwrap_or_else(|| "<unknown>".to_string());
                                if let Some(index) = path_index(path) {
                                    use std::fmt::Write;
                                    let _ = write!(name, " (ID {index})");
                                }

                                cams.push(CameraData {
                                    name,
                                    handle,
                                    egui_id: egui::Id::new(("v4l", &path)),
                                    state: State {
                                        ident: Ident::V4l(std::mem::take(path)),
                                    },
                                });
                            }
                        }
                    });
                });
            });
    }
}
pub fn show_camera(
    data: &CameraData,
    monochrome: &mut Vec<super::Monochrome>,
) -> impl FnOnce(&mut egui::Ui) {
    move |ui| {
        use crate::daemon::states::*;
        let run_state = data.handle.context().run_state.load(Ordering::Relaxed);
        let mut lock = data.handle.context().context.locked.lock().unwrap();
        ui.horizontal(|ui| {
            if run_state == SHUTDOWN {
                ui.label("Closing...");
                ui.spinner();
            } else {
                #[allow(clippy::collapsible_if)]
                if run_state == RUNNING {
                    if ui.button("Pause").clicked() {
                        data.handle.pause();
                    }
                } else if run_state == PAUSED {
                    if ui.button("Start").clicked() {
                        data.handle.start();
                    }
                }
                if ui.button("Close").clicked() {
                    data.handle.shutdown();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let [min, max] = lock.fps.minmax().unwrap_or_default();
                let avg = lock.fps.fps();
                ui.label(format!("{min:.2}/{max:.2}/{avg:.2} FPS"));
                crate::derived::add_button(ui, &data.name, data.egui_id, &mut lock.tree);
            });
        });
        {
            #[cfg(feature = "v4l")]
            let has_cap = lock.cap.is_some();
            #[cfg(not(feature = "v4l"))]
            let has_cap = false;
            if has_cap || lock.mono.is_some() {
                ui.horizontal(|ui| {
                    #[cfg(feature = "v4l")]
                    if let Some(opts) = &mut lock.cap {
                        {
                            let (cc_idx, cc_desc) = opts
                                .formats
                                .iter()
                                .enumerate()
                                .find_map(|(n, (fourcc, desc))| {
                                    (*fourcc == opts.fourcc).then_some((n, desc))
                                })
                                .unwrap();
                            let mut index = cc_idx;
                            ui.label("Format: ");
                            egui::ComboBox::new("Format", "")
                                .selected_text(cc_desc)
                                .show_index(ui, &mut index, opts.formats.len(), |i| {
                                    &opts.formats[i].1
                                });
                            if index != cc_idx {
                                opts.selected_format = Some(index);
                            }
                        }
                        {
                            let mut index = opts.size_idx;
                            let [w, h] = opts.sizes[index];
                            ui.label("Resolution: ");
                            egui::ComboBox::new("Resolution", "")
                                .selected_text(format!("{w}x{h}"))
                                .show_index(ui, &mut index, opts.sizes.len(), |i| {
                                    let [w, h] = opts.sizes[i];
                                    format!("{w}x{h}")
                                });
                            if index != opts.size_idx {
                                opts.selected_size = Some(index);
                            }
                        }
                        {
                            let mut index = opts.interval_idx;
                            ui.label("Interval: ");
                            egui::ComboBox::new("Interval", "")
                                .selected_text(opts.intervals[index].to_string())
                                .show_index(ui, &mut index, opts.intervals.len(), |i| {
                                    opts.intervals[i].to_string()
                                });
                            if index != opts.interval_idx {
                                opts.selected_interval = Some(index);
                            }
                        }
                    }
                    if let Some(opts) = &mut lock.mono {
                        ui.label("Width: ");
                        if ui
                            .add(
                                egui::Slider::new(&mut opts.width, 0..=1000)
                                    .clamping(egui::SliderClamping::Never),
                            )
                            .changed()
                        {
                            opts.reshape = true;
                        }
                        ui.label("Height: ");
                        if ui
                            .add(
                                egui::Slider::new(&mut opts.height, 0..=1000)
                                    .clamping(egui::SliderClamping::Never),
                            )
                            .changed()
                        {
                            opts.reshape = true;
                        }
                        ui.label("Color: ");
                        if ui.color_edit_button_srgb(&mut opts.color).changed() {
                            opts.recolor = true;
                        }
                        if (opts.reshape || opts.recolor)
                            && let Ident::Mono(id) = data.state.ident
                        {
                            for m in monochrome {
                                if m.id == id {
                                    m.width = opts.width;
                                    m.height = opts.height;
                                    m.color = opts.color;
                                }
                            }
                        }
                    }
                });
            }
        }
        let new_frames = data
            .handle
            .context()
            .context
            .counter
            .swap(0, Ordering::Relaxed);
        if new_frames > 0 {
            ui.ctx().request_repaint();
        } else {
            ui.ctx().request_repaint_after_secs(0.05);
        }
        let buffer = &lock.frame;
        let img = egui::ColorImage::from_rgb([buffer.width as _, buffer.height as _], &buffer.data);
        let texture = ui.ctx().load_texture(
            format!("{:p}", std::sync::Arc::as_ptr(data.handle.context())),
            img,
            Default::default(),
        );
        ui.image(&texture);
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub enum Ident {
    Img(PathBuf),
    Mono(usize),
    #[cfg(feature = "v4l")]
    V4l(PathBuf),
}

#[derive(Debug, Serialize, Deserialize)]
pub struct State {
    pub ident: Ident,
}

pub struct CameraData {
    pub name: String,
    pub handle: DaemonHandle<Context>,
    pub egui_id: egui::Id,
    pub state: State,
}

impl Serialize for CameraData {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        let mut s = serializer.serialize_struct("CameraData", 4)?;
        s.serialize_field("name", &self.name)?;
        s.serialize_field("egui_id", &self.egui_id)?;
        s.serialize_field("state", &self.state)?;
        let lock = self.handle.context().context.locked.lock().unwrap();
        s.serialize_field("tree", &lock.tree)?;
        s.end()
    }
}

#[derive(Deserialize)]
pub struct DeserializedData {
    name: String,
    egui_id: egui::Id,
    state: State,
    tree: Vec<crate::derived::DerivedFrame>,
}

#[cfg(feature = "v4l")]
fn spawn_from_v4l_path(path: &Path) -> Option<DaemonHandle<Context>> {
    Device::with_path(path)
        .and_then(CaptureCamera::from_device)
        .inspect_err(|err| error!(%err, "failed to open camera"))
        .ok()
        .and_then(|inner| {
            let mut name = dev_name(path).unwrap_or_else(|| "<unknown>".to_string());
            if let Some(index) = path_index(path) {
                use std::fmt::Write;
                let _ = write!(name, " (ID {index})");
            }
            let res = DaemonHandle::new(
                Default::default(),
                CameraWorker::new(Camera::new(name.clone(), Box::new(inner))),
            );
            res.ok()
        })
        .inspect(DaemonHandle::start)
}

fn spawn_from_img_path(path: &Path) -> Option<DaemonHandle<Context>> {
    let buf = std::fs::read(path)
        .inspect_err(|err| error!(%err, "failed to open file"))
        .ok()?;
    let buffer = Buffer::decode_img_data(&buf)
        .inspect_err(|err| error!(%err, "error decoding image file"))
        .ok()?;
    DaemonHandle::new(
        Default::default(),
        CameraWorker::new(Camera::new(
            path.display().to_string(),
            Box::new(MonoCamera {
                buffer,
                color: None,
            }),
        )),
    )
    .ok()
    .inspect(DaemonHandle::start)
}

fn spawn_from_mono(mono: &Monochrome) -> Option<DaemonHandle<Context>> {
    let id = mono.id;
    DaemonHandle::new(
        Default::default(),
        CameraWorker::new(Camera::new(
            format!("Monochrome {id}"),
            Box::new(MonoCamera {
                buffer: Buffer::monochrome(mono.width, mono.height, PixelFormat::RGB, &mono.color),
                color: Some(mono.color),
            }),
        )),
    )
    .ok()
    .inspect(DaemonHandle::start)
}

pub fn convert(mono: &[super::Monochrome]) -> impl Fn(DeserializedData) -> Option<CameraData> {
    move |deserialized| {
        let DeserializedData {
            name,
            egui_id,
            state,
            tree,
        } = deserialized;
        let handle = match &state.ident {
            #[cfg(feature = "v4l")]
            Ident::V4l(path) => spawn_from_v4l_path(path)?,
            Ident::Img(path) => spawn_from_img_path(path)?,
            Ident::Mono(id) => spawn_from_mono(mono.iter().find(|m| m.id == *id)?)?,
        };
        let mut lock = handle.context().context.locked.lock().unwrap();
        lock.tree = tree;
        drop(lock);
        Some(CameraData {
            name,
            handle,
            egui_id,
            state,
        })
    }
}

#[cfg(feature = "v4l")]
#[derive(Debug, Default)]
pub struct CapOptions {
    pub fourcc: FourCC,
    pub size_idx: usize,
    pub interval_idx: usize,
    pub formats: Vec<(FourCC, String)>,
    pub selected_format: Option<usize>,
    pub sizes: Vec<[u32; 2]>,
    pub selected_size: Option<usize>,
    pub intervals: Vec<v4l::Fraction>,
    pub selected_interval: Option<usize>,
}

#[derive(Debug, Default)]
pub struct MonochromeOptions {
    pub width: u32,
    pub height: u32,
    pub color: [u8; 3],
    pub reshape: bool,
    pub recolor: bool,
}

#[derive(Default)]
pub struct LockedState {
    pub frame: Buffer<'static>,
    pub fps: FpsCounter,
    #[cfg(feature = "v4l")]
    pub cap: Option<CapOptions>,
    pub mono: Option<MonochromeOptions>,
    pub tree: Vec<crate::derived::DerivedFrame>,
}

#[derive(Default)]
pub struct Context {
    pub locked: Mutex<LockedState>,
    pub counter: AtomicUsize,
}

#[derive(Debug)]
pub struct CameraWorker {
    pub camera: Result<Camera, Cell<bool>>,
}
impl CameraWorker {
    pub const fn new(camera: Camera) -> Self {
        Self { camera: Ok(camera) }
    }
}
impl Worker<Context> for CameraWorker {
    fn name(&self) -> String {
        match &self.camera {
            Ok(camera) => format!("{}-worker", camera.name()),
            Err(reported) => {
                if !reported.replace(true) {
                    error!("attempted to use a camera worker that has already been shut down");
                }
                "<error>-worker".to_string()
            }
        }
    }
    fn work(&mut self, context: &Context) {
        match &mut self.camera {
            Ok(camera) => {
                let Ok(frame) = camera.read() else { return };
                let Ok(mut state) = context.locked.lock() else {
                    return;
                };
                frame.convert_into(&mut state.frame);
                for next in &mut state.tree {
                    next.update_frame(frame.borrow(), frame.borrow());
                }
                state.fps.tick();
                #[cfg(feature = "v4l")]
                if let Some(capture) = camera.downcast_mut::<CaptureCamera>() {
                    let opts = state.cap.get_or_insert_default();
                    let dev = &capture.device;
                    'config: {
                        let Ok(formats) = dev
                            .enum_formats()
                            .inspect_err(|err| error!(%err, "failed to read available formats"))
                        else {
                            break 'config;
                        };
                        let mut changed = false;
                        if let Some(desc) =
                            opts.selected_format.take().and_then(|idx| formats.get(idx))
                        {
                            if desc.fourcc != capture.config.fourcc {
                                capture.set_fourcc(desc.fourcc);
                                changed = true;
                            }
                        } else {
                            opts.formats = formats
                                .into_iter()
                                .map(|f| (f.fourcc, f.description))
                                .collect();
                            if let Some(&[width, height]) = opts
                                .selected_size
                                .take()
                                .and_then(|idx| opts.sizes.get(idx))
                            {
                                capture.set_resolution(width, height);
                                changed = true;
                            } else {
                                let Ok(sizes) = dev.enum_framesizes(capture.fourcc()).inspect_err(
                                    |err| error!(%err, "failed to read available sizes"),
                                ) else {
                                    break 'config;
                                };
                                opts.sizes = sizes
                                    .into_iter()
                                    .flat_map(|s| s.size.to_discrete())
                                    .map(|s| [s.width, s.height])
                                    .collect::<Vec<_>>();
                                opts.size_idx = opts
                                    .sizes
                                    .iter()
                                    .position(|&[w, h]| {
                                        w == capture.width() && h == capture.height()
                                    })
                                    .unwrap();
                                if let Some(interval) = opts
                                    .selected_interval
                                    .take()
                                    .and_then(|idx| opts.intervals.get(idx))
                                {
                                    capture.set_interval(*interval);
                                    changed = true;
                                } else {
                                    let Ok(intervals) = dev
                                        .enum_frameintervals(
                                            capture.fourcc(),
                                            capture.width(),
                                            capture.height(),
                                        )
                                        .inspect_err(
                                            |err| error!(%err, "failed to read available intervals"),
                                        )
                                    else {
                                        break 'config;
                                    };
                                    opts.intervals = intervals
                                        .into_iter()
                                        .flat_map(|i| match i.interval {
                                            v4l::frameinterval::FrameIntervalEnum::Discrete(f) => {
                                                vec![f]
                                            }
                                            v4l::frameinterval::FrameIntervalEnum::Stepwise(s) => {
                                                vec![s.min, s.max]
                                            }
                                        })
                                        .collect();
                                    let Ok(v4l::Fraction {
                                        numerator: n,
                                        denominator: d,
                                    }) = capture.interval_mut().inspect_err(
                                        |err| error!(%err, "failed to get current interval"),
                                    )
                                    else {
                                        break 'config;
                                    };
                                    opts.interval_idx = opts
                                        .intervals
                                        .iter()
                                        .position(|i| i.numerator == n && i.denominator == d)
                                        .unwrap_or(0);
                                }
                            }
                        }
                        opts.fourcc = capture.fourcc();
                        if changed && let Err(err) = capture.config_device() {
                            error!(%err, "failed to set format");
                        }
                    }
                } else {
                    state.cap = None;
                }
                if let Some(MonoCamera {
                    color: Some(color),
                    buffer,
                }) = camera.downcast_mut::<MonoCamera>()
                {
                    let opts = state.mono.get_or_insert_default();
                    if opts.reshape {
                        opts.reshape = false;
                        buffer.width = opts.width;
                        buffer.height = opts.height;
                        opts.recolor = true;
                    } else {
                        opts.width = buffer.width;
                        opts.height = buffer.height;
                    }
                    if opts.recolor {
                        opts.recolor = false;
                        *color = opts.color;
                        par_broadcast1(|c: &mut [u8; 3]| *c = opts.color, buffer.resize_data());
                    } else {
                        opts.color = *color;
                    }
                } else {
                    state.mono = None;
                }
                context.counter.fetch_add(1, Ordering::Relaxed);
            }
            Err(reported) => {
                if !std::mem::replace(reported.get_mut(), true) {
                    error!("attempted to use a camera worker that has already been shut down");
                }
            }
        }
    }
    fn cleanup(&mut self, _context: &Context) {
        if let Err(reported) = &mut self.camera {
            if !std::mem::replace(reported.get_mut(), true) {
                error!("attempted to use a camera worker that has already been shut down");
            }
        } else {
            self.camera = Err(Cell::new(false));
        }
    }
}
