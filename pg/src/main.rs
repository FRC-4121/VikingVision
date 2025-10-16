use eframe::{App, CreationContext, egui};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::io;
use std::path::{Path, PathBuf};
use std::pin::Pin;
use tracing::error;
#[cfg(feature = "v4l")]
use v4l::Device;
use viking_vision::camera::Camera;
#[cfg(feature = "v4l")]
use viking_vision::camera::capture::CaptureCamera;
use viking_vision::camera::config::{BasicConfig, CameraConfig};
use viking_vision::camera::frame::{Color, FrameCameraConfig};
use viking_vision::pipeline::daemon::DaemonHandle;

mod camera;

#[cfg(feature = "v4l")]
fn open_from_v4l_path(cameras: &mut Vec<CameraData>) -> impl FnMut(&PathBuf) -> bool {
    move |path| {
        let res = Device::with_path(path).and_then(CaptureCamera::from_device);
        match res {
            Ok(inner) => {
                let mut name = camera::dev_name(path).unwrap_or_else(|| "<unknown>".to_string());
                if let Some(index) = camera::path_index(path) {
                    use std::fmt::Write;
                    let _ = write!(name, " (ID {index})");
                }
                let res = DaemonHandle::new(
                    Default::default(),
                    camera::CameraWorker::new(Camera::new(name.clone(), Box::new(inner))),
                );
                if let Ok(handle) = res {
                    cameras.push(CameraData {
                        name,
                        handle,
                        egui_id: egui::Id::new(("v4l", path)),
                        state: camera::State {
                            ident: camera::Ident::V4l(path.clone()),
                        },
                    });
                    true
                } else {
                    false
                }
            }
            Err(err) => {
                error!(%err, "failed to open camera");
                false
            }
        }
    }
}
fn open_from_img_path<P: AsRef<Path> + ?Sized>(
    cameras: &mut Vec<CameraData>,
) -> impl FnMut(&P) -> bool {
    fn inner(cameras: &mut Vec<CameraData>, path: &Path) -> bool {
        let res = FrameCameraConfig {
            basic: BasicConfig {
                width: 0,
                height: 0,
                fov: None,
                max_fps: Some(60.0),
            },
            source: viking_vision::camera::frame::ImageSource::Path(path.to_path_buf()),
        }
        .build_camera();
        let name = path.display().to_string();
        match res {
            Ok(inner) => {
                let res = DaemonHandle::new(
                    Default::default(),
                    camera::CameraWorker::new(Camera::new(name.clone(), inner)),
                );
                if let Ok(handle) = res {
                    cameras.push(CameraData {
                        name,
                        handle,
                        egui_id: egui::Id::new(path),
                        state: camera::State {
                            ident: camera::Ident::Img(path.to_path_buf()),
                        },
                    });
                    true
                } else {
                    false
                }
            }
            Err(err) => {
                error!(%err, "error loading image file");
                false
            }
        }
    }
    move |path| inner(cameras, path.as_ref())
}
fn open_from_mono(mono: &Monochrome) -> Option<CameraData> {
    let id = mono.id;
    let inner = FrameCameraConfig {
        basic: BasicConfig {
            width: mono.width,
            height: mono.height,
            fov: None,
            max_fps: Some(60.0),
        },
        source: viking_vision::camera::frame::ImageSource::Color(Color {
            format: viking_vision::buffer::PixelFormat::Rgb,
            bytes: mono.color.to_vec(),
        }),
    }
    .build_camera()
    .unwrap();
    let res = DaemonHandle::new(
        Default::default(),
        camera::CameraWorker::new(Camera::new(format!("Monochrome {id}"), inner)),
    );
    res.ok().map(|handle| {
        handle.start();
        CameraData {
            name: format!("Monochrome {id}"),
            handle,
            egui_id: egui::Id::new(("monochrome", id)),
            state: camera::State {
                ident: camera::Ident::Mono(mono.id),
            },
        }
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Monochrome {
    width: u32,
    height: u32,
    color: [u8; 3],
    id: usize,
}

struct CameraData {
    name: String,
    handle: DaemonHandle<camera::Context>,
    egui_id: egui::Id,
    state: camera::State,
}

struct VikingVision {
    #[cfg(feature = "v4l")]
    open_caps: Vec<PathBuf>,
    open_imgs: Vec<PathBuf>,
    monochrome: Vec<Monochrome>,
    cameras: Vec<CameraData>,
    text_buffers: Vec<(String, String, usize)>,
    buffer_id: usize,
    mono_count: usize,
    image_pick_future: Option<Pin<Box<dyn Future<Output = Option<rfd::FileHandle>>>>>,
}
impl VikingVision {
    fn new(ctx: &CreationContext) -> io::Result<Self> {
        #[cfg(feature = "v4l")]
        let mut open_caps = ctx
            .storage
            .and_then(|s| s.get_string("open_caps"))
            .map_or_else(Vec::new, |s| {
                s.split('\0')
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .collect()
            });
        let mut open_imgs = ctx
            .storage
            .and_then(|s| s.get_string("open_imgs"))
            .map_or_else(Vec::new, |s| {
                s.split('\0')
                    .filter(|s| !s.is_empty())
                    .map(PathBuf::from)
                    .collect()
            });
        let monochrome: Vec<Monochrome> = ctx
            .storage
            .and_then(|s| s.get_string("monochrome"))
            .and_then(|s| ron::from_str(&s).ok())
            .unwrap_or_default();
        let mut cameras = monochrome.iter().filter_map(open_from_mono).collect();
        let mut mono_count = 0;
        while monochrome.iter().any(|m| m.id == mono_count) {
            mono_count += 1; // I don't care enough to do this right
        }
        #[cfg(feature = "v4l")]
        open_caps.retain(open_from_v4l_path(&mut cameras));
        open_imgs.retain(open_from_img_path(&mut cameras));
        let text_buffers = ctx
            .storage
            .and_then(|s| s.get_string("text_buffers"))
            .and_then(|s| ron::from_str(&s).ok())
            .unwrap_or_default();
        let buffer_id = ctx
            .storage
            .and_then(|s| s.get_string("buffer_id"))
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();
        Ok(Self {
            #[cfg(feature = "v4l")]
            open_caps,
            open_imgs,
            monochrome,
            cameras,
            text_buffers,
            buffer_id,
            mono_count,
            image_pick_future: None,
        })
    }
    fn new_boxed(ctx: &CreationContext) -> Result<Box<dyn App>, Box<dyn Error + Send + Sync>> {
        Self::new(ctx)
            .map(|a| Box::new(a) as _)
            .map_err(|e| Box::new(e) as _)
    }
}
impl App for VikingVision {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::Window::new("Egui Debug")
            .default_open(false)
            .show(ctx, |ui| {
                ui.collapsing("Settings", |ui| ctx.settings_ui(ui));
                ui.collapsing("Inspection", |ui| ctx.inspection_ui(ui));
                ui.collapsing("Textures", |ui| ctx.texture_ui(ui));
                ui.collapsing("Memory", |ui| ctx.memory_ui(ui));
            });
        #[cfg(feature = "v4l")]
        {
            let mut i = self.open_caps.len();
            egui::Window::new("V4L Cameras").show(ctx, camera::show_cams(&mut self.open_caps));
            while i < self.open_caps.len() {
                if open_from_v4l_path(&mut self.cameras)(&self.open_caps[i]) {
                    i += 1;
                } else {
                    self.open_caps.swap_remove(i);
                }
            }
        }
        egui::Window::new("Utilities").show(ctx, |ui| {
            if ui.button("Text Buffer").clicked() {
                self.text_buffers
                    .push(("New Buffer".to_string(), String::new(), self.buffer_id));
                self.buffer_id += 1;
            }
            if ui.button("Monochrome").clicked() {
                let mono = Monochrome {
                    width: 256,
                    height: 256,
                    color: [0; 3],
                    id: self.mono_count,
                };
                self.mono_count += 1;
                while self.monochrome.iter().any(|m| m.id == self.mono_count) {
                    self.mono_count += 1; // I don't care enough to do this right
                }
                if let Some(entry) = open_from_mono(&mono) {
                    self.cameras.push(entry);
                }
                self.monochrome.push(mono);
            }
            if ui.button("Load Image").clicked() {
                self.image_pick_future = Some(Box::pin(
                    rfd::AsyncFileDialog::new()
                        .add_filter("Images", &["png", "jpg", "jpeg"])
                        .pick_file(),
                ));
            }
        });
        self.cameras.retain_mut(|data| {
            let m = egui::Window::new(&data.name)
                .id(data.egui_id)
                .show(ctx, camera::show_camera(&data.handle, &mut data.state))
                .and_then(|o| o.inner.flatten());
            if let Some(m) = m {
                for m2 in &mut self.monochrome {
                    if m2.id == m.id {
                        *m2 = m;
                    }
                }
            }
            if data.handle.is_finished() {
                match &data.state.ident {
                    #[cfg(feature = "v4l")]
                    camera::Ident::V4l(path) => self.open_caps.retain(|p| p != path),
                    camera::Ident::Img(path) => self.open_imgs.retain(|p| p != path),
                    camera::Ident::Mono(ident) => self.monochrome.retain(|m| m.id != *ident),
                }
            }
            !data.handle.is_finished()
        });
        self.text_buffers.retain_mut(|(title, body, id)| {
            let res = egui::Window::new(&*title)
                .id(egui::Id::new(("buffer", *id)))
                .show(ctx, |ui| {
                    let mut keep = true;
                    ui.horizontal(|ui| {
                        let rename = ui.button("Rename");
                        if ui.button("Delete").clicked() {
                            keep = false;
                        }
                        egui::Popup::menu(&rename)
                            .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                            .show(|ui| {
                                ui.set_min_width(100.0);
                                ui.text_edit_singleline(title);
                            });
                    });
                    ui.code_editor(body);
                    keep
                });
            res.is_none_or(|res| res.inner.unwrap_or(true))
        });
        if let Some(fut) = self.image_pick_future.as_mut() {
            use std::task::*;
            if let Poll::Ready(opt) = fut.as_mut().poll(&mut Context::from_waker(Waker::noop())) {
                self.image_pick_future = None;
                if let Some(handle) = opt {
                    open_from_img_path(&mut self.cameras)(handle.path());
                }
            }
        }
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        #[cfg(feature = "v4l")]
        storage.set_string(
            "open_caps",
            self.open_caps.iter().filter_map(|c| c.to_str()).fold(
                String::new(),
                |mut accum, path| {
                    if !accum.is_empty() {
                        accum.push('\0');
                    }
                    accum.push_str(path);
                    accum
                },
            ),
        );
        storage.set_string(
            "open_imgs",
            self.open_imgs.iter().filter_map(|c| c.to_str()).fold(
                String::new(),
                |mut accum, path| {
                    if !accum.is_empty() {
                        accum.push('\0');
                    }
                    accum.push_str(path);
                    accum
                },
            ),
        );
        if let Ok(s) = ron::to_string(&self.monochrome) {
            storage.set_string("monochrome", s);
        }
        if let Ok(s) = ron::to_string(&self.text_buffers) {
            storage.set_string("text_buffers", s);
        }
        storage.set_string("buffer_id", self.buffer_id.to_string());
    }
}

fn main() {
    tracing_subscriber::fmt().init();
    let res = eframe::run_native(
        "VikingVision Playground",
        Default::default(),
        Box::new(VikingVision::new_boxed),
    );
    if let Err(err) = res {
        tracing::error!(%err, "error in app");
        std::process::exit(101);
    }
}
