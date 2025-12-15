use eframe::egui;
use std::borrow::Cow;
use std::collections::VecDeque;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, mpsc};
use tracing::Subscriber;
use tracing::level_filters::LevelFilter;
use tracing_subscriber::{EnvFilter, Layer, Registry, reload};

use crate::trace::color::ToColor32;

mod color;

#[derive(Clone)]
struct LogEntry {
    time: time::OffsetDateTime,
    fields: Vec<(&'static str, String)>,
    level: tracing::Level,
    target: Cow<'static, str>,
}

#[derive(Default)]
struct SharedData {
    filter_noisy: AtomicBool,
}

struct LevelsFilter {
    error: bool,
    warn: bool,
    info: bool,
    debug: bool,
    trace: bool,
}
impl LevelsFilter {
    fn allows(&self, level: tracing::Level) -> bool {
        match level {
            tracing::Level::ERROR => self.error,
            tracing::Level::WARN => self.warn,
            tracing::Level::INFO => self.info,
            tracing::Level::DEBUG => self.debug,
            tracing::Level::TRACE => self.trace,
        }
    }
}

pub struct LogWidget {
    shared: Arc<SharedData>,
    recv: mpsc::Receiver<LogEntry>,
    filter: String,
    handle: reload::Handle<EnvFilter, Registry>,
    queue: VecDeque<LogEntry>,
    filter_noisy: bool,
    levels: LevelsFilter,
    pattern_str: String,
    pattern: Option<matchers::Pattern>,
    pattern_err: Option<matchers::BuildError>,
    filtered: Vec<LogEntry>,
}
impl LogWidget {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.heading("Logs");
        ui.horizontal(|ui| {
            ui.label("Input Filter: ")
                .on_hover_text("A filter for captured events");
            if ui.text_edit_singleline(&mut self.filter).changed() {
                let _ = self.handle.reload(parse_filter(&self.filter));
            }
            ui.checkbox(&mut self.filter_noisy, "Filter Noisy events")
                .on_hover_text("GUI events send noisy trace events, this can filter them");
            self.shared
                .filter_noisy
                .store(self.filter_noisy, Ordering::Relaxed);
        });
        let builder = egui_extras::TableBuilder::new(ui)
            .resizable(true)
            .stick_to_bottom(true)
            .striped(true)
            .column(egui_extras::Column::initial(250.0).clip(true))
            .column(egui_extras::Column::initial(50.0))
            .column(egui_extras::Column::initial(100.0).clip(true))
            .column(egui_extras::Column::remainder().clip(true))
            .auto_shrink(false);
        let mut refresh = false;
        let table = builder.header(15.0, |mut row| {
            row.col(|ui| {
                ui.label("Time");
            });
            row.col(|ui| {
                let button = ui.button("Level");
                egui::Popup::from_toggle_button_response(&button)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        ui.horizontal(|ui| {
                            if ui.button("All").clicked() {
                                self.levels.error = true;
                                self.levels.warn = true;
                                self.levels.info = true;
                                self.levels.debug = true;
                                self.levels.trace = true;
                                refresh = true;
                            }
                            if ui.button("None").clicked() {
                                self.levels.error = false;
                                self.levels.warn = false;
                                self.levels.info = false;
                                self.levels.debug = false;
                                self.levels.trace = false;
                                refresh = true;
                            }
                        });
                        refresh |= ui
                            .checkbox(&mut self.levels.error, tracing::Level::ERROR.to_rich_text())
                            .changed();
                        refresh |= ui
                            .checkbox(&mut self.levels.warn, tracing::Level::WARN.to_rich_text())
                            .changed();
                        refresh |= ui
                            .checkbox(&mut self.levels.info, tracing::Level::INFO.to_rich_text())
                            .changed();
                        refresh |= ui
                            .checkbox(&mut self.levels.debug, tracing::Level::DEBUG.to_rich_text())
                            .changed();
                        refresh |= ui
                            .checkbox(&mut self.levels.trace, tracing::Level::TRACE.to_rich_text())
                            .changed();
                    });
            });
            row.col(|ui| {
                let button = ui.button("Target");
                egui::Popup::from_toggle_button_response(&button)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        let edit = ui
                            .text_edit_singleline(&mut self.pattern_str)
                            .on_hover_text("A regex for event targets");
                        if edit.changed() {
                            if self.pattern_str.is_empty() {
                                self.pattern = None;
                                self.pattern_err = None;
                                refresh = true;
                            } else {
                                match matchers::Pattern::new(&self.pattern_str) {
                                    Ok(pat) => {
                                        self.pattern = Some(pat);
                                        self.pattern_err = None;
                                        refresh = true;
                                    }
                                    Err(err) => {
                                        self.pattern_err = Some(err);
                                    }
                                }
                            }
                        }
                        if let Some(err) = &self.pattern_err {
                            egui::Popup::from_response(&edit)
                                .frame(
                                    egui::Frame::popup(ui.style())
                                        .fill(ui.style().visuals.extreme_bg_color),
                                )
                                .show(|ui| {
                                    ui.label(
                                        egui::RichText::new(err.to_string())
                                            .color(ui.style().visuals.error_fg_color),
                                    );
                                });
                        }
                    });
            });
            row.col(|ui| {
                ui.label("Message");
            });
        });
        for e in self.recv.try_iter() {
            refresh = true;
            if self.queue.len() == 1024 {
                self.queue.pop_front();
            }
            self.queue.push_back(e);
        }
        if refresh {
            self.filtered.clear();
            self.filtered.extend(
                self.queue
                    .iter()
                    .filter(|e| {
                        self.levels.allows(e.level)
                            && self.pattern.as_ref().is_none_or(|p| p.matches(&e.target))
                    })
                    .cloned(),
            );
        }
        table.body(|mut body| {
            let height = body.ui_mut().text_style_height(&egui::TextStyle::Body);
            body.rows(height, self.filtered.len(), |mut row| {
                let entry = &self.filtered[row.index()];
                row.col(|ui| {
                    ui.label(
                        entry
                            .time
                            .format(&time::format_description::well_known::Rfc3339)
                            .unwrap(),
                    );
                });
                row.col(|ui| {
                    ui.label(entry.level.to_rich_text());
                });
                row.col(|ui| {
                    ui.label(
                        egui::RichText::new(&*entry.target)
                            .monospace()
                            .color(ui.style().visuals.weak_text_color()),
                    );
                });
                row.col(|ui| {
                    let message = entry
                        .fields
                        .iter()
                        .find(|e| e.0 == "message")
                        .map_or("", |e| &e.1);
                    ui.label(message);
                });
            });
        });
        ui.ctx()
            .request_repaint_after(std::time::Duration::from_millis(100));
    }
}
pub struct LogLayer {
    shared: Arc<SharedData>,
    send: mpsc::SyncSender<LogEntry>,
}
impl<S: Subscriber> Layer<S> for LogLayer {
    fn on_event(
        &self,
        event: &tracing::Event<'_>,
        _ctx: tracing_subscriber::layer::Context<'_, S>,
    ) {
        use tracing_log::NormalizeEvent;
        let raw_meta = event.metadata();
        let normalized_meta = event.normalized_metadata();
        let target = if let Some(meta) = normalized_meta {
            Cow::Owned(meta.target().to_string())
        } else {
            Cow::Borrowed(raw_meta.target())
        };
        if self.shared.filter_noisy.load(Ordering::Relaxed)
            && ["egui", "eframe", "calloop"]
                .iter()
                .any(|p| target.starts_with(p))
        {
            return;
        }
        let mut fields = FieldVisitor(Vec::with_capacity(raw_meta.fields().len()));
        event.record(&mut fields);
        let _ = self.send.send(LogEntry {
            time: super::now(),
            fields: fields.0,
            level: *raw_meta.level(),
            target,
        });
    }
}

struct FieldVisitor(Vec<(&'static str, String)>);
impl tracing::field::Visit for FieldVisitor {
    fn record_debug(&mut self, field: &tracing::field::Field, value: &dyn core::fmt::Debug) {
        self.0.push((field.name(), format!("{value:?}")));
    }
}

fn parse_filter(filter: &str) -> EnvFilter {
    EnvFilter::builder()
        .with_default_directive(LevelFilter::INFO.into())
        .parse_lossy(filter)
}

pub fn create() -> (reload::Layer<EnvFilter, Registry>, LogLayer, LogWidget) {
    let filter = std::env::var("RUST_LOG").unwrap_or_default();
    let (layer, handle) = reload::Layer::new(parse_filter(&filter));
    let shared = Arc::default();
    let (send, recv) = mpsc::sync_channel(512);
    (
        layer,
        LogLayer {
            shared: Arc::clone(&shared),
            send,
        },
        LogWidget {
            shared,
            recv,
            queue: VecDeque::new(),
            filter,
            handle,
            filter_noisy: true,
            levels: LevelsFilter {
                error: true,
                warn: true,
                info: true,
                debug: true,
                trace: true,
            },
            pattern_str: String::new(),
            pattern: None,
            pattern_err: None,
            filtered: Vec::new(),
        },
    )
}
