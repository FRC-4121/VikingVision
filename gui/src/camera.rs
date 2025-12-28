use crate::dyn_elem::DynElemType;
use crate::edit::Edits;
use crate::visit::prelude::*;
use eframe::egui;
use toml_parser::decoder::ScalarKind;

pub struct Frame {}
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
    }
    fn visit(&mut self) -> Self::Visitor<'_> {
        CCVisitor {
            common: &mut self.common,
            kind: match &mut self.inner {
                Inner::Unknown => CCVInner::Unknown,
                Inner::Frame(inner) => CCVInner::Frame(inner),
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
            0 => self.inner = Inner::Frame(Frame {}),
            1 => self.inner = Inner::V4l(V4l {}),
            _ => self.inner = Inner::Unknown,
        }
    }
}

enum CCVInner<'a> {
    Unknown,
    Frame(&'a mut Frame),
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
        let old = path.clone();
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
                        error.report_error(ParseError::new(format!(
                            "Expected an integer for key .width, got a {}",
                            scalar.kind.description()
                        )));
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
                        error.report_error(ParseError::new(format!(
                            "Expected an integer for key .width, got a {}",
                            scalar.kind.description()
                        )));
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
                        error.report_error(ParseError::new(format!(
                            "Expected a float for key .width, got a {}",
                            scalar.kind.description()
                        )));
                    }
                }
                "outputs" => {}
                _ => {
                    error.report_error(
                        ParseError::new(format!("Unknown key {old}")).with_context(k.span()),
                    );
                }
            }
        }
    }
    fn begin_array(&mut self, path: PathIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool {
        true
    }
    fn end_array(
        &mut self,
        path: PathIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
    }
    fn begin_table(&mut self, path: PathIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool {
        true
    }
    fn end_table(
        &mut self,
        path: PathIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
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
    }
}
