use crate::dyn_elem::DynElemType;
use crate::edit::{Edits, format_string};
use crate::visit::prelude::*;
use eframe::egui;
use toml_parser::decoder::{Encoding, ScalarKind};
use viking_vision::buffer::PixelFormat;

#[derive(Default)]
enum Frame {
    #[default]
    NoSource,
    Path(Spanned<String>, Encoding, Span),
    ColorString(Spanned<String>, Encoding, Span),
    ColorStruct {
        format: Option<Spanned<Spanned<(Option<PixelFormat>, String, Encoding)>>>,
        bytes: Option<Spanned<Vec<Spanned<u8>>>>,
        total_spans: Vec<Span>,
        defs: Vec<Span>,
        invalid_bytes: bool,
    },
}
pub struct V4l {}

#[derive(Clone, Copy)]
struct Spanned<T> {
    val: T,
    span: Span,
}

#[derive(Default)]
enum Inner {
    #[default]
    Unknown,
    Frame(Frame),
    V4l(V4l),
}

#[derive(Default)]
struct Common {
    width: Option<Spanned<u32>>,
    height: Option<Spanned<u32>>,
    fov: Option<(Spanned<f32>, Span)>,
}

#[derive(Default)]
pub struct CameraConfig {
    pub window_id: Option<egui::Id>,
    common: Common,
    inner: Inner,
}

impl DynElemType for CameraConfig {
    type Visitor<'a>
        = CCVisitor<'a>
    where
        Self: 'a;
    fn add(&mut self, ui: &mut eframe::egui::Ui) -> bool {
        let width = &mut self
            .common
            .width
            .get_or_insert(Spanned {
                val: 0,
                span: Span::new_unchecked(0, 0),
            })
            .val;
        let height = &mut self
            .common
            .height
            .get_or_insert(Spanned {
                val: 0,
                span: Span::new_unchecked(0, 0),
            })
            .val;
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(width));
            ui.label("Width");
        });
        ui.horizontal(|ui| {
            ui.add(egui::DragValue::new(height));
            ui.label("Height");
        });
        ui.horizontal(|ui| {
            let mut show_fov = self.common.fov.is_some();
            ui.checkbox(&mut show_fov, "FoV");
            if show_fov {
                let (fov, _) = self.common.fov.get_or_insert((
                    Spanned {
                        val: 45.0,
                        span: Span::new_unchecked(0, 0),
                    },
                    Span::new_unchecked(0, 0),
                ));
                ui.add(egui::DragValue::new(&mut fov.val).range(0..=90));
            } else {
                self.common.fov = None;
            }
        });
        *width != 0 && *height != 0
    }
    fn finish(&mut self, new: &mut String) {
        use std::fmt::Write;
        if let Some(width) = self.common.width {
            let _ = writeln!(new, "width = {}", width.val);
        }
        if let Some(height) = self.common.height {
            let _ = writeln!(new, "height = {}", height.val);
        }
        if let Some((fov, _)) = self.common.fov {
            let _ = writeln!(new, "fov = {}", fov.val);
        }
    }
    fn show(&mut self, ui: &mut egui::Ui, edits: &mut Edits) {
        if let Some(width) = &mut self.common.width {
            ui.horizontal(|ui| {
                let resp = ui.add(egui::DragValue::new(&mut width.val));
                ui.label("Width");
                if resp.changed() {
                    edits.replace(width.span, width.val.to_string());
                }
            });
        }
        if let Some(height) = &mut self.common.height {
            ui.horizontal(|ui| {
                let resp = ui.add(egui::DragValue::new(&mut height.val));
                ui.label("Height");
                if resp.changed() {
                    edits.replace(height.span, height.val.to_string());
                }
            });
        }
        ui.horizontal(|ui| {
            let mut show_fov = self.common.fov.is_some();
            let old = show_fov;
            ui.checkbox(&mut show_fov, "FoV");
            if show_fov {
                let (fov, _) = self.common.fov.get_or_insert((
                    Spanned {
                        val: 45.0,
                        span: Span::new_unchecked(0, 0),
                    },
                    Span::new_unchecked(0, 0),
                ));
                let resp = ui.add(egui::DragValue::new(&mut fov.val).range(0..=90));
                if !old {
                    // TODO: figure out where to insert
                } else if resp.changed() {
                    edits.replace(fov.span, fov.val.to_string());
                }
            } else if let Some((_, old)) = self.common.fov.take() {
                edits.delete(old);
            }
        });
        match &mut self.inner {
            Inner::Frame(frame) => {
                let mut i = match frame {
                    Frame::Path(..) => 0,
                    Frame::ColorString(..) => 1,
                    Frame::ColorStruct { .. } => 2,
                    Frame::NoSource => 3,
                };
                let old = i;
                egui::ComboBox::new(ui.next_auto_id(), "Source").show_index(ui, &mut i, 3, |i| {
                    ["Path", "Color (String)", "Color (Fields)", ""][i]
                });
                if i != old {
                    match frame {
                        Frame::Path(_, _, span) | Frame::ColorString(_, _, span) => {
                            edits.delete(*span);
                        }
                        Frame::ColorStruct { total_spans, .. } => {
                            edits.delete_all(total_spans.iter().copied());
                        }
                        Frame::NoSource => {}
                    }
                }
                match frame {
                    Frame::NoSource => {}
                    Frame::ColorString(string, encoding, _) | Frame::Path(string, encoding, _) => {
                        if ui.text_edit_singleline(&mut string.val).changed() {
                            edits.replace(string.span, format_string(&string.val, encoding));
                        }
                    }
                    Frame::ColorStruct { format, bytes, .. } => {
                        if let Some(Spanned {
                            val:
                                Spanned {
                                    val: (fmt, edit, encoding),
                                    span,
                                },
                            ..
                        }) = format
                        {
                            let line = ui.text_edit_singleline(edit);
                            if line.changed() {
                                edits.replace(*span, format_string(edit, encoding));
                            }
                            *fmt = edit
                                .parse()
                                .inspect_err(|err: &viking_vision::buffer::FormatParseError| {
                                    let mut tip = egui::Tooltip::for_widget(&line);
                                    tip.popup = tip.popup.frame(
                                        egui::Frame::popup(ui.style())
                                            .fill(ui.style().visuals.extreme_bg_color),
                                    );
                                    tip.show(|ui| {
                                        ui.label(
                                            egui::RichText::new(err.to_string())
                                                .color(ui.style().visuals.error_fg_color),
                                        );
                                    });
                                })
                                .ok();
                        }
                    }
                }
            }
            Inner::V4l(_) => {}
            Inner::Unknown => {}
        }
    }
    fn visit(&mut self) -> Self::Visitor<'_> {
        CCVisitor {
            common: &mut self.common,
            kind: match &mut self.inner {
                Inner::Unknown => CCVInner::Unknown,
                Inner::Frame(inner) => CCVInner::Frame {
                    inner,
                    new: Frame::NoSource,
                },
                Inner::V4l(inner) => CCVInner::V4l(inner),
            },
            width: None,
            height: None,
            fov: None,
            defs: Vec::new(),
        }
    }
    fn kinds() -> &'static [(&'static str, &'static str)] {
        &[("frame", "Frame"), ("v4l", "V4L")]
    }
    fn common_keys() -> &'static [&'static str] {
        &["width", "height", "fov", "outputs"]
    }
    fn kind(&self) -> usize {
        match self.inner {
            Inner::Frame(_) => 0,
            Inner::V4l(_) => 1,
            Inner::Unknown => usize::MAX,
        }
    }
    fn set_kind(&mut self, kind: usize) {
        match kind {
            0 => self.inner = Inner::Frame(Frame::NoSource),
            1 => self.inner = Inner::V4l(V4l {}),
            _ => self.inner = Inner::Unknown,
        }
    }
}

enum CCVInner<'a> {
    Unknown,
    Frame { inner: &'a mut Frame, new: Frame },
    V4l(&'a mut V4l),
}

pub struct CCVisitor<'a> {
    common: &'a mut Common,
    kind: CCVInner<'a>,
    width: Option<Option<Spanned<u32>>>,
    height: Option<Option<Spanned<u32>>>,
    fov: Option<Option<(Spanned<f32>, Span)>>,
    defs: Vec<Span>,
}

#[allow(unused_variables)]
impl<'i> Visitor<'i> for CCVisitor<'_> {
    fn begin_def(&mut self, key: Span) {
        self.defs.push(key);
    }
    fn accept_scalar(
        &mut self,
        mut path: PathIter<'_, 'i>,
        scalar: ScalarInfo<'i>,
        error: &mut dyn ErrorSink,
    ) {
        if let Some(PathKind::Key(k)) = path.next() {
            let mut s = String::new();
            k.decode_key(&mut s, &mut ());
            match &*s {
                "width" if path.next().is_none() => {
                    if self.width.is_some() {
                        error.report_error(
                            ParseError::new("Duplicate key .width").with_context(scalar.raw.span()),
                        );
                    } else if scalar.kind
                        == ScalarKind::Integer(toml_parser::decoder::IntegerRadix::Dec)
                        && let Ok(val) = scalar.raw.as_str().parse()
                    {
                        self.width = Some(Some(Spanned {
                            val,
                            span: scalar.raw.span(),
                        }));
                    } else {
                        error.report_error(
                            ParseError::new(format!(
                                "Expected an integer for key .width, got a {}",
                                scalar.kind.description()
                            ))
                            .with_context(scalar.raw.span()),
                        );
                    }
                }
                "height" if path.next().is_none() => {
                    if self.height.is_some() {
                        error.report_error(
                            ParseError::new("Duplicate key .width").with_context(scalar.raw.span()),
                        );
                    } else if scalar.kind
                        == ScalarKind::Integer(toml_parser::decoder::IntegerRadix::Dec)
                        && let Ok(val) = scalar.raw.as_str().parse()
                    {
                        self.height = Some(Some(Spanned {
                            val,
                            span: scalar.raw.span(),
                        }));
                    } else {
                        error.report_error(
                            ParseError::new(format!(
                                "Expected an integer for key .width, got a {}",
                                scalar.kind.description()
                            ))
                            .with_context(scalar.raw.span()),
                        );
                    }
                }
                "fov" if path.next().is_none() => {
                    if self.fov.is_some() {
                        error.report_error(
                            ParseError::new("Duplicate key .fov").with_context(scalar.raw.span()),
                        );
                    } else if (scalar.kind
                        == ScalarKind::Integer(toml_parser::decoder::IntegerRadix::Dec)
                        || scalar.kind == ScalarKind::Float)
                        && let Ok(val) = scalar.raw.as_str().parse()
                    {
                        self.fov = Some(Some((
                            Spanned {
                                val,
                                span: scalar.raw.span(),
                            },
                            scalar.full,
                        )));
                    } else {
                        error.report_error(
                            ParseError::new(format!(
                                "Expected a float for key .width, got a {}",
                                scalar.kind.description()
                            ))
                            .with_context(scalar.raw.span()),
                        );
                    }
                }
                "outputs" => {}
                "path" if path.next().is_none() => match &mut self.kind {
                    CCVInner::Frame { new, .. } => match new {
                        Frame::NoSource => {
                            let mut val = String::new();
                            if scalar.kind == ScalarKind::String {
                                let _ = scalar.raw.decode_scalar(&mut val, &mut ());
                            } else {
                                error.report_error(
                                    ParseError::new(format!(
                                        "Expected a string for key .path, got a {}",
                                        scalar.kind.description()
                                    ))
                                    .with_context(scalar.raw.span()),
                                );
                            }
                            *new = Frame::Path(
                                Spanned {
                                    val,
                                    span: scalar.raw.span(),
                                },
                                scalar.raw.encoding().unwrap_or(Encoding::BasicString),
                                scalar.full,
                            );
                        }
                        Frame::Path(..) => error.report_error(
                            ParseError::new("Duplicate key .path").with_context(scalar.raw.span()),
                        ),
                        _ => error.report_error(
                            ParseError::new("Key .path conflicts with .color")
                                .with_context(scalar.raw.span()),
                        ),
                    },
                    CCVInner::V4l(_) => {}
                    CCVInner::Unknown => {}
                },
                "color" => match &mut self.kind {
                    CCVInner::Frame { new, .. } => match new {
                        Frame::NoSource => {
                            let mut val = String::new();
                            if path.next().is_some() {
                                unreachable!()
                            } else if scalar.kind == ScalarKind::String {
                                let _ = scalar.raw.decode_scalar(&mut val, &mut ());
                            } else {
                                error.report_error(
                                    ParseError::new(format!(
                                        "Expected a float for key .path, got a {}",
                                        scalar.kind.description()
                                    ))
                                    .with_context(scalar.raw.span()),
                                );
                            }
                            *new = Frame::ColorString(
                                Spanned {
                                    val,
                                    span: scalar.raw.span(),
                                },
                                scalar.raw.encoding().unwrap_or(Encoding::BasicString),
                                scalar.full,
                            );
                        }
                        Frame::Path(..) => error.report_error(
                            ParseError::new("Key .color conflicts with .path")
                                .with_context(k.span()),
                        ),
                        Frame::ColorString(..) => error.report_error(
                            ParseError::new("Duplicate key .color").with_context(scalar.raw.span()),
                        ),
                        Frame::ColorStruct { format, bytes, .. } => {
                            if let Some(PathKind::Key(k2)) = path.next() {
                                let mut s = String::new();
                                k2.decode_key(&mut s, &mut ());
                                match &*s {
                                    "format" => {
                                        if format.is_some() {
                                            error.report_error(
                                                ParseError::new("Duplicate key .color.format")
                                                    .with_context(k2.span()),
                                            );
                                        } else {
                                            let val = if scalar.kind == ScalarKind::String {
                                                let mut s = String::new();
                                                let _ = scalar.raw.decode_scalar(&mut s, &mut ());
                                                let fmt = s
                                                    .parse::<PixelFormat>()
                                                    .inspect_err(|err| {
                                                        error.report_error(
                                                            ParseError::new(err.to_string())
                                                                .with_context(scalar.raw.span()),
                                                        )
                                                    })
                                                    .ok();
                                                (
                                                    fmt,
                                                    s,
                                                    scalar
                                                        .raw
                                                        .encoding()
                                                        .unwrap_or(Encoding::BasicString),
                                                )
                                            } else {
                                                (None, String::new(), Encoding::BasicString)
                                            };
                                            *format = Some(Spanned {
                                                val: Spanned {
                                                    val,
                                                    span: scalar.raw.span(),
                                                },
                                                span: scalar.full,
                                            });
                                        }
                                    }
                                    "bytes" => {}
                                    _ => error.report_error(
                                        ParseError::new(format!(
                                            "Unknown key .color.{}",
                                            k2.as_str()
                                        ))
                                        .with_context(k2.span()),
                                    ),
                                }
                            } else {
                                error.report_error(
                                    ParseError::new("Duplicate key .color").with_context(k.span()),
                                );
                            }
                        }
                    },
                    CCVInner::V4l(_) => error.report_error(
                        ParseError::new(format!("Unknown key .{}", k.as_str()))
                            .with_context(k.span()),
                    ),
                    CCVInner::Unknown => {}
                },
                _ => {
                    error.report_error(
                        ParseError::new(format!("Unknown key .{}", k.as_str()))
                            .with_context(k.span()),
                    );
                }
            }
        }
    }
    fn begin_array(&mut self, mut path: PathIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool {
        if let Some(PathKind::Key(k)) = path.next() {
            let mut s = String::new();
            k.decode_key(&mut s, &mut ());
            match &*s {
                "outputs" => {}
                "color" => {
                    if let CCVInner::Frame {
                        new:
                            Frame::ColorStruct {
                                bytes,
                                invalid_bytes,
                                ..
                            },
                        ..
                    } = &mut self.kind
                        && let Some(PathKind::Key(k2)) = path.next()
                    {
                        let mut s = String::new();
                        k2.decode_key(&mut s, &mut ());
                        match &*s {
                            "bytes" => {
                                if bytes.is_none() {
                                    *bytes = Some(Spanned {
                                        val: Vec::new(),
                                        span: Span::new_unchecked(0, 0),
                                    });
                                    *invalid_bytes = false;
                                    return true;
                                }
                                *invalid_bytes = true;
                                error.report_error(
                                    ParseError::new("Duplicate key .color.bytes")
                                        .with_context(k2.span()),
                                );
                            }
                            "format" => {}
                            _ => {
                                error.report_error(
                                    ParseError::new(format!("Unknown key .{}", k.as_str()))
                                        .with_context(k2.span()),
                                );
                            }
                        }
                    }
                }
                "width" | "height" | "fov" => {}
                _ => {
                    error.report_error(
                        ParseError::new(format!("Unknown key .{}", k.as_str()))
                            .with_context(k.span()),
                    );
                }
            }
        }
        false
    }
    fn end_array(
        &mut self,
        mut path: PathIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
        if let Some(PathKind::Key(k)) = path.next() {
            let mut s = String::new();
            k.decode_key(&mut s, &mut ());
            match &*s {
                "width" => {
                    error.report_error(
                        ParseError::new("Expected an integer for key .width, got an array")
                            .with_context(value),
                    );
                }
                "height" => {
                    error.report_error(
                        ParseError::new("Expected an integer for key .height, got an array")
                            .with_context(value),
                    );
                }
                "fov" => {
                    error.report_error(
                        ParseError::new("Expected a float for key .fov, got an array")
                            .with_context(value),
                    );
                }
                "path"
                    if matches!(
                        self.kind,
                        CCVInner::Frame {
                            new: Frame::NoSource | Frame::Path(..),
                            ..
                        }
                    ) =>
                {
                    error.report_error(
                        ParseError::new("Expected a string for key .path, got an array")
                            .with_context(value),
                    );
                }
                "color" => {
                    if let Some(PathKind::Key(k2)) = path.next() {
                        let mut s = String::new();
                        k2.decode_key(&mut s, &mut ());
                        match &*s {
                            "bytes" => {
                                if let CCVInner::Frame {
                                    new:
                                        Frame::ColorStruct {
                                            bytes: Some(Spanned { span, .. }),
                                            invalid_bytes: false,
                                            ..
                                        },
                                    ..
                                } = &mut self.kind
                                {
                                    *span = value;
                                }
                            }
                            "format" => {
                                error.report_error(
                                    ParseError::new(
                                        "Expected a string for key .color.format, got an array",
                                    )
                                    .with_context(value),
                                );
                            }
                            _ => {}
                        }
                    } else if matches!(self.kind, CCVInner::Frame { .. }) {
                        error.report_error(
                            ParseError::new(
                                "Expected a string or table for key .color, got an array",
                            )
                            .with_context(value),
                        );
                    }
                }
                _ => {}
            }
        }
    }
    fn begin_table(&mut self, mut path: PathIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool {
        if let Some(PathKind::Key(k)) = path.next() {
            let mut s = String::new();
            k.decode_key(&mut s, &mut ());
            match &*s {
                "color" => match &mut self.kind {
                    CCVInner::Frame { new, .. } => {
                        if let Some(PathKind::Key(k2)) = path.next() {
                            error.report_error(
                                ParseError::new(format!("Unknown key .color.{}", k2.as_str()))
                                    .with_context(k2.span()),
                            );
                        } else {
                            match new {
                                Frame::NoSource => {
                                    *new = Frame::ColorStruct {
                                        format: None,
                                        bytes: None,
                                        total_spans: Vec::new(),
                                        defs: vec![k.span()],
                                        invalid_bytes: false,
                                    };
                                    return true;
                                }
                                Frame::ColorStruct { defs, .. } => {
                                    defs.push(k.span());
                                    return true;
                                }
                                Frame::ColorString(..) => {
                                    error.report_error(
                                        ParseError::new("Duplicate key .color")
                                            .with_context(k.span()),
                                    );
                                }
                                Frame::Path(..) => {
                                    error.report_error(
                                        ParseError::new("Key .color conflicts with .path")
                                            .with_context(k.span()),
                                    );
                                }
                            }
                        }
                    }
                    CCVInner::V4l(_) => {
                        error.report_error(
                            ParseError::new(format!("Unknown key .{}", k.as_str()))
                                .with_context(k.span()),
                        );
                    }
                    CCVInner::Unknown => {}
                },
                "width" | "height" | "fov" | "outputs" => {}
                _ => {
                    error.report_error(
                        ParseError::new(format!("Unknown key .{}", k.as_str()))
                            .with_context(k.span()),
                    );
                }
            }
        }
        false
    }
    fn end_table(
        &mut self,
        mut path: PathIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
        if let Some(PathKind::Key(k)) = path.next() {
            let mut s = String::new();
            k.decode_key(&mut s, &mut ());
            match &*s {
                "width" => {
                    error.report_error(
                        ParseError::new("Expected an integer for key .width, got a table")
                            .with_context(value),
                    );
                }
                "height" => {
                    error.report_error(
                        ParseError::new("Expected an integer for key .height, got a table")
                            .with_context(value),
                    );
                }
                "fov" => {
                    error.report_error(
                        ParseError::new("Expected a float for key .fov, got a table")
                            .with_context(value),
                    );
                }
                "path"
                    if matches!(
                        self.kind,
                        CCVInner::Frame {
                            new: Frame::NoSource | Frame::Path(..),
                            ..
                        }
                    ) =>
                {
                    error.report_error(
                        ParseError::new("Expected a string for key .path, got a table")
                            .with_context(value),
                    );
                }
                "outputs" => {
                    error.report_error(
                        ParseError::new("Expected a string or table for key .color, got a table")
                            .with_context(value),
                    );
                }
                "color" => {
                    if let CCVInner::Frame {
                        new: Frame::ColorStruct { total_spans, .. },
                        ..
                    } = &mut self.kind
                    {
                        total_spans.push(value);
                    }
                }
                _ => {}
            }
        }
    }
    fn finish(&mut self, source: Source<'i>, error: &mut dyn ErrorSink) {
        self.common.width = self.width.unwrap_or_else(|| {
            let err = ParseError::new("Missing key .width");
            for &span in &self.defs {
                error.report_error(err.clone().with_context(span));
            }
            None
        });
        self.common.height = self.height.unwrap_or_else(|| {
            let err = ParseError::new("Missing key .height");
            for &span in &self.defs {
                error.report_error(err.clone().with_context(span));
            }
            None
        });
        self.common.fov = self.fov.flatten();
        match &mut self.kind {
            CCVInner::Unknown => {}
            CCVInner::Frame { inner, new } => {
                match new {
                    Frame::NoSource => {
                        let err = ParseError::new("Missing key .path | .color");
                        for &span in &self.defs {
                            error.report_error(err.clone().with_context(span));
                        }
                    }
                    Frame::ColorStruct {
                        defs,
                        format,
                        bytes,
                        ..
                    } => 'errs: {
                        let err = ParseError::new(match (format.is_none(), bytes.is_none()) {
                            (true, false) => "Missing key .color.format",
                            (false, true) => "Missing key .color.bytes",
                            (true, true) => "Missing keys .color.format, .color.bytes",
                            _ => break 'errs,
                        });
                        for &span in &self.defs {
                            error.report_error(err.clone().with_context(span));
                        }
                    }
                    _ => {}
                }
                **inner = std::mem::take(new);
            }
            CCVInner::V4l(_) => {}
        }
    }
}
