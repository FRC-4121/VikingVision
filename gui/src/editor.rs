use eframe::egui;
use std::fs::{File, OpenOptions};
use std::io::{self, prelude::*};
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};

pub type DocumentResult = Result<toml_edit::DocumentMut, toml_edit::TomlError>;

fn opts() -> OpenOptions {
    let mut opts = OpenOptions::new();
    opts.read(true).write(true).truncate(false);
    opts
}

pub struct EditorState {
    pub document: DocumentResult,
    contents: String,
    loaded: PathBuf,
    file: Option<File>,
    file_err: Option<io::Error>,
    save_fut: Option<Pin<Box<dyn Future<Output = Option<rfd::FileHandle>>>>>,
    open_fut: Option<Pin<Box<dyn Future<Output = Option<rfd::FileHandle>>>>>,
    saved: bool,
    contents_persisted: bool,
    path_persisted: bool,
    show_confirm_open: bool,
    last_saved: Option<time::Time>,
}
impl EditorState {
    pub fn load(storage: Option<&dyn eframe::Storage>) -> Self {
        let mut contents = String::new();
        let mut loaded = PathBuf::new();
        if let Some(storage) = storage {
            contents = storage.get_string("file_contents").unwrap_or_default();
            loaded = storage
                .get_string("file_path")
                .map_or(PathBuf::new(), PathBuf::from);
        }
        let mut file = None;
        let mut file_err = None;
        let mut saved = false;
        let mut last_saved = None;
        if !loaded.as_os_str().is_empty() {
            let f = opts().open(&loaded);
            match f {
                Ok(mut f) => {
                    let mut buf = [0u8; 256];
                    let mut expected = contents.as_bytes();
                    loop {
                        match f.read(&mut buf) {
                            Ok(0) => {
                                saved = expected.is_empty();
                                break;
                            }
                            Ok(len) => {
                                let Some(blk) = expected.split_off(..len) else {
                                    saved = false;
                                    break;
                                };
                                if *blk != buf[..len] {
                                    saved = false;
                                    break;
                                }
                            }
                            Err(err) => {
                                saved = false;
                                tracing::error!(%err, "error reading file");
                                break;
                            }
                        }
                    }
                    if saved {
                        last_saved = Some(now());
                    }
                    file = Some(f);
                }
                Err(err) => {
                    tracing::error!(%err, "error opening previous file");
                    file_err = Some(err);
                }
            }
        }
        let document = contents.parse();
        EditorState {
            document,
            contents,
            loaded,
            file,
            file_err,
            saved,
            save_fut: None,
            open_fut: None,
            contents_persisted: true,
            path_persisted: true,
            show_confirm_open: false,
            last_saved,
        }
    }
    pub fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if !self.contents_persisted {
            self.contents_persisted = true;
            storage.set_string("file_contents", self.contents.clone());
        }
        if !self.path_persisted {
            self.path_persisted = true;
            storage.set_string("file_path", self.loaded.display().to_string());
        }
    }
    pub fn render(&mut self, ui: &mut egui::Ui) {
        let mut cx = Context::from_waker(Waker::noop());
        if let Some(fut) = &mut self.save_fut
            && let Poll::Ready(handle) = fut.as_mut().poll(&mut cx)
        {
            self.save_fut = None;
            if let Some(handle) = handle {
                let path = handle.path();
                match opts().create(true).open(path) {
                    Ok(mut file) => {
                        if let Err(err) = write_to_file(&mut file, self.contents.as_bytes()) {
                            tracing::error!(%err, "failed to write to file");
                            self.loaded.clear();
                            self.file = None;
                            self.file_err = Some(err);
                            self.saved = false;
                            self.path_persisted = false;
                        } else {
                            self.loaded = path.to_path_buf();
                            self.file = Some(file);
                            self.file_err = None;
                            self.saved = true;
                            self.last_saved = Some(now());
                            self.path_persisted = false;
                        }
                    }
                    Err(err) => {
                        tracing::error!(%err, "failed to open file");
                        self.loaded.clear();
                        self.file = None;
                        self.file_err = Some(err);
                        self.path_persisted = false;
                    }
                }
            }
        }
        if let Some(fut) = &mut self.open_fut
            && let Poll::Ready(handle) = fut.as_mut().poll(&mut cx)
        {
            self.open_fut = None;
            if let Some(handle) = handle {
                let path = handle.path();
                match opts().open(path) {
                    Ok(mut file) => {
                        self.contents.clear();
                        if let Err(err) = file.read_to_string(&mut self.contents) {
                            tracing::error!(%err, "failed to read from file");
                            self.loaded.clear();
                            self.file = None;
                            self.file_err = Some(err);
                            self.path_persisted = false;
                        } else {
                            self.loaded = path.to_path_buf();
                            self.file = Some(file);
                            self.file_err = None;
                            self.saved = true;
                            self.last_saved = Some(now());
                            self.path_persisted = false;
                            self.contents_persisted = false;
                        }
                    }
                    Err(err) => {
                        tracing::error!(%err, "failed to open file");
                        self.loaded.clear();
                        self.file = None;
                        self.file_err = Some(err);
                        self.path_persisted = false;
                    }
                }
            }
        }
        ui.heading("Editor");
        if self.loaded.as_os_str().is_empty() {
            ui.label("No file open");
        } else {
            ui.label(format!("Opened: {}", self.loaded.display()));
        }
        ui.horizontal(|ui| {
            let open_button = ui.button("Open");
            let mut open_file = false;
            if open_button.clicked() {
                if self.saved || self.file.is_none() {
                    open_file = true;
                } else {
                    self.show_confirm_open = true;
                }
            }
            let mut close = false;
            egui::Popup::from_response(&open_button)
                .open_bool(&mut self.show_confirm_open)
                .show(|ui| {
                    ui.label("File not saved!");
                    if ui.button("Save").clicked() {
                        if let Some(file) = &mut self.file {
                            self.last_saved = Some(now());
                            self.saved = true;
                            if let Err(err) = write_to_file(file, self.contents.as_bytes()) {
                                tracing::error!(%err, "error saving to file");
                                self.file_err = Some(err);
                            }
                        }
                        open_file = true;
                        close = true;
                    }
                    if ui.button("Don't Save").clicked() {
                        open_file = true;
                        close = true;
                    }
                    if ui.button("Cancel").clicked() {
                        close = true;
                    }
                });
            if close {
                self.show_confirm_open = false;
            }
            if open_file {
                self.open_fut = Some(Box::pin(
                    rfd::AsyncFileDialog::new()
                        .add_filter("TOML", &["toml"])
                        .set_can_create_directories(true)
                        .pick_file(),
                ));
            }
            if ui.button("Save").clicked() {
                if let Some(file) = &mut self.file {
                    if let Err(err) = write_to_file(file, self.contents.as_bytes()) {
                        tracing::error!(%err, "error saving to file");
                        self.file_err = Some(err);
                    }
                    self.last_saved = Some(now());
                    self.saved = true;
                } else {
                    self.save_fut = Some(Box::pin(
                        rfd::AsyncFileDialog::new()
                            .add_filter("TOML", &["toml"])
                            .set_can_create_directories(true)
                            .save_file(),
                    ));
                }
            }
            if ui.button("Save As").clicked() {
                self.save_fut = Some(Box::pin(
                    rfd::AsyncFileDialog::new()
                        .add_filter("TOML", &["toml"])
                        .set_can_create_directories(true)
                        .save_file(),
                ));
            }
        });
        ui.label(format!("Saved: {}", self.saved));
        ui.label(self.last_saved.map_or_else(
            || "Last saved: never".to_string(),
            |time| format!("Last saved: {time}"),
        ));
        if let Some(err) = &self.file_err {
            let mut clear = false;
            egui::Frame::new()
                .fill(ui.style().visuals.extreme_bg_color)
                .corner_radius(4.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(err.to_string())
                            .monospace()
                            .color(ui.style().visuals.error_fg_color),
                    );
                    ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                        clear = ui.button("X").clicked();
                    })
                });
            if clear {
                self.file_err = None;
            }
        }
        if let Err(err) = &self.document {
            egui::Frame::new()
                .fill(ui.style().visuals.extreme_bg_color)
                .corner_radius(4.0)
                .show(ui, |ui| {
                    ui.label(
                        egui::RichText::new(err.to_string())
                            .monospace()
                            .color(ui.style().visuals.error_fg_color),
                    );
                });
        }
        if scrollable_text(&mut self.contents, ui).changed() {
            self.contents_persisted = false;
            self.saved = false;
            self.document = self.contents.parse();
        }
    }
}
fn write_to_file(file: &mut File, contents: &[u8]) -> io::Result<()> {
    file.seek(io::SeekFrom::Start(0))?;
    file.set_len(contents.len() as _)?;
    file.write_all(contents)
}
fn now() -> time::Time {
    time::OffsetDateTime::now_local()
        .ok()
        .unwrap_or_else(time::OffsetDateTime::now_utc)
        .time()
}
fn scrollable_text(buf: &mut String, ui: &mut egui::Ui) -> egui::Response {
    let available = ui.available_rect_before_wrap();
    let where_to_put_background = ui.painter().add(egui::Shape::Noop);
    let sao = egui::ScrollArea::both().show(ui, |ui| {
        ui.set_min_size(available.size());
        ui.set_clip_rect(available);
        ui.add_sized(
            available.size(),
            egui::TextEdit::multiline(buf).code_editor().frame(false),
        )
    });
    let visuals = ui.visuals();
    let widget = ui.style().interact(&sao.inner);
    let background = visuals.text_edit_bg_color();
    let stroke = if sao.inner.has_focus() {
        visuals.selection.stroke
    } else {
        widget.bg_stroke
    };
    let shape = egui::epaint::RectShape::new(
        available,
        widget.corner_radius,
        background,
        stroke,
        egui::StrokeKind::Inside,
    );
    ui.painter().set(where_to_put_background, shape);
    sao.inner
}
