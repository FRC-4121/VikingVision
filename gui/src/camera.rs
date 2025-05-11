use eframe::egui;
use egui_extras::{Column, TableBuilder};
use std::cell::Cell;
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tracing::error;
use viking_vision::buffer::Buffer;
use viking_vision::camera::Camera;
use viking_vision::pipeline::daemon::Worker;

fn enum_cams() -> Vec<PathBuf> {
    v4l::context::enum_devices()
        .iter()
        .map(|n| n.path().to_path_buf())
        .collect()
}

fn path_index(path: &Path) -> Option<usize> {
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

pub fn show_cams(devs: &mut Vec<PathBuf>) -> impl FnMut(&mut egui::Ui) {
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

#[derive(Debug)]
pub struct CameraWorker {
    pub camera: Result<Camera, Cell<bool>>,
}
impl CameraWorker {
    pub const fn new(camera: Camera) -> Self {
        Self { camera: Ok(camera) }
    }
}
impl Worker<Mutex<Buffer<'static>>> for CameraWorker {
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
    fn work(&mut self, context: &Mutex<Buffer<'static>>) {
        match &mut self.camera {
            Ok(camera) => {
                let Ok(frame) = camera.read() else { return };
                let Ok(mut buffer) = context.lock() else {
                    return;
                };
                frame.convert_into(&mut buffer);
            }
            Err(reported) => {
                if !std::mem::replace(reported.get_mut(), true) {
                    error!("attempted to use a camera worker that has already been shut down");
                }
            }
        }
    }
    fn cleanup(&mut self, _context: &Mutex<Buffer<'static>>) {
        if let Err(reported) = &mut self.camera {
            if !std::mem::replace(reported.get_mut(), true) {
                error!("attempted to use a camera worker that has already been shut down");
            }
        } else {
            self.camera = Err(Cell::new(false));
        }
    }
}
