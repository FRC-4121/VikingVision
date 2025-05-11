use eframe::{App, CreationContext, egui};
use std::error::Error;
use std::io;
use std::path::PathBuf;
use std::sync::Mutex;
use tracing::error;
use v4l::FourCC;
use viking_vision::buffer::Buffer;
use viking_vision::camera::Camera;
use viking_vision::camera::capture::{CaptureCameraConfig, V4lPath};
use viking_vision::camera::config::{BasicConfig, Config};
use viking_vision::pipeline::daemon::DaemonHandle;

mod camera;

fn open_from_path(
    cameras: &mut Vec<(String, DaemonHandle<Mutex<Buffer<'static>>>)>,
) -> impl FnMut(&PathBuf) -> bool {
    move |path| {
        let res = CaptureCameraConfig {
            basic: BasicConfig {
                width: 640,
                height: 480,
                fov: None,
                max_fps: None,
            },
            fourcc: FourCC::new(b"YUYV"),
            source: std::sync::Arc::new(V4lPath(path.clone())),
        }
        .build_camera();
        match res {
            Ok(inner) => {
                let name = camera::dev_name(path).unwrap_or_else(|| "<unknown>".to_string());
                let res = DaemonHandle::new(
                    Default::default(),
                    camera::CameraWorker::new(Camera::new(name.clone(), inner)),
                );
                if let Ok(handle) = res {
                    cameras.push((name, handle));
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

#[derive(Debug)]
struct VikingVision {
    open_caps: Vec<PathBuf>,
    cameras: Vec<(String, DaemonHandle<Mutex<Buffer<'static>>>)>,
}
impl VikingVision {
    fn new(ctx: &CreationContext) -> io::Result<Self> {
        let mut open_caps = ctx
            .storage
            .and_then(|s| s.get_string("open_caps"))
            .map_or_else(Vec::new, |s| s.split('\0').map(PathBuf::from).collect());
        let mut cameras = Vec::new();
        open_caps.retain(open_from_path(&mut cameras));
        Ok(Self { open_caps, cameras })
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
            egui::Window::new("Cameras").show(ctx, camera::show_cams(&mut self.open_caps));
            while i < self.open_caps.len() {
                if open_from_path(&mut self.cameras)(&self.open_caps[i]) {
                    i += 1;
                } else {
                    self.open_caps.swap_remove(i);
                }
            }
        }
        self.cameras.retain(|(name, handle)| {
            egui::Window::new(name)
                .id(egui::Id::new(handle as *const _))
                .show(ctx, |ui| {
                    use viking_vision::pipeline::daemon::states::*;
                    let state = handle
                        .context()
                        .run_state
                        .load(std::sync::atomic::Ordering::Relaxed);
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
                    });
                    let buffer = handle.context().context.lock().unwrap();
                    let img = egui::ColorImage::from_rgb(
                        [buffer.width as _, buffer.height as _],
                        &buffer.data,
                    );
                    drop(buffer);
                    let texture =
                        ui.ctx()
                            .load_texture(format!("{handle:p}"), img, Default::default());
                    ui.image(&texture);
                });
            !handle.is_finished()
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
    }
    fn on_exit(&mut self, _gl: Option<&eframe::glow::Context>) {}
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
