use crate::edit::{Edits, format_key};
use crate::visit::prelude::*;
use eframe::egui;
use std::collections::HashMap;
use toml_parser::decoder::Encoding;

pub trait MapElem: Default {
    type Visitor<'a>: for<'i> Visitor<'i> + 'a
    where
        Self: 'a;
    fn add(&mut self, ui: &mut egui::Ui) -> bool;
    fn finish(&mut self, new: &mut String);
    fn show(&mut self, ui: &mut egui::Ui, edits: &mut Edits);
    fn visit(&mut self) -> Self::Visitor<'_>;
}

impl MapElem for () {
    type Visitor<'a>
        = ()
    where
        Self: 'a;
    fn add(&mut self, _ui: &mut egui::Ui) -> bool {
        true
    }
    fn finish(&mut self, _new: &mut String) {}
    fn show(&mut self, _ui: &mut egui::Ui, _edits: &mut Edits) {}
    fn visit(&mut self) -> Self::Visitor<'_> {}
}

#[derive(Default)]
pub struct ElemData<T> {
    pub elem: T,
    name_spans: Vec<(Span, Option<Encoding>)>,
    val_spans: Vec<Span>,
}

/// A wrapper around an element and its visitor that ensures that the element isn't accessed
struct Visiting<'a, T: MapElem + 'a> {
    elem: Box<ElemData<T>>,
    visitor: T::Visitor<'a>,
    index: usize,
}
impl<'a, T: MapElem + 'a> Visiting<'a, T> {
    fn new(mut elem: Box<ElemData<T>>, index: usize) -> Self {
        let visitor = unsafe { std::mem::transmute::<&mut T, &'a mut T>(&mut elem.elem).visit() };
        Self {
            elem,
            visitor,
            index,
        }
    }
    fn visitor(&mut self) -> &mut T::Visitor<'_> {
        unsafe {
            std::mem::transmute::<&mut T::Visitor<'a>, &mut T::Visitor<'_>>(&mut self.visitor)
        }
    }
    fn into_inner(self) -> (Box<ElemData<T>>, usize) {
        (self.elem, self.index)
    }
}

pub struct MapConfig<T> {
    prefix: &'static str,
    elems: Vec<(String, Box<ElemData<T>>)>,
    names: Vec<String>,
    adding: Option<(String, T)>,
}
impl<T: MapElem> MapConfig<T> {
    pub fn new(prefix: &'static str) -> Self {
        Self {
            prefix,
            elems: Vec::new(),
            names: Vec::new(),
            adding: None,
        }
    }
    pub fn elems(&self) -> &[(String, Box<ElemData<T>>)] {
        &self.elems
    }
    pub fn show(&mut self, ui: &mut egui::Ui, edits: &mut Edits) {
        {
            let new = ui.add_enabled(self.adding.is_none(), egui::Button::new("New"));
            if new.clicked() {
                self.adding = Some((String::new(), Default::default()));
            }
            let mut finish = false;
            let mut cancel = false;
            if let Some((name, state)) = &mut self.adding {
                egui::Popup::from_response(&new)
                    .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                    .show(|ui| {
                        let mut valid = false;
                        ui.horizontal(|ui| {
                            ui.label("Name");
                            let edit = ui.text_edit_singleline(name);
                            let mut tooltip = egui::Tooltip::for_widget(&edit);
                            tooltip.popup = tooltip.popup.frame(
                                egui::Frame::popup(ui.style())
                                    .fill(ui.style().visuals.extreme_bg_color),
                            );
                            if name.is_empty() {
                                tooltip.show(|ui| {
                                    ui.label(
                                        egui::RichText::new("Name cannot be empty")
                                            .color(ui.style().visuals.error_fg_color),
                                    );
                                });
                            } else if self.names.contains(name) {
                                tooltip.show(|ui| {
                                    ui.label(
                                        egui::RichText::new(format!("Duplicate name {name:?}"))
                                            .color(ui.style().visuals.error_fg_color),
                                    );
                                });
                            } else {
                                valid = true;
                            }
                        });
                        valid &= state.add(ui);
                        ui.horizontal(|ui| {
                            finish = ui.add_enabled(valid, egui::Button::new("Finish")).clicked();
                            cancel = ui.button("Cancel").clicked();
                        });
                    });
            }
            if finish {
                let (name, mut state) = self.adding.take().unwrap();
                let mut encoding = None;
                let mut new = format!(
                    "\n\n[{}.{}]\n",
                    self.prefix,
                    format_key(&name, &mut encoding)
                );
                let end = edits.end();
                let name_start = end + 4 + self.prefix.len();
                let name_end = name_start + name.len();
                state.finish(&mut new);
                let len = new.len();
                edits.insert(end, new);
                self.elems.push((
                    name,
                    Box::new(ElemData {
                        elem: state,
                        name_spans: vec![(Span::new_unchecked(name_start, name_end), encoding)],
                        val_spans: vec![Span::new_unchecked(end, end + len)],
                    }),
                ));
            }
            if cancel {
                self.adding = None;
            }
        }
        egui::ScrollArea::vertical().show(ui, |ui| {
            self.elems.retain_mut(|(name, elem)| {
                let mut keep = true;
                egui::Frame::group(ui.style()).show(ui, |ui| {
                    ui.set_min_width(200.0);
                    ui.horizontal(|ui| {
                        ui.heading(&*name);
                        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
                            keep = !ui.button("Delete").clicked();
                            let rename = ui.button("Rename");
                            egui::Popup::from_toggle_button_response(&rename)
                                .close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside)
                                .show(|ui| {
                                    let mut update = false;
                                    ui.horizontal(|ui| {
                                        ui.label("Name");
                                        let edit = ui.text_edit_singleline(name);
                                        let mut tooltip = egui::Tooltip::for_widget(&edit);
                                        tooltip.popup = tooltip.popup.frame(
                                            egui::Frame::popup(ui.style())
                                                .fill(ui.style().visuals.extreme_bg_color),
                                        );
                                        if name.is_empty() {
                                            tooltip.show(|ui| {
                                                ui.label(
                                                    egui::RichText::new("Name cannot be empty")
                                                        .color(ui.style().visuals.error_fg_color),
                                                );
                                            });
                                        } else if self.names.contains(name) {
                                            tooltip.show(|ui| {
                                                ui.label(
                                                    egui::RichText::new(format!(
                                                        "Duplicate name {name:?}"
                                                    ))
                                                    .color(ui.style().visuals.error_fg_color),
                                                );
                                            });
                                        } else {
                                            update = edit.changed();
                                        }
                                    });
                                    if update {
                                        for (span, encoding) in &mut elem.name_spans {
                                            edits.replace(*span, format_key(name, encoding));
                                        }
                                    }
                                });
                        })
                    });
                    elem.elem.show(ui, edits);
                });
                if !keep {
                    edits.delete_all(elem.val_spans.drain(..));
                }
                keep
            });
        });
    }
    pub fn visit(&mut self) -> MapVisitor<'_, T> {
        MapVisitor {
            cfg: self,
            visitors: HashMap::new(),
            curr_idx: 0,
        }
    }
}
pub struct MapVisitor<'a, T: MapElem> {
    cfg: &'a mut MapConfig<T>,
    visitors: HashMap<String, Visiting<'a, T>>,
    curr_idx: usize,
}
impl<'i, T: MapElem> Visitor<'i> for MapVisitor<'_, T> {
    fn accept_scalar(
        &mut self,
        mut path: RawsIter<'_, 'i>,
        scalar: ScalarInfo<'i>,
        error: &mut dyn ErrorSink,
    ) {
        let old = path.clone();
        if let Some(PathKind::Key(k)) = path.next()
            && path.clone().next().is_some()
        {
            self.visitors
                .get_mut(k.as_str())
                .expect("This should've been inserted through a call to begin_table already")
                .visitor()
                .accept_scalar(path, scalar, error);
        } else {
            error.report_error(
                ParseError::new(format!("Unexpected scalar at {old}"))
                    .with_context(scalar.raw.span()),
            );
        }
    }
    fn begin_array(&mut self, mut path: RawsIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool {
        if let Some(PathKind::Key(k)) = path.next()
            && path.clone().next().is_some()
        {
            self.visitors
                .get_mut(k.as_str())
                .expect("This should've been inserted through a call to begin_table already")
                .visitor()
                .begin_array(path, error)
        } else {
            false
        }
    }
    fn end_array(
        &mut self,
        mut path: RawsIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
        let old = path.clone();
        if let Some(PathKind::Key(k)) = path.next()
            && path.clone().next().is_some()
        {
            self.visitors
                .get_mut(k.as_str())
                .expect("This should've been inserted through a call to begin_table already")
                .visitor()
                .end_array(path, key, value, error);
        } else {
            error.report_error(
                ParseError::new(format!("Unexpected array at {old}")).with_context(value),
            );
        }
    }
    fn begin_table(&mut self, mut path: RawsIter<'_, 'i>, error: &mut dyn ErrorSink) -> bool {
        if let Some(PathKind::Key(k)) = path.next() {
            if path.clone().next().is_some() {
                self.visitors
                    .get_mut(k.as_str())
                    .expect("This should've been inserted through a call to begin_table already")
                    .visitor()
                    .begin_table(path, error)
            } else {
                let mut s = String::new();
                k.decode_key(&mut s, &mut ());
                let visit = self.visitors.entry(s).or_insert_with_key(|name| {
                    let elems = &mut self.cfg.elems;
                    let elem =
                        elems
                            .iter()
                            .position(|x| x.0 == *name)
                            .map_or_else(Box::default, |idx| {
                                let mut elem = elems.remove(idx).1;
                                elem.name_spans.clear();
                                elem.val_spans.clear();
                                elem
                            });
                    let index = self.curr_idx;
                    self.curr_idx += 1;
                    Visiting::new(elem, index)
                });
                visit.elem.name_spans.push((k.span(), k.encoding()));
                visit.visitor().begin_def(k.span());
                true
            }
        } else {
            false
        }
    }
    fn end_table(
        &mut self,
        mut path: RawsIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
        let old = path.clone();
        if let Some(PathKind::Key(k)) = path.next() {
            let visitor = self
                .visitors
                .get_mut(k.as_str())
                .expect("This should've been inserted through a call to begin_table already");
            if path.clone().next().is_some() {
                visitor.visitor().end_table(path, key, value, error);
            } else {
                visitor.elem.val_spans.push(value);
                visitor.visitor().end_def(key, value);
            }
        } else {
            error.report_error(
                ParseError::new(format!("Unexpected array at {old}")).with_context(value),
            );
        }
    }
    fn finish(&mut self, source: Source<'i>, error: &mut dyn ErrorSink) {
        for visitor in self.visitors.values_mut() {
            visitor.visitor().finish(source, error);
        }
        let len = self.visitors.len();
        self.cfg.elems.clear();
        self.cfg.elems.reserve(len);
        let cap = self.cfg.elems.spare_capacity_mut();
        for (name, visitor) in self.visitors.drain() {
            let (elem, index) = visitor.into_inner();
            cap[index].write((name, elem));
        }
        unsafe {
            self.cfg.elems.set_len(len);
        }
        self.curr_idx = 0;
    }
}
