use std::fmt::{self, Debug, Display, Formatter};
use toml_parser::decoder::{Encoding, ScalarKind};
use toml_parser::parser::EventReceiver;
use toml_parser::{ErrorSink, ParseError, Raw, Source, Span};

pub mod log;
pub mod prelude {
    pub use super::{PathIter, PathKind, ScalarInfo, Visitor};
    pub use toml_parser::{ErrorSink, ParseError, Source, Span};
}

pub struct ScalarInfo<'i> {
    pub raw: Raw<'i>,
    pub kind: ScalarKind,
}

#[allow(unused_variables)]
pub trait Visitor<'i> {
    fn begin_def(&mut self, key: Span) {}
    fn end_def(&mut self, key: Span, value: Span) {}
    fn accept_scalar(
        &mut self,
        path: PathIter<'_, 'i>,
        scalar: ScalarInfo<'i>,
        error: &mut dyn ErrorSink,
    );
    fn begin_array(&mut self, path: PathIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool;
    fn end_array(
        &mut self,
        path: PathIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    );
    fn begin_table(&mut self, path: PathIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool;
    fn end_table(
        &mut self,
        path: PathIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    );
    fn finish(&mut self, source: Source<'i>, error: &mut dyn ErrorSink);
}

#[allow(unused_variables)]
impl<'i> Visitor<'i> for () {
    fn begin_def(&mut self, def: Span) {}
    fn accept_scalar(
        &mut self,
        path: PathIter<'_, 'i>,
        scalar: ScalarInfo<'i>,
        error: &mut dyn ErrorSink,
    ) {
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
    fn finish(&mut self, source: Source<'i>, error: &mut dyn ErrorSink) {}
}

#[derive(Clone, Copy)]
enum PathElemKind {
    Key,
    Table,
    InlineStart,
    ArrayStart,
}

#[derive(Clone, Copy)]
struct PathElem {
    span: Span,
    kind: PathElemKind,
}

pub struct Receiver<'a, 'i> {
    source: Source<'i>,
    visitor: &'a mut dyn Visitor<'i>,
    path: Vec<PathElem>,
    pending_aot_error: Option<Span>,
    table_def: Option<Span>,
    skip_depth: usize,
    outer_table_start: Option<Span>,
}
impl<'a, 'i> Receiver<'a, 'i> {
    pub fn new(source: Source<'i>, visitor: &'a mut dyn Visitor<'i>) -> Self {
        Self {
            source,
            visitor,
            path: Vec::new(),
            pending_aot_error: None,
            table_def: None,
            skip_depth: 0,
            outer_table_start: None,
        }
    }
    fn finish_line(&mut self, span: Span, error: &mut dyn ErrorSink) {
        self.array_table_close(span, error);
        self.std_table_close(span, error);
    }
    fn close_table(&mut self, span: Span, error: &mut dyn ErrorSink) {
        if let Some(start) = self.outer_table_start.take() {
            let value = start.append(span);
            let key = self
                .path
                .iter()
                .rfind(|v| matches!(v.kind, PathElemKind::Table))
                .unwrap()
                .span;
            while let Some(PathElem { kind, .. }) = self.path.last() {
                if self.path.len() <= self.skip_depth {
                    self.skip_depth = 0;
                }
                if self.skip_depth == 0 && matches!(kind, PathElemKind::Key) {
                    let path = PathIter {
                        source: self.source,
                        iter: self.path.iter(),
                    };
                    self.visitor.end_table(path, key, value, error);
                }
                self.path.pop();
            }
        }
    }
    fn close_keys(&mut self, span: Span, error: &mut dyn ErrorSink) {
        while let Some(PathElem { span: start, .. }) =
            self.path.pop_if(|e| matches!(e.kind, PathElemKind::Key))
        {
            if self.path.len() <= self.skip_depth {
                self.skip_depth = 0;
            }
            if self.skip_depth == 0
                && matches!(
                    self.path.last(),
                    Some(PathElem {
                        kind: PathElemKind::Key,
                        ..
                    })
                )
            {
                let path = PathIter {
                    source: self.source,
                    iter: self.path.iter(),
                };
                self.visitor.end_table(
                    path,
                    start,
                    self.path
                        .first()
                        .map_or(span, |start| start.span.append(span)),
                    error,
                );
            }
        }
    }
    pub fn finish(&mut self, error: &mut dyn ErrorSink) {
        let len = self.source.input().len();
        let span = Span::new_unchecked(len, len);
        self.finish_line(span, error);
        self.close_table(span, error);
        self.visitor.finish(self.source, error);
    }
}
impl EventReceiver for Receiver<'_, '_> {
    fn array_table_open(&mut self, span: Span, _error: &mut dyn ErrorSink) {
        self.pending_aot_error = Some(span);
        self.skip_depth = 1;
    }
    fn array_table_close(&mut self, span: Span, error: &mut dyn ErrorSink) {
        if let Some(start) = self.pending_aot_error.take() {
            error.report_error(
                ParseError::new("arrays of tables aren't supported")
                    .with_context(start.append(span)),
            );
        }
    }
    fn std_table_open(&mut self, span: Span, error: &mut dyn ErrorSink) {
        self.close_table(span.before(), error);
        self.skip_depth = 0;
        self.table_def = Some(span);
        self.outer_table_start = Some(span);
    }
    fn std_table_close(&mut self, span: Span, _error: &mut dyn ErrorSink) {
        if let Some(start) = self.table_def.take() {
            self.path.push(PathElem {
                span: start.append(span),
                kind: PathElemKind::Table,
            });
        }
    }
    fn inline_table_open(&mut self, span: Span, error: &mut dyn ErrorSink) -> bool {
        let len = self.path.len();
        self.path.push(PathElem {
            span,
            kind: PathElemKind::InlineStart,
        });
        let path = PathIter {
            source: self.source,
            iter: self.path[..len].iter(),
        };
        if self.skip_depth == 0 && !self.visitor.begin_table(path, error) {
            self.skip_depth = self.path.len();
        }
        true
    }
    fn inline_table_close(&mut self, end: Span, error: &mut dyn ErrorSink) {
        loop {
            match self.path.pop() {
                None => return,
                Some(PathElem {
                    span: start,
                    kind: PathElemKind::InlineStart,
                }) => {
                    let path = PathIter {
                        source: self.source,
                        iter: self.path.iter(),
                    };
                    let span = start.append(end);
                    if self.skip_depth == 0 {
                        let key = self
                            .path
                            .iter()
                            .rfind(|e| !matches!(e.kind, PathElemKind::Key))
                            .unwrap()
                            .span;
                        self.visitor.end_table(path, key, span, error);
                    }
                    if self.path.len() <= self.skip_depth {
                        self.skip_depth = 0;
                    }
                    self.close_keys(span, error);
                    return;
                }
                _ => {}
            }
        }
    }
    fn array_open(&mut self, span: Span, error: &mut dyn ErrorSink) -> bool {
        let len = self.path.len();
        self.path.push(PathElem {
            span,
            kind: PathElemKind::ArrayStart,
        });
        let path = PathIter {
            source: self.source,
            iter: self.path[..len].iter(),
        };
        if self.skip_depth == 0 && !self.visitor.begin_array(path, error) {
            self.skip_depth = self.path.len();
        }
        true
    }
    fn array_close(&mut self, end: Span, error: &mut dyn ErrorSink) {
        loop {
            match self.path.pop() {
                None => return,
                Some(PathElem {
                    span: start,
                    kind: PathElemKind::ArrayStart,
                }) => {
                    let path = PathIter {
                        source: self.source,
                        iter: self.path.iter(),
                    };
                    let span = start.append(end);
                    if self.skip_depth == 0 {
                        let key = self
                            .path
                            .iter()
                            .rfind(|e| matches!(e.kind, PathElemKind::Key))
                            .unwrap()
                            .span;
                        self.visitor.end_array(path, key, span, error);
                    }
                    if self.path.len() <= self.skip_depth {
                        self.skip_depth = 0;
                    }
                    self.close_keys(span, error);
                    return;
                }
                _ => {}
            }
        }
    }
    fn simple_key(&mut self, span: Span, kind: Option<Encoding>, error: &mut dyn ErrorSink) {
        let Some(raw) = self.source.input().get(span.start()..span.end()) else {
            return;
        };
        let key = Raw::new_unchecked(raw, kind, span);
        key.decode_key(&mut (), error);
        self.path.push(PathElem {
            span,
            kind: PathElemKind::Key,
        });
        let len = self.path.len();
        if self.skip_depth == 0 {
            let slice;
            if self.table_def.is_some() {
                slice = &*self.path;
            } else {
                slice = &self.path[..(len - 1)];
                if !matches!(
                    slice.last(),
                    Some(PathElem {
                        kind: PathElemKind::Key,
                        ..
                    })
                ) {
                    return;
                }
            }
            let path = PathIter {
                source: self.source,
                iter: slice.iter(),
            };
            if !self.visitor.begin_table(path, error) {
                self.skip_depth = len;
            }
        }
    }
    fn scalar(&mut self, span: Span, kind: Option<Encoding>, error: &mut dyn ErrorSink) {
        let Some(raw) = self.source.input().get(span.start()..span.end()) else {
            return;
        };
        let scalar = Raw::new_unchecked(raw, kind, span);
        let kind = scalar.decode_scalar(&mut (), error);
        if self.skip_depth == 0 {
            let path = PathIter {
                source: self.source,
                iter: self.path.iter(),
            };
            self.visitor
                .accept_scalar(path, ScalarInfo { raw: scalar, kind }, error);
        }
        self.close_keys(span, error);
    }
    fn newline(&mut self, span: Span, error: &mut dyn ErrorSink) {
        self.finish_line(span, error);
    }
}

/// The kind of an element in a path
#[derive(Debug, Clone, Copy)]
pub enum PathKind<'i> {
    /// A raw key
    Key(Raw<'i>),
    /// An element in an array
    Array,
}

#[derive(Clone)]
pub struct PathIter<'a, 'i> {
    source: Source<'i>,
    iter: std::slice::Iter<'a, PathElem>,
}
impl<'i> PathIter<'_, 'i> {
    pub fn source(&self) -> Source<'i> {
        self.source
    }
    pub fn to_toml_path(&self) -> TomlPath {
        TomlPath(self.iter.as_slice().to_vec())
    }
    pub fn raw_len(&self) -> usize {
        self.iter.len()
    }
}
impl<'i> Iterator for PathIter<'_, 'i> {
    type Item = PathKind<'i>;
    fn next(&mut self) -> Option<Self::Item> {
        self.iter.find_map(|e| match e.kind {
            PathElemKind::Key => self.source.get(e.span).map(PathKind::Key),
            PathElemKind::ArrayStart => Some(PathKind::Array),
            _ => None,
        })
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        (0, Some(self.iter.len()))
    }
}
impl Debug for PathIter<'_, '_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("RawsIter").finish_non_exhaustive()
    }
}
impl Display for PathIter<'_, '_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        for elem in self.clone() {
            match elem {
                PathKind::Key(k) => write!(f, ".{}", k.as_str())?,
                PathKind::Array => f.write_str("[_]")?,
            }
        }
        Ok(())
    }
}

#[derive(Clone)]
pub struct TomlPath(Vec<PathElem>);
impl TomlPath {
    pub fn iter<'i>(&self, source: Source<'i>) -> PathIter<'_, 'i> {
        PathIter {
            source,
            iter: self.0.iter(),
        }
    }
    pub fn raw_len(&self) -> usize {
        self.0.len()
    }
}
