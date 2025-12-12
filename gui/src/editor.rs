use crate::visit::{Receiver, Visitor, log};
use eframe::egui;
use std::fs::{File, OpenOptions};
use std::io::{self, prelude::*};
use std::path::PathBuf;
use std::pin::Pin;
use std::task::{Context, Poll, Waker};
use toml_parser::ParseError;

fn opts() -> OpenOptions {
    let mut opts = OpenOptions::new();
    opts.read(true).write(true).truncate(false);
    opts
}

#[derive(Clone, Copy)]
enum ConfirmReason {
    New,
    Open,
}

pub struct EditorState {
    contents: String,
    loaded: PathBuf,
    file: Option<File>,
    file_err: Option<io::Error>,
    save_fut: Option<Pin<Box<dyn Future<Output = Option<rfd::FileHandle>>>>>,
    open_fut: Option<Pin<Box<dyn Future<Output = Option<rfd::FileHandle>>>>>,
    saved: bool,
    contents_persisted: bool,
    path_persisted: bool,
    show_confirm: Option<ConfirmReason>,
    events: log::LoggedEvents,
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
        EditorState {
            contents,
            loaded,
            file,
            file_err,
            saved,
            save_fut: None,
            open_fut: None,
            contents_persisted: true,
            path_persisted: true,
            show_confirm: None,
            events: log::LoggedEvents::new(),
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
    pub fn file_menu(&mut self, ui: &mut egui::Ui) {
        let (open_pressed, save_pressed, save_as_pressed, new_pressed) = ui.input_mut(|input| {
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
            let n = input.consume_shortcut(&egui::KeyboardShortcut::new(
                egui::Modifiers::COMMAND,
                egui::Key::N,
            ));
            (o, s, sa, n)
        });
        let [open_clicked, save_clicked, save_as_clicked, new_clicked] = ui
            .menu_button("File", |ui| {
                [
                    ui.button("New").clicked(),
                    ui.button("Open").clicked(),
                    ui.button("Save").clicked(),
                    ui.button("Save As").clicked(),
                ]
            })
            .inner
            .unwrap_or([false; 4]);
        let mut open_file = false;
        if open_clicked || open_pressed {
            if self.saved || self.file.is_none() {
                open_file = true;
            } else {
                self.show_confirm = Some(ConfirmReason::Open);
            }
        }
        let mut new_file = false;
        if new_clicked || new_pressed {
            if self.saved || self.file.is_none() {
                new_file = true;
            } else {
                self.show_confirm = Some(ConfirmReason::New);
            }
        }
        if let Some(reason) = self.show_confirm {
            egui::Modal::new(egui::Id::new("unsaved-changes")).show(ui.ctx(), |ui| {
                ui.label("The currently open file has unsaved changes.");
                ui.horizontal(|ui| {
                    if ui.button("Save").clicked() {
                        if let Some(file) = &mut self.file {
                            self.last_saved = Some(now());
                            self.saved = true;
                            if let Err(err) = write_to_file(file, self.contents.as_bytes()) {
                                tracing::error!(%err, "error saving to file");
                                self.file_err = Some(err);
                            }
                        }
                        match reason {
                            ConfirmReason::New => new_file = true,
                            ConfirmReason::Open => open_file = true,
                        }
                        self.show_confirm = None;
                    }
                    if ui.button("Don't Save").clicked() {
                        match reason {
                            ConfirmReason::New => new_file = true,
                            ConfirmReason::Open => open_file = true,
                        }
                        self.show_confirm = None;
                    }
                    if ui.button("Cancel").clicked() {
                        self.show_confirm = None;
                    }
                })
            });
        }
        if open_file {
            self.open_fut = Some(Box::pin(
                rfd::AsyncFileDialog::new()
                    .add_filter("TOML", &["toml"])
                    .set_can_create_directories(true)
                    .pick_file(),
            ));
        }
        if new_file {
            self.contents.clear();
            self.contents_persisted = false;
            self.loaded.clear();
            self.path_persisted = false;
            self.file = None;
        }
        if save_clicked || save_pressed {
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
        if save_as_clicked || save_as_pressed {
            self.save_fut = Some(Box::pin(
                rfd::AsyncFileDialog::new()
                    .add_filter("TOML", &["toml"])
                    .set_can_create_directories(true)
                    .save_file(),
            ));
        }
    }
    pub fn parse_events(&mut self, ui: &mut egui::Ui) {
        let mut available = ui.available_rect_before_wrap();
        available.set_height(300.0);
        available.set_width(300.0);
        ui.set_max_size(available.size());
        ui.vertical(|ui| {
            ui.label("These are events from parsing:");
            let where_to_put_background = ui.painter().add(egui::Shape::Noop);
            self.events.show(ui);
            let visuals = ui.visuals();
            let widget = visuals.noninteractive();
            let shape = egui::epaint::RectShape::new(
                available,
                widget.corner_radius,
                visuals.text_edit_bg_color(),
                widget.bg_stroke,
                egui::StrokeKind::Inside,
            );
            ui.painter().set(where_to_put_background, shape);
        });
    }
    pub fn in_left(&mut self, visit: &mut dyn for<'i> Visitor<'i>, ui: &mut egui::Ui) {
        ui.heading("Editor");
        if self.loaded.as_os_str().is_empty() {
            ui.label("No file open");
        } else {
            ui.label(format!("Opened: {}", self.loaded.display()));
        }
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
        self.events.clear();
        let editor = TomlEditorInner {
            buf: &mut self.contents,
            visit: &mut log::LoggingVisitor(&mut self.events, visit),
        };
        if editor_frame(ui, editor).changed() {
            self.contents_persisted = false;
            self.saved = false;
        }
    }
    pub fn poll_futures(&mut self) {
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
fn maybe_underline(font: &egui::FontId, color: egui::Color32, error: bool) -> egui::TextFormat {
    let mut format = egui::TextFormat::simple(font.clone(), color);
    if error {
        format.underline = egui::Stroke::new(1.0, egui::Color32::RED);
    }
    format
}
/// Arbitrarily change the lifetime of a mutable reference
///
/// # Safety
/// The resulting reference must follow Rust's referencing rules
const unsafe fn unbind_lifetime<'dst, T: ?Sized>(x: &mut T) -> &'dst mut T {
    unsafe { std::mem::transmute(x) }
}

pub fn editor_frame(ui: &mut egui::Ui, widget: impl egui::Widget) -> egui::Response {
    let available = ui.available_rect_before_wrap();
    let where_to_put_background = ui.painter().add(egui::Shape::Noop);
    let resp = egui::ScrollArea::vertical()
        .show(ui, |ui| {
            ui.set_min_size(available.size());
            ui.set_clip_rect(available);
            ui.add_sized(available.size(), widget)
        })
        .inner;
    let visuals = ui.visuals();
    let widget = ui.style().interact(&resp);
    let background = visuals.text_edit_bg_color();
    let stroke = if resp.has_focus() {
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
    resp
}

struct TomlEditorInner<'b, 'v> {
    buf: &'b mut String,
    visit: &'v mut dyn for<'i> Visitor<'i>,
}
impl egui::Widget for TomlEditorInner<'_, '_> {
    #[allow(clippy::field_reassign_with_default)]
    fn ui(self, ui: &mut egui::Ui) -> egui::Response {
        let mut errs = Vec::new();
        let mut err_rects = Vec::new();
        let res = egui::TextEdit::multiline(self.buf)
            .code_editor()
            .frame(false)
            .layouter(&mut |ui, buf, _| {
                use toml_parser::lexer::TokenKind as Tk;
                let mono = &ui.style().text_styles[&egui::TextStyle::Monospace];
                let buf = buf.as_str();
                let source = toml_parser::Source::new(buf);
                let tokens = source.lex().collect::<Vec<_>>();
                let mut layout = egui::text::LayoutJob::default();
                layout.text = buf.to_string();
                layout.wrap = egui::text::TextWrapping::wrap_at_width(ui.available_width());
                let mut white_start = 0;
                let mut recv = Receiver::new(source, self.visit);
                toml_parser::parser::parse_document(
                    &tokens,
                    &mut toml_parser::parser::ValidateWhitespace::new(&mut recv, source),
                    &mut errs,
                );
                recv.finish(&mut errs);
                errs.sort_unstable_by_key(|k| k.context());
                let all_errs = errs.iter().any(|e| e.context().is_none());
                let mut err_slice = errs.as_mut_slice();
                for tok in &tokens {
                    let span = tok.span();
                    let mut color = match tok.kind() {
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

                    let mut is_err = false;
                    if let Some((head, tail)) =
                        unsafe { unbind_lifetime(err_slice).split_first_mut() } // Rust doesn't like this control flow, but the only place we store the reference is in err_slice
                        && let Some(err_span) = head.context()
                        && span.end() > err_span.start()
                        && span.start() <= err_span.end()
                    {
                        *head = std::mem::replace(head, ParseError::new("placeholder!"))
                            .with_context(span);
                        err_slice = tail;
                        is_err = true;
                    }
                    if is_err && !all_errs && color.is_none() {
                        color = Some(egui::Color32::WHITE);
                    }
                    if let Some(color) = color {
                        if white_start < span.start() {
                            layout.sections.push(egui::text::LayoutSection {
                                leading_space: 0.0,
                                byte_range: white_start..span.start(),
                                format: maybe_underline(mono, egui::Color32::WHITE, all_errs),
                            });
                        }
                        layout.sections.push(egui::text::LayoutSection {
                            leading_space: 0.0,
                            byte_range: span.start()..span.end(),
                            format: maybe_underline(mono, color, is_err || all_errs),
                        });
                        white_start = span.end();
                    }
                }
                if white_start < buf.len() {
                    layout.sections.push(egui::text::LayoutSection {
                        leading_space: 0.0,
                        byte_range: white_start..buf.len(),
                        format: maybe_underline(mono, egui::Color32::WHITE, all_errs),
                    });
                }
                ui.fonts_mut(|f| f.layout_job(layout))
            })
            .show(ui);
        let mut indices = self.buf.char_indices().map(|x| x.0);
        let split_idx = errs
            .iter()
            .position(|e| e.context().is_some())
            .unwrap_or(errs.len());
        err_rects.extend(
            errs.iter()
                .map(|err| (err.description(), None::<egui::Rect>)),
        );
        let (_without_span, mut with_span) = errs.split_at(split_idx);
        let (errs_without_span, mut errs_with_span) = err_rects.split_at_mut(split_idx);
        let mut overall_rect: Option<egui::Rect> = None;
        for row in &res.galley.rows {
            let mut glyphs = row.glyphs.as_slice();
            if !row.ends_with_newline {
                glyphs.split_off_last();
            }
            for (idx, glyph) in indices.by_ref().zip(glyphs) {
                let r = glyph.logical_rect().translate(row.pos.to_vec2());
                if let Some(rect) = &mut overall_rect {
                    *rect = rect.union(r);
                } else {
                    overall_rect = Some(r);
                }
                while let (Some((err_head, err_tail)), Some(((_, rect_head), rect_tail))) = unsafe {
                    (
                        with_span.split_first(),
                        unbind_lifetime(errs_with_span).split_first_mut(),
                    )
                } {
                    let span = err_head.context().unwrap();
                    if idx < span.start() {
                        break;
                    }
                    with_span = err_tail;
                    errs_with_span = rect_tail;
                    if idx > span.end() {
                        continue;
                    }
                    if let Some(rect) = rect_head {
                        *rect = rect.union(r);
                    } else {
                        *rect_head = Some(r);
                    }
                }
            }
        }
        if let Some(overall_rect) = overall_rect {
            for err in errs_without_span {
                err.1 = Some(overall_rect);
            }
        }
        let err = err_rects.iter().find_map(|&(msg, rect)| {
            let rect = rect?.translate(res.galley_pos.to_vec2());
            ui.rect_contains_pointer(rect).then_some((msg, rect))
        });
        if let Some((msg, rect)) = err {
            let mut tip = egui::Tooltip::for_widget(&res.response.clone().with_new_rect(rect));
            tip.popup = tip
                .popup
                .frame(egui::Frame::popup(ui.style()).fill(ui.style().visuals.extreme_bg_color))
                .anchor(rect);
            tip.show(|ui| {
                ui.label(egui::RichText::new(msg).color(ui.style().visuals.error_fg_color));
            });
        }
        res.response
    }
}
