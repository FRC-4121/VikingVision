use eframe::egui;
use egui_extras::{Column, TableBuilder};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use std::sync::atomic::{AtomicUsize, Ordering};
use tracing::error;
use v4l::FourCC;
use v4l::video::Capture;
use viking_vision::broadcast::par_broadcast1;
use viking_vision::buffer::{Buffer, PixelFormat};
use viking_vision::camera::Camera;
use viking_vision::camera::capture::CaptureCamera;
use viking_vision::camera::frame::{Color, FrameCamera, ImageSource};
use viking_vision::pipeline::daemon::{DaemonHandle, Worker};
use viking_vision::utils::FpsCounter;

fn enum_cams() -> Vec<PathBuf> {
    v4l::context::enum_devices()
        .iter()
        .map(|n| n.path().to_path_buf())
        .collect()
}

pub fn path_index(path: &Path) -> Option<usize> {
    let name = path.file_name()?.to_str()?;
    if let Ok(dev) = path.strip_prefix("/dev/") {
        if dev.parent().is_none_or(|p| p == Path::new("")) {
            if let Some(idx_str) = name.strip_prefix("video") {
                if let Ok(idx) = idx_str.parse() {
                    return Some(idx);
                }
            }
        }
    }
    None
}
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

pub fn show_cams(devs: &mut Vec<PathBuf>) -> impl FnOnce(&mut egui::Ui) {
    move |ui| {
        let refresh = ui.button("Refresh").clicked();
        let rows = ui.memory_mut(|mem| {
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
                    let node = &rows[row.index()];
                    row.col(|ui| {
                        ui.label(dev_name(node).unwrap_or_else(|| "unknown".to_string()));
                    });
                    row.col(|ui| {
                        ui.label(
                            path_index(node).map_or_else(|| "?".to_string(), |i| i.to_string()),
                        );
                    });
                    row.col(|ui| {
                        ui.code(node.display().to_string());
                    });
                    row.col(|ui| {
                        if ui.button("Add").clicked() {
                            devs.push(node.clone());
                        }
                    });
                });
            });
    }
}
pub fn show_image(handle: &DaemonHandle<Context>) -> impl FnOnce(&mut egui::Ui) {
    move |ui| {
        use viking_vision::pipeline::daemon::states::*;
        let state = handle.context().run_state.load(Ordering::Relaxed);
        let lock = handle.context().context.locked.lock().unwrap();
        ui.horizontal(|ui| {
            if state == SHUTDOWN {
                ui.label("Closing...");
                ui.spinner();
            } else {
                #[allow(clippy::collapsible_if)]
                if state == RUNNING {
                    if ui.button("Pause").clicked() {
                        handle.pause();
                    }
                } else if state == PAUSED {
                    if ui.button("Start").clicked() {
                        handle.start();
                    }
                }
                if ui.button("Close").clicked() {
                    handle.shutdown();
                }
            }
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                let [min, max] = lock.fps.minmax().unwrap_or_default();
                let avg = lock.fps.fps();
                ui.label(format!("{min:.2}/{max:.2}/{avg:.2} FPS"));
            });
        });
        let new_frames = handle.context().context.counter.swap(0, Ordering::Relaxed);
        if new_frames > 0 {
            ui.ctx().request_repaint();
        } else {
            ui.ctx().request_repaint_after_secs(0.05);
        }
        let buffer = &lock.frame;
        let img = egui::ColorImage::from_rgb([buffer.width as _, buffer.height as _], &buffer.data);
        let texture = ui
            .ctx()
            .load_texture(format!("{handle:p}"), img, Default::default());
        ui.image(&texture);
    }
}
pub fn show_controls(
    handle: &DaemonHandle<Context>,
    state: &mut State,
) -> impl FnOnce(&mut egui::Ui) -> Option<super::Monochrome> {
    move |ui| {
        let mut ret = None;
        let mut lock = handle.context().context.locked.lock().unwrap();
        if let Some(opts) = &mut lock.cap {
            ui.collapsing("V4L Options", |ui| {
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
                    egui::ComboBox::from_label("FourCC")
                        .selected_text(cc_desc)
                        .show_index(ui, &mut index, opts.formats.len(), |i| &opts.formats[i].1);
                    if index != cc_idx {
                        opts.selected_format = Some(index);
                    }
                }
                {
                    let mut index = opts.size_idx;
                    let [w, h] = opts.sizes[index];
                    egui::ComboBox::from_label("Resolution")
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
                    egui::ComboBox::from_label("Interval")
                        .selected_text(opts.intervals[index].to_string())
                        .show_index(ui, &mut index, opts.intervals.len(), |i| {
                            opts.intervals[i].to_string()
                        });
                    if index != opts.interval_idx {
                        opts.selected_interval = Some(index);
                    }
                }
            });
        }
        if let Some(opts) = &mut lock.mono {
            ui.collapsing("Monochrome", |ui| {
                if ui
                    .add(
                        egui::Slider::new(&mut opts.width, 0..=1000)
                            .clamping(egui::SliderClamping::Never)
                            .text("Width"),
                    )
                    .changed()
                {
                    opts.reshape = true;
                }
                if ui
                    .add(
                        egui::Slider::new(&mut opts.height, 0..=1000)
                            .clamping(egui::SliderClamping::Never)
                            .text("Height"),
                    )
                    .changed()
                {
                    opts.reshape = true;
                }
                if ui.color_edit_button_srgb(&mut opts.color).changed() {
                    opts.recolor = true;
                }
            });
            if opts.reshape || opts.recolor {
                if let Ident::Mono(id) = state.ident {
                    ret = Some(super::Monochrome {
                        width: opts.width,
                        height: opts.height,
                        color: opts.color,
                        id,
                    });
                }
            }
        }
        ret
    }
}

#[derive(Debug)]
pub enum Ident {
    V4l(PathBuf),
    Img(PathBuf),
    Mono(usize),
}

#[derive(Debug)]
pub struct State {
    pub ident: Ident,
}

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

#[derive(Debug, Default)]
pub struct LockedState {
    pub frame: Buffer<'static>,
    pub fps: FpsCounter,
    pub cap: Option<CapOptions>,
    pub mono: Option<MonochromeOptions>,
}

#[derive(Debug, Default)]
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
                state.fps.tick();
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
                        if changed {
                            if let Err(err) = capture.config_device() {
                                error!(%err, "failed to set format");
                            }
                        }
                    }
                } else {
                    state.cap = None;
                }
                if let Some(mono) = camera.downcast_mut::<FrameCamera>() {
                    if let ImageSource::Color(Color {
                        format: PixelFormat::Rgb,
                        ref bytes,
                    }) = mono.config.source
                    {
                        let opts = state.mono.get_or_insert_default();
                        if opts.recolor {
                            par_broadcast1(|c| *c = opts.color, &mut mono.buffer);
                        } else {
                            opts.color.copy_from_slice(bytes);
                        }
                        if opts.reshape {
                            let _ = mono.reshape_monochrome(opts.width, opts.height);
                        } else {
                            opts.width = mono.buffer.width;
                            opts.height = mono.buffer.height;
                        }
                    } else {
                        state.mono = None;
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
