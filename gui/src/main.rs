use eframe::{App, CreationContext, egui};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::io;
use std::path::PathBuf;
use tracing::error;
use v4l::Device;
use viking_vision::camera::Camera;
use viking_vision::camera::capture::CaptureCamera;
use viking_vision::camera::config::{BasicConfig, CameraConfig};
use viking_vision::camera::frame::{Color, FrameCameraConfig};
use viking_vision::pipeline::daemon::DaemonHandle;

mod camera;

fn open_from_v4l_path(
    cameras: &mut Vec<(String, DaemonHandle<camera::Context>, camera::State)>,
) -> impl FnMut(&PathBuf) -> bool {
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
                    cameras.push((
                        name,
                        handle,
                        camera::State {
                            ident: camera::Ident::V4l(path.clone()),
                        },
                    ));
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
fn open_from_img_path(
    cameras: &mut Vec<(String, DaemonHandle<camera::Context>, camera::State)>,
) -> impl FnMut(&PathBuf) -> bool {
    move |path| {
        let res = FrameCameraConfig {
            basic: BasicConfig {
                width: 0,
                height: 0,
                fov: None,
                max_fps: Some(60.0),
            },
            source: viking_vision::camera::frame::ImageSource::Path(path.clone()),
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
                    cameras.push((
                        name,
                        handle,
                        camera::State {
                            ident: camera::Ident::Img(path.clone()),
                        },
                    ));
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
}
fn open_from_mono(
    mono: &Monochrome,
) -> Option<(String, DaemonHandle<camera::Context>, camera::State)> {
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
        (
            format!("Monochrome {id}"),
            handle,
            camera::State {
                ident: camera::Ident::Mono(mono.id),
            },
        )
    })
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Monochrome {
    width: u32,
    height: u32,
    color: [u8; 3],
    id: usize,
}

#[derive(Debug)]
struct VikingVision {
    open_caps: Vec<PathBuf>,
    open_imgs: Vec<PathBuf>,
    monochrome: Vec<Monochrome>,
    cameras: Vec<(String, DaemonHandle<camera::Context>, camera::State)>,
    text_buffers: Vec<(String, String, usize)>,
    buffer_id: usize,
    mono_count: usize,
    image_path_wip: String,
}
impl VikingVision {
    fn new(ctx: &CreationContext) -> io::Result<Self> {
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
            open_caps,
            open_imgs,
            monochrome,
            cameras,
            text_buffers,
            buffer_id,
            mono_count,
            image_path_wip: String::new(),
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
            let image = ui.button("Load Image");
            let image_id = egui::Id::new("image-open");
            if image.clicked() {
                ui.memory_mut(|mem| mem.open_popup(image_id));
            }
            egui::popup_above_or_below_widget(
                ui,
                image_id,
                &image,
                egui::AboveOrBelow::Below,
                egui::PopupCloseBehavior::CloseOnClickOutside,
                |ui| {
                    ui.set_min_width(100.0);
                    ui.horizontal(|ui| {
                        ui.text_edit_singleline(&mut self.image_path_wip);
                        if ui.button("Open").clicked() {
                            let path = PathBuf::from(std::mem::take(&mut self.image_path_wip));
                            open_from_img_path(&mut self.cameras)(&path);
                        }
                    })
                },
            );
        });
        self.cameras.retain_mut(|(name, handle, state)| {
            egui::Window::new(format!("{name}- Image"))
                .id(egui::Id::new((&**handle.context() as *const _, 1)))
                .show(ctx, camera::show_image(handle));
            let m = egui::Window::new(format!("{name}- Controls"))
                .id(egui::Id::new((&**handle.context() as *const _, 2)))
                .show(ctx, camera::show_controls(handle, state))
                .and_then(|o| o.inner.flatten());
            if let Some(m) = m {
                for m2 in &mut self.monochrome {
                    if m2.id == m.id {
                        *m2 = m;
                    }
                }
            }
            if handle.is_finished() {
                match &state.ident {
                    camera::Ident::V4l(path) => self.open_caps.retain(|p| p != path),
                    camera::Ident::Img(path) => self.open_imgs.retain(|p| p != path),
                    camera::Ident::Mono(ident) => self.monochrome.retain(|m| m.id != *ident),
                }
            }
            !handle.is_finished()
        });
        self.text_buffers.retain_mut(|(title, body, id)| {
            let res = egui::Window::new(&*title)
                .id(egui::Id::new(("buffer", *id)))
                .show(ctx, |ui| {
                    let mut keep = true;
                    ui.horizontal(|ui| {
                        let rename = ui.button("Rename");
                        let rename_id = egui::Id::new(("buffer-rename", *id));
                        if rename.clicked() {
                            ui.memory_mut(|mem| mem.open_popup(rename_id));
                        }
                        if ui.button("Delete").clicked() {
                            keep = false;
                        }
                        egui::popup_above_or_below_widget(
                            ui,
                            rename_id,
                            &rename,
                            egui::AboveOrBelow::Below,
                            egui::PopupCloseBehavior::CloseOnClickOutside,
                            |ui| {
                                ui.set_min_width(100.0);
                                ui.text_edit_singleline(title);
                            },
                        );
                    });
                    ui.code_editor(body);
                    keep
                });
            res.is_none_or(|res| res.inner.unwrap_or(true))
        });
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
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
    fn persist_egui_memory(&self) -> bool {
        false // window IDs don't persist
    }
}

fn main() {
    tracing_subscriber::fmt().init();
    let res = eframe::run_native(
        "VikingVision",
        Default::default(),
        Box::new(VikingVision::new_boxed),
    );
    if let Err(err) = res {
        tracing::error!(%err, "error in app");
        std::process::exit(101);
    }
}
