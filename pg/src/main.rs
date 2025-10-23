use eframe::{App, CreationContext, egui};
use serde::{Deserialize, Serialize};
use std::error::Error;
use std::io;
use std::pin::Pin;

mod camera;
mod derived;
mod range_slider;

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
struct Monochrome {
    width: u32,
    height: u32,
    color: [u8; 3],
    id: usize,
}

struct VikingVision {
    monochrome: Vec<Monochrome>,
    cameras: Vec<camera::CameraData>,
    text_buffers: Vec<(String, String, usize)>,
    buffer_id: usize,
    mono_count: usize,
    image_pick_future: Option<Pin<Box<dyn Future<Output = Option<rfd::FileHandle>>>>>,
}
impl VikingVision {
    fn new(ctx: &CreationContext) -> io::Result<Self> {
        let monochrome: Vec<Monochrome> = ctx
            .storage
            .and_then(|s| s.get_string("monochrome"))
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let cameras = ctx
            .storage
            .and_then(|s| s.get_string("cameras"))
            .and_then(|s| serde_json::from_str::<Vec<_>>(&s).ok())
            .unwrap_or_default()
            .into_iter()
            .filter_map(camera::convert(&monochrome))
            .collect::<Vec<_>>();
        let mut mono_count = 0;
        while monochrome.iter().any(|m| m.id == mono_count) {
            mono_count += 1; // I don't care enough to do this right
        }
        let text_buffers = ctx
            .storage
            .and_then(|s| s.get_string("text_buffers"))
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        let buffer_id = ctx
            .storage
            .and_then(|s| s.get_string("buffer_id"))
            .and_then(|s| s.parse().ok())
            .unwrap_or_default();
        Ok(Self {
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
        #[cfg(feature = "v4l")]
        egui::Window::new("V4L Cameras").show(ctx, camera::show_cams(&mut self.cameras));
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
                if let Some(entry) = camera::open_from_mono(&mono) {
                    self.cameras.push(entry);
                    self.monochrome.push(mono);
                }
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
            egui::Window::new(&data.name)
                .id(data.egui_id)
                .show(ctx, camera::show_camera(data, &mut self.monochrome));
            data.handle
                .context()
                .context
                .locked
                .lock()
                .unwrap()
                .tree
                .retain_mut(derived::render_frame(ctx, &data.name));
            let finished = data.handle.is_finished();
            if finished && let camera::Ident::Mono(id) = data.state.ident {
                self.monochrome.retain(|m| m.id != id);
            }
            !finished
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
                    if let Some(cam) = camera::open_from_img_path(handle.path().to_path_buf()) {
                        self.cameras.push(cam);
                    }
                }
            }
        }
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if let Ok(s) = serde_json::to_string(&self.cameras) {
            storage.set_string("cameras", s);
        }
        if let Ok(s) = serde_json::to_string(&self.monochrome) {
            storage.set_string("monochrome", s);
        }
        if let Ok(s) = serde_json::to_string(&self.text_buffers) {
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
