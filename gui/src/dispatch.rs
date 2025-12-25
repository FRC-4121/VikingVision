use crate::map::MapVisitor;
use crate::nt::NtVisitor;
use crate::visit::prelude::*;

pub struct DispatchVisitor<'a> {
    pub ntable: NtVisitor<'a>,
    pub cameras: MapVisitor<'a, ()>,
    pub components: MapVisitor<'a, ()>,
}
impl<'i> Visitor<'i> for DispatchVisitor<'_> {
    fn accept_scalar(
        &mut self,
        mut path: PathIter<'_, 'i>,
        scalar: ScalarInfo<'i>,
        error: &mut dyn ErrorSink,
    ) {
        let old = path.clone();
        if let Some(PathKind::Key(k)) = path.next()
            && path.clone().next().is_some()
        {
            match k.as_str() {
                "ntable" => self.ntable.accept_scalar(path, scalar, error),
                "camera" | "cameras" => self.cameras.accept_scalar(path, scalar, error),
                "component" | "components" => self.components.accept_scalar(path, scalar, error),
                _ => {
                    error.report_error(
                        ParseError::new(format!("Unexpected scalar at {old}"))
                            .with_context(scalar.raw.span()),
                    );
                }
            }
        } else {
            error.report_error(
                ParseError::new(format!("Unexpected scalar at {old}"))
                    .with_context(scalar.raw.span()),
            );
        }
    }
    fn begin_array(&mut self, mut path: PathIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool {
        if let Some(PathKind::Key(k)) = path.next() {
            path.clone().next().is_some()
                && match k.as_str() {
                    "ntable" => self.ntable.begin_array(path, error),
                    "camera" | "cameras" => self.cameras.begin_array(path, error),
                    "component" | "components" => self.components.begin_array(path, error),
                    _ => false,
                }
        } else {
            false
        }
    }
    fn begin_table(&mut self, mut path: PathIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool {
        if let Some(PathKind::Key(k)) = path.next() {
            match k.as_str() {
                "ntable" => {
                    if path.clone().next().is_none() {
                        self.ntable.begin_def(k.span());
                        true
                    } else {
                        self.ntable.begin_table(path, error)
                    }
                }
                "camera" | "cameras" => {
                    if path.clone().next().is_none() {
                        self.cameras.begin_def(k.span());
                        true
                    } else {
                        self.cameras.begin_table(path, error)
                    }
                }
                "component" | "components" => {
                    if path.clone().next().is_none() {
                        self.components.begin_def(k.span());
                        true
                    } else {
                        self.components.begin_table(path, error)
                    }
                }
                _ => false,
            }
        } else {
            false
        }
    }
    fn end_array(
        &mut self,
        mut path: PathIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
        let old = path.clone();
        if let Some(PathKind::Key(k)) = path.next() {
            match k.as_str() {
                "ntable" => {
                    if path.clone().next().is_none() {
                        error.report_error(
                            ParseError::new("Expected a table for key .ntable").with_context(value),
                        );
                    } else {
                        self.ntable.end_array(path, key, value, error);
                    }
                }
                "camera" | "cameras" => {
                    if path.clone().next().is_none() {
                        error.report_error(
                            ParseError::new(format!("Expected a table for key .{}", k.as_str()))
                                .with_context(value),
                        );
                    } else {
                        self.cameras.end_array(path, key, value, error);
                    }
                }
                "component" | "components" => {
                    if path.clone().next().is_none() {
                        error.report_error(
                            ParseError::new(format!("Expected a table for key .{}", k.as_str()))
                                .with_context(value),
                        );
                    } else {
                        self.components.end_array(path, key, value, error);
                    }
                }
                _ => error.report_error(
                    ParseError::new(format!("Unexpected array at {old}")).with_context(key),
                ),
            }
        } else {
            unreachable!()
        }
    }
    fn end_table(
        &mut self,
        mut path: PathIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
        let old = path.clone();
        if let Some(PathKind::Key(k)) = path.next() {
            match k.as_str() {
                "ntable" => {
                    if path.clone().next().is_none() {
                        self.ntable.end_def(key, value);
                    } else {
                        self.ntable.end_table(path, key, value, error);
                    }
                }
                "camera" | "cameras" => {
                    if path.clone().next().is_none() {
                        self.cameras.end_def(key, value);
                    } else {
                        self.cameras.end_table(path, key, value, error);
                    }
                }
                "component" | "components" => {
                    if path.clone().next().is_none() {
                        self.components.end_def(key, value);
                    } else {
                        self.components.end_table(path, key, value, error);
                    }
                }
                _ => error.report_error(
                    ParseError::new(format!("Unexpected table at {old}")).with_context(key),
                ),
            }
        } else {
            unreachable!()
        }
    }
    fn finish(&mut self, source: Source<'i>, error: &mut dyn ErrorSink) {
        self.ntable.finish(source, error);
        self.cameras.finish(source, error);
        self.components.finish(source, error);
    }
}
