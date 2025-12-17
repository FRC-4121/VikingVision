use crate::visit::prelude::*;
use eframe::egui;
use std::marker::PhantomData;

#[derive(Default)]
pub struct NtConfig {
    _priv: (),
}
impl NtConfig {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        ui.label("NetworkTables is unavailable in this build");
    }
    pub fn visitor(&mut self) -> NtVisitor<'_> {
        NtVisitor {
            _marker: PhantomData,
        }
    }
}
pub struct NtVisitor<'a> {
    _marker: PhantomData<&'a mut NtConfig>,
}
#[allow(unused_variables)]
impl<'i> Visitor<'i> for NtVisitor<'_> {
    fn accept_scalar(
        &mut self,
        path: RawsIter<'_, 'i>,
        scalar: ScalarInfo<'i>,
        error: &mut dyn ErrorSink,
    ) {
    }
    fn begin_array(&mut self, path: RawsIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool {
        true
    }
    fn end_array(
        &mut self,
        path: RawsIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
    }
    fn begin_table(&mut self, path: RawsIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool {
        true
    }
    fn end_table(
        &mut self,
        path: RawsIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
    }
    fn finish(&mut self, source: Source<'i>, error: &mut dyn ErrorSink) {}
}
