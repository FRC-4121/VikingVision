use super::*;
use eframe::egui;

#[derive(Debug, Clone)]
struct LogEntry(&'static str, String);

#[derive(Debug, Default, Clone)]
pub struct LoggedEvents(Vec<LogEntry>);
impl LoggedEvents {
    pub const fn new() -> Self {
        Self(Vec::new())
    }
    pub fn clear(&mut self) {
        self.0.clear();
    }
    pub fn show(&self, ui: &mut egui::Ui) {
        let row_height = ui.text_style_height(&egui::TextStyle::Monospace);
        let rect = ui.available_rect_before_wrap();
        ui.set_clip_rect(rect);
        egui::ScrollArea::both().auto_shrink(false).show_rows(
            ui,
            row_height * 0.9, // make sure we see all of the rows
            self.0.len(),
            |ui, rows| {
                let mut content = String::new();
                for row in &self.0[rows] {
                    use std::fmt::Write;
                    let _ = writeln!(content, "{:<11} {}", row.0, row.1);
                }
                ui.style_mut().wrap_mode = Some(egui::TextWrapMode::Extend);
                ui.label(egui::RichText::new(content).monospace());
            },
        );
    }
}

pub struct LoggingVisitor<'a>(
    pub &'a mut LoggedEvents,
    pub &'a mut dyn for<'i> Visitor<'i>,
);
impl<'i> Visitor<'i> for LoggingVisitor<'_> {
    fn begin_def(&mut self, key: Span) {
        self.0.0.push(LogEntry("begin_def", format!("{key:?}")));
        self.1.begin_def(key);
    }
    fn end_def(&mut self, key: Span, value: Span) {
        self.0
            .0
            .push(LogEntry("end_def", format!("{key:?} = {value:?}")));
        self.1.end_def(key, value);
    }
    fn accept_scalar(
        &mut self,
        path: RawsIter<'_, 'i>,
        scalar: ScalarInfo<'i>,
        error: &mut dyn ErrorSink,
    ) {
        let s = scalar.raw.as_str();
        self.0.0.push(LogEntry("scalar", format!("{path} = {s}")));
        self.1.accept_scalar(path, scalar, error);
    }
    fn begin_array(&mut self, path: RawsIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool {
        self.0.0.push(LogEntry("begin_array", path.to_string()));
        self.1.begin_array(path, error)
    }
    fn end_array(
        &mut self,
        path: RawsIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
        self.0.0.push(LogEntry(
            "end_array",
            format!("{path} ({key:?} => {value:?})"),
        ));
        self.1.end_array(path, key, value, error);
    }
    fn begin_table(&mut self, path: RawsIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool {
        self.0.0.push(LogEntry("begin_table", path.to_string()));
        self.1.begin_table(path, error)
    }
    fn end_table(
        &mut self,
        path: RawsIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
        self.0.0.push(LogEntry(
            "end_table",
            format!("{path} ({key:?} => {value:?})"),
        ));
        self.1.end_table(path, key, value, error);
    }
    fn finish(&mut self, source: Source<'i>, error: &mut dyn ErrorSink) {
        self.1.finish(source, error);
    }
}
