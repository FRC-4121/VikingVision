use crate::edit::{Edits, format_string};
use crate::map;
use crate::visit::{TomlPath, prelude::*};
use eframe::egui;
use toml_parser::Span;
use toml_parser::decoder::{Encoding, ScalarKind};

pub trait DynElemType: map::MapElem {
    fn kinds() -> &'static [(&'static str, &'static str)];
    fn create(kind: usize) -> Self;
    fn cleanup(&mut self, edits: Option<&mut Edits>);
}

struct UnknownSpans(Vec<Span>);
impl UnknownSpans {
    fn clear_all(&self, edits: &mut Edits) {
        edits.delete_all(self.0.iter().copied());
    }
}

pub struct DynElemConfig<T> {
    kind: usize,
    kind_span: Option<(Span, Encoding)>,
    selected: Result<T, UnknownSpans>,
}
impl<T> Default for DynElemConfig<T> {
    fn default() -> Self {
        Self {
            kind: usize::MAX,
            kind_span: None,
            selected: Err(UnknownSpans(Vec::new())),
        }
    }
}
impl<T: DynElemType> map::MapElem for DynElemConfig<T> {
    type Visitor<'a>
        = DECVisitor<'a, T>
    where
        Self: 'a;
    fn add(&mut self, ui: &mut egui::Ui) -> bool {
        self.select(ui);
        self.selected.as_mut().is_ok_and(|c| c.add(ui))
    }
    fn finish(&mut self, new: &mut String) {
        new.push_str("type = ");
        new.push_str(&format_string(
            T::kinds()[self.kind].0,
            &mut Encoding::BasicString,
        ));
        new.push('\n');
        self.selected.as_mut().ok().unwrap().finish(new);
    }
    fn show(&mut self, ui: &mut egui::Ui, edits: &mut Edits) {
        if let Some(old) = self.select(ui) {
            match old {
                Ok(mut old) => old.cleanup(Some(edits)),
                Err(spans) => spans.clear_all(edits),
            }
        }
        if let Ok(inner) = &mut self.selected {
            inner.show(ui, edits);
        }
    }
    fn visit(&mut self) -> Self::Visitor<'_> {
        DECVisitor {
            parent: self,
            events: Vec::new(),
            visitor: VisitorPresence::Waiting,
            has_type: false,
        }
    }
}
impl<T: DynElemType> DynElemConfig<T> {
    fn select(&mut self, ui: &mut egui::Ui) -> Option<Result<T, UnknownSpans>> {
        let old = self.kind;
        let kinds = T::kinds();
        egui::ComboBox::new("elem-type", "Type").show_index(ui, &mut self.kind, kinds.len(), |i| {
            kinds.get(i).map_or("", |p| p.1)
        });
        (old != self.kind && self.kind < kinds.len())
            .then(|| std::mem::replace(&mut self.selected, Ok(T::create(self.kind))))
    }
}

enum Event {
    BeginDef {
        key: Span,
    },
    EndDef {
        key: Span,
        value: Span,
    },
    Scalar {
        path: TomlPath,
        span: Span,
        encoding: Option<Encoding>,
        kind: ScalarKind,
    },
    BeginArray {
        path: TomlPath,
    },
    EndArray {
        path: TomlPath,
        key: Span,
        value: Span,
    },
    BeginTable {
        path: TomlPath,
    },
    EndTable {
        path: TomlPath,
        key: Span,
        value: Span,
    },
}

enum VisitorPresence<T> {
    Waiting,
    Drop,
    Here(T),
}

pub struct DECVisitor<'a, T: DynElemType + 'a> {
    parent: &'a mut DynElemConfig<T>,
    events: Vec<Event>,
    visitor: VisitorPresence<T::Visitor<'a>>,
    has_type: bool,
}
impl<'i, T: DynElemType> Visitor<'i> for DECVisitor<'_, T> {
    fn begin_def(&mut self, key: Span) {
        match &mut self.visitor {
            VisitorPresence::Here(visitor) => visitor.begin_def(key),
            VisitorPresence::Waiting => {
                self.events.push(Event::BeginDef { key });
            }
            VisitorPresence::Drop => {
                self.has_type = false;
            }
        }
    }
    fn end_def(&mut self, key: Span, value: Span) {
        match &mut self.visitor {
            VisitorPresence::Here(visitor) => visitor.end_def(key, value),
            VisitorPresence::Waiting => {
                self.events.push(Event::EndDef { key, value });
            }
            VisitorPresence::Drop => {
                if !self.has_type
                    && let Err(spans) = &mut self.parent.selected
                {
                    spans.0.push(value);
                }
            }
        }
    }
    fn accept_scalar(
        &mut self,
        path: PathIter<'_, 'i>,
        scalar: ScalarInfo<'i>,
        error: &mut dyn ErrorSink,
    ) {
        let mut it = path.clone();
        if let Some(PathKind::Key(k)) = it.next()
            && it.next().is_none()
        {
            let is_type = k.as_str() == "type" || k.as_str() == "'type'" || {
                let mut s = String::new();
                k.decode_key(&mut s, &mut ());
                s == "type"
            };
            if is_type {
                if !matches!(self.visitor, VisitorPresence::Waiting) {
                    error.report_error(
                        ParseError::new("Duplicate key .type").with_context(k.span()),
                    );
                } else if scalar.kind == ScalarKind::String {
                    let mut s = String::new();
                    let _ = scalar.raw.decode_scalar(&mut s, &mut ());
                    let kinds = T::kinds();
                    if let Some(kind) = kinds.iter().position(|k| s == k.0) {
                        if kind != self.parent.kind
                            && let Ok(mut sel) =
                                std::mem::replace(&mut self.parent.selected, Ok(T::create(kind)))
                        {
                            sel.cleanup(None);
                        }
                        let Ok(sel) = &mut self.parent.selected else {
                            panic!("Valid kind but no selected element!")
                        };
                        let mut visitor = sel.visit();
                        // Don't do this at home, kids
                        // This is safe because:
                        // - the parent can't be moved while this visitor is live,
                        // - we can only access the visitor here if there isn't a visitor
                        // - when we finish, we only mutate it if we don't have a visitor
                        // - we don't access the parent anywhere else
                        self.visitor = VisitorPresence::Here(unsafe {
                            std::mem::transmute::<T::Visitor<'_>, T::Visitor<'_>>(visitor)
                        });
                    } else {
                        use std::fmt::Write;
                        let mut message = format!("Unknown type {s:?} out of ");
                        match kinds {
                            [] => message.push_str("no options"),
                            [(kind, _)] => drop(write!(message, "{kind:?}")),
                            [(first, _), rest @ ..] => {
                                let _ = write!(message, "{first:?}");
                                for (kind, _) in rest {
                                    let _ = write!(message, ", {kind:?}");
                                }
                            }
                        }
                        error
                            .report_error(ParseError::new(message).with_context(scalar.raw.span()));
                        self.parent.selected = Err(UnknownSpans(Vec::new()));
                        self.has_type = true;
                    }
                } else {
                    self.parent.kind = usize::MAX;
                    self.parent.kind_span = Some((scalar.raw.span(), Encoding::BasicString));
                    error.report_error(
                        ParseError::new(format!(
                            "Expected a string for .type, got a {}",
                            scalar.kind.description()
                        ))
                        .with_context(scalar.raw.span()),
                    );
                }
                return;
            }
        }
        match &mut self.visitor {
            VisitorPresence::Here(visitor) => visitor.accept_scalar(path, scalar, error),
            VisitorPresence::Waiting => self.events.push(Event::Scalar {
                path: path.to_toml_path(),
                span: scalar.raw.span(),
                encoding: scalar.raw.encoding(),
                kind: scalar.kind,
            }),
            VisitorPresence::Drop => {
                // TODO: figure out how to track spans here
            }
        }
    }
    fn begin_array(&mut self, path: PathIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool {
        match &mut self.visitor {
            VisitorPresence::Here(visitor) => visitor.begin_array(path, error),
            VisitorPresence::Waiting => {
                self.events.push(Event::BeginArray {
                    path: path.to_toml_path(),
                });
                true
            }
            VisitorPresence::Drop => false,
        }
    }
    fn end_array(
        &mut self,
        path: PathIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
        match &mut self.visitor {
            VisitorPresence::Here(visitor) => visitor.end_array(path, key, value, error),
            VisitorPresence::Waiting => {
                self.events.push(Event::EndArray {
                    path: path.to_toml_path(),
                    key,
                    value,
                });
            }
            VisitorPresence::Drop => {
                if let Err(spans) = &mut self.parent.selected {
                    spans.0.push(value);
                }
            }
        }
    }
    fn begin_table(&mut self, path: PathIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool {
        match &mut self.visitor {
            VisitorPresence::Here(visitor) => visitor.begin_table(path, error),
            VisitorPresence::Waiting => {
                self.events.push(Event::BeginTable {
                    path: path.to_toml_path(),
                });
                true
            }
            VisitorPresence::Drop => false,
        }
    }
    fn end_table(
        &mut self,
        path: PathIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
        match &mut self.visitor {
            VisitorPresence::Here(visitor) => visitor.end_table(path, key, value, error),
            VisitorPresence::Waiting => {
                self.events.push(Event::EndTable {
                    path: path.to_toml_path(),
                    key,
                    value,
                });
            }
            VisitorPresence::Drop => {
                if let Err(spans) = &mut self.parent.selected {
                    spans.0.push(value);
                }
            }
        }
    }
    fn finish(&mut self, source: Source<'i>, error: &mut dyn ErrorSink) {
        match std::mem::replace(&mut self.visitor, VisitorPresence::Waiting) {
            VisitorPresence::Here(mut visitor) => {
                visitor.finish(source, error);
            }
            VisitorPresence::Waiting => {
                let err = ParseError::new("Missing key .type");
                for event in self.events.drain(..) {
                    if let Event::BeginDef { key } = event {
                        error.report_error(err.clone().with_context(key));
                    }
                }
            }
            VisitorPresence::Drop => {}
        }
    }
}
