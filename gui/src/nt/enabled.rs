use crate::visit::prelude::*;
use eframe::egui;
use ntable::team::{TeamNumber, TeamParseError};
use toml_parser::decoder::Encoding;

enum Host {
    Team {
        team: TeamNumber,
        edit: String,
        err: Option<TeamParseError>,
    },
    Host(String, Encoding),
}

struct PresentNt {
    host: Host,
    identity: String,
    host_span: Span,
    id_span: Span,
    id_enc: Encoding,
}

#[derive(Default)]
pub struct NtConfig {
    inner: Option<PresentNt>,
}
impl NtConfig {
    pub fn show(&mut self, ui: &mut egui::Ui) {
        if let Some(inner) = &mut self.inner {
            ui.text_edit_singleline(&mut inner.identity);
            let mut is_host = matches!(inner.host, Host::Host(..));
            egui::ComboBox::new("nt-host-kind", "Host Kind")
                .selected_text(if is_host { "Hostname" } else { "Team Number" })
                .show_ui(ui, |ui| {
                    ui.selectable_value(&mut is_host, true, "Hostname");
                    ui.selectable_value(&mut is_host, false, "Team Number");
                });
            let mut changed = false;
            match (&inner.host, is_host) {
                (Host::Team { team, .. }, true) => {
                    changed = true;
                    inner.host = Host::Host(team.to_ipv4().to_string(), Encoding::BasicString);
                }
                (Host::Host(hostname, _), false) => {
                    changed = true;
                    if let Some(team) = TeamNumber::parse_ipv4(hostname) {
                        inner.host = Host::Team {
                            team,
                            edit: team.to_string(),
                            err: None,
                        };
                    } else {
                        inner.host = Host::Team {
                            team: TeamNumber::new_unchecked(0),
                            edit: "0".to_string(),
                            err: None,
                        };
                    }
                }
                _ => {}
            }
            match &mut inner.host {
                Host::Team { team, edit, err } => {
                    let resp = ui.text_edit_singleline(edit);
                    if resp.changed() {
                        match edit.parse() {
                            Ok(t) => {
                                changed = true;
                                *team = t;
                                *err = None;
                            }
                            Err(e) => {
                                *err = Some(e);
                            }
                        }
                    }
                }
                Host::Host(hostname, _) => {
                    changed |= ui.text_edit_singleline(hostname).changed();
                }
            }
            let _ = changed;
        } else {
            ui.label("NetworkTables not present in this file!");
            let _ = ui.button("Add to file");
        }
    }
    pub fn visitor(&mut self) -> NtVisitor<'_> {
        NtVisitor { nt: self }
    }
}
pub struct NtVisitor<'a> {
    nt: &'a mut NtConfig,
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
