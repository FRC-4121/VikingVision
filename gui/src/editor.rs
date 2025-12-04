use eframe::egui;
use std::fs::{File, OpenOptions};
use std::io::{self, prelude::*};
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
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
    cache: Option<CachedLayouts>,
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
            cache: None,
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
                            self.cache = None;
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
                        let mut buf = String::new();
                        if let Err(err) = file.read_to_string(&mut buf) {
                            tracing::error!(%err, "failed to read from file");
                            self.loaded.clear();
                            self.file = None;
                            self.file_err = Some(err);
                            self.path_persisted = false;
                            self.contents = buf;
                        } else {
                            self.loaded = path.to_path_buf();
                            self.file = Some(file);
                            self.file_err = None;
                            self.saved = true;
                            self.last_saved = Some(now());
                            self.path_persisted = false;
                            self.contents_persisted = false;
                            self.cache = None;
                            self.contents = buf;
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
        let (open_clicked, save_clicked, save_as_clicked) = ui.input_mut(|input| {
            let sa = input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND | egui::Modifiers::SHIFT,
                egui::Key::S,
            ));
            let s = input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::S,
            ));
            let o = input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::O,
            ));
            (o, s, sa)
        });
        ui.horizontal(|ui| {
            let open_button = ui.button("Open");
            let mut open_file = false;
            if open_button.clicked() || open_clicked {
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
            if ui.button("Save").clicked() || save_clicked {
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
            if ui.button("Save As").clicked() || save_as_clicked {
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
            |time| {
                format!(
                    "Last saved: {}:{:02}:{:02}",
                    time.hour(),
                    time.minute(),
                    time.second()
                )
            },
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
        // if let Err(err) = &self.document {
        //     egui::Frame::new()
        //         .fill(ui.style().visuals.extreme_bg_color)
        //         .corner_radius(4.0)
        //         .show(ui, |ui| {
        //             ui.label(
        //                 egui::RichText::new(err.to_string())
        //                     .monospace()
        //                     .color(ui.style().visuals.error_fg_color),
        //             );
        //         });
        // }
        if toml_editor(&mut self.contents, &mut self.document, &mut self.cache, ui).changed() {
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
fn toml_editor(
    buf: &mut String,
    document: &mut DocumentResult,
    cache: &mut Option<CachedLayouts>,
    ui: &mut egui::Ui,
) -> egui::Response {
    let available = ui.available_rect_before_wrap();
    let where_to_put_background = ui.painter().add(egui::Shape::Noop);
    let sao = egui::ScrollArea::vertical().show(ui, |ui| {
        ui.set_min_size(available.size());
        ui.set_clip_rect(available);
        ui.add_sized(
            available.size(),
            TomlEditorInner {
                buf,
                document,
                cache,
                width: available.width(),
            },
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
struct CachedLayouts {
    galley: Arc<egui::Galley>,
    err_rect: Option<egui::Rect>,
}
struct TomlEditorInner<'a> {
    buf: &'a mut String,
    document: &'a mut DocumentResult,
    cache: &'a mut Option<CachedLayouts>,
    width: f32,
}
impl egui::Widget for TomlEditorInner<'_> {
    #[allow(clippy::field_reassign_with_default)]
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let res = egui::TextEdit::multiline(self.buf)
            .code_editor()
            .frame(false)
            .layouter(&mut |ui, buf, _| {
                *self.cache = None;
                self.cache
                    .get_or_insert_with(|| {
                        use toml_parser::lexer::TokenKind as Tk;
                        let mono = &ui.style().text_styles[&egui::TextStyle::Monospace];
                        let buf = buf.as_str();
                        let err_span = self
                            .document
                            .as_ref()
                            .err()
                            .map(|err| err.span().unwrap_or(0..buf.len()));
                        let lexer = toml_parser::Source::new(buf).lex();
                        let mut layout = egui::text::LayoutJob::default();
                        layout.text = buf.to_string();
                        layout.wrap = egui::text::TextWrapping::wrap_at_width(self.width);
                        let mut white_start = 0;
                        for tok in lexer {
                            let span = tok.span();
                            let color = match tok.kind() {
                                Tk::Atom => Some(
                                    if buf.as_bytes()[span.start()..span.end()]
                                        .iter()
                                        .all(u8::is_ascii_digit)
                                    {
                                        egui::Color32::YELLOW
                                    } else {
                                        egui::Color32::LIGHT_GREEN
                                    },
                                ),
                                Tk::BasicString
                                | Tk::LiteralString
                                | Tk::MlBasicString
                                | Tk::MlLiteralString => Some(egui::Color32::LIGHT_BLUE),
                                Tk::Comment => Some(egui::Color32::DARK_GRAY),
                                _ => None,
                            };
                            if let Some(color) = color {
                                if white_start < span.start() {
                                    layout.sections.push(egui::text::LayoutSection {
                                        leading_space: 0.0,
                                        byte_range: white_start..span.start(),
                                        format: egui::TextFormat::simple(
                                            mono.clone(),
                                            egui::Color32::WHITE,
                                        ),
                                    });
                                    white_start = span.end();
                                }
                                layout.sections.push(egui::text::LayoutSection {
                                    leading_space: 0.0,
                                    byte_range: span.start()..span.end(),
                                    format: egui::TextFormat::simple(mono.clone(), color),
                                });
                            }
                        }
                        if white_start < buf.len() {
                            layout.sections.push(egui::text::LayoutSection {
                                leading_space: 0.0,
                                byte_range: white_start..buf.len(),
                                format: egui::TextFormat::simple(
                                    mono.clone(),
                                    egui::Color32::WHITE,
                                ),
                            });
                        }
                        let mut err_start = 0;
                        let mut err_end = 0;
                        if let Some(span) = &err_span {
                            for section in &mut layout.sections {
                                let range = section.byte_range.clone();
                                if range.end < span.start {
                                    continue;
                                }
                                if range.start >= span.end {
                                    break;
                                }
                                section.format.underline =
                                    egui::Stroke::new(1.0, egui::Color32::RED);
                                if err_start == 0 {
                                    err_start = range.start;
                                }
                                err_end = range.end;
                            }
                        }
                        let galley = ui.fonts_mut(|f| f.layout_job(layout));
                        let err_rect = (err_start < err_end)
                            .then(|| {
                                let mut indices = buf.char_indices().map(|x| x.0);
                                let mut rect: Option<egui::Rect> = None;
                                'outer: for row in &galley.rows {
                                    for (idx, glyph) in indices.by_ref().zip(&row.glyphs) {
                                        if idx < err_start {
                                            continue;
                                        }
                                        if idx >= err_end {
                                            break 'outer;
                                        }
                                        let r = glyph.logical_rect().translate(row.pos.to_vec2());
                                        if let Some(rect) = &mut rect {
                                            *rect = rect.union(r);
                                        } else {
                                            rect = Some(r);
                                        }
                                    }
                                }
                                rect
                            })
                            .flatten();
                        CachedLayouts { galley, err_rect }
                    })
                    .galley
                    .clone()
            })
            .show(ui);
        if let Some(CachedLayouts {
            err_rect: Some(rect),
            ..
        }) = self.cache
            && let Err(err) = self.document
        {
            let rect = rect.translate(res.galley_pos.to_vec2());
            if ui.rect_contains_pointer(rect) {
                let mut tip = egui::Tooltip::for_widget(&res.response.clone().with_new_rect(rect));
                tip.popup = tip
                    .popup
                    .frame(egui::Frame::popup(ui.style()).fill(ui.style().visuals.extreme_bg_color))
                    .anchor(rect);
                tip.show(|ui| {
                    ui.label(
                        egui::RichText::new(err.message()).color(ui.style().visuals.error_fg_color),
                    );
                });
            }
        }
        if res.response.changed() {
            *self.document = self.buf.parse();
            *self.cache = None;
        }
        res.response
    }
}
