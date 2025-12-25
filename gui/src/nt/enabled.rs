use crate::edit::{Edits, format_string};
use crate::visit::prelude::*;
use eframe::egui;
use ntable::team::{TeamNumber, TeamParseError};
use toml_parser::decoder::{Encoding, ScalarKind};

enum HostKind {
    Team(Option<TeamNumber>),
    Host(String, Encoding),
}

struct Identity {
    identity: String,
    id_span: Span,
    id_enc: Encoding,
}

struct Host {
    kind: HostKind,
    span: Span,
    path: Span,
}

#[derive(Default)]
struct PresentNt {
    host: Option<Host>,
    identity: Option<Identity>,
    spans: Vec<Span>,
}

#[derive(Default)]
pub struct NtConfig {
    inner: Option<PresentNt>,
    host_edit: String,
    host_err: Option<TeamParseError>,
    last_team: Option<TeamNumber>,
}
impl NtConfig {
    pub fn show(&mut self, ui: &mut egui::Ui, edits: &mut Edits) {
        if let Some(inner) = &mut self.inner {
            if ui.button("Delete").clicked() {
                edits.extend(inner.spans.drain(..).map(|s| (s, "")));
                return;
            }
            if let Some(id) = &mut inner.identity {
                ui.horizontal(|ui| {
                    ui.label("Identity: ");
                    if ui.text_edit_singleline(&mut id.identity).changed() {
                        edits.add(id.id_span, format_string(&id.identity, &mut id.id_enc));
                    }
                });
            } else {
                ui.horizontal(|ui| {
                    ui.label("Identity not present!");
                    if ui.button("Add").clicked() {
                        edits.add(
                            Span::new_unchecked(0, 0),
                            "ntable.identity = \"vv-client\"\n",
                        );
                    }
                });
            }
            if let Some(host) = &mut inner.host {
                let mut host_changed = false;
                let mut is_host = matches!(host.kind, HostKind::Host(..));
                egui::ComboBox::new("nt-host-kind", "Host")
                    .selected_text(if is_host { "Hostname" } else { "Team Number" })
                    .show_ui(ui, |ui| {
                        ui.selectable_value(&mut is_host, true, "Hostname");
                        ui.selectable_value(&mut is_host, false, "Team Number");
                    });
                match (&host.kind, is_host) {
                    (HostKind::Team(Some(team)), true) => {
                        host_changed = true;
                        host.kind =
                            HostKind::Host(team.to_ipv4().to_string(), Encoding::BasicString);
                        edits.add(host.path, "host");
                    }
                    (HostKind::Team(None), true) => {
                        host_changed = true;
                        host.kind = HostKind::Host("localhost".to_string(), Encoding::BasicString);
                        edits.add(host.path, "host");
                    }
                    (HostKind::Host(hostname, _), false) => {
                        host_changed = true;
                        if let Some(team) = TeamNumber::parse_ipv4(hostname) {
                            self.host_edit = team.to_string();
                            self.host_err = None;
                            host.kind = HostKind::Team(Some(team));
                        } else {
                            self.host_edit = "0".to_string();
                            self.host_err = None;
                            host.kind = HostKind::Team(Some(TeamNumber::new_unchecked(0)));
                        }
                        edits.add(host.path, "team");
                    }
                    _ => {}
                }
                match &mut host.kind {
                    HostKind::Team(team) => {
                        let resp = ui.text_edit_singleline(&mut self.host_edit);
                        if resp.changed() {
                            match self.host_edit.parse() {
                                Ok(t) => {
                                    host_changed = true;
                                    *team = Some(t);
                                    self.host_err = None;
                                }
                                Err(e) => {
                                    self.host_err = Some(e);
                                }
                            }
                        }
                    }
                    HostKind::Host(hostname, _) => {
                        host_changed |= ui.text_edit_singleline(hostname).changed();
                    }
                }
                if host_changed {
                    edits.add(
                        host.span,
                        match host.kind {
                            HostKind::Host(ref hn, ref mut enc) => format_string(hn, enc),
                            HostKind::Team(team) => team.unwrap().to_string(),
                        },
                    );
                }
            } else {
                ui.horizontal(|ui| {
                    ui.label("Identity not present!");
                    if ui.button("Add").clicked() {
                        edits.add(Span::new_unchecked(0, 0), "ntable.host = \"localhost\"\n");
                    }
                });
            }
        } else {
            ui.label("NetworkTables not present in this file!");
            if ui.button("Add to file").clicked() {
                edits.add(
                    Span::new_unchecked(0, 0),
                    "[ntable]\nidentity = \"vv-client\"\nhost = \"localhost\"\n",
                );
            }
        }
    }
    pub fn visitor(&mut self) -> NtVisitor<'_> {
        self.inner = None;
        NtVisitor {
            nt: self,
            spans: Vec::new(),
        }
    }
}
pub struct NtVisitor<'a> {
    nt: &'a mut NtConfig,
    spans: Vec<Span>,
}
impl<'i> Visitor<'i> for NtVisitor<'_> {
    fn begin_def(&mut self, key: Span) {
        self.nt.inner.get_or_insert_default();
        self.spans.push(key);
    }
    fn end_def(&mut self, _key: Span, value: Span) {
        self.nt.inner.get_or_insert_default().spans.push(value);
    }
    fn accept_scalar(
        &mut self,
        path: RawsIter<'_, 'i>,
        scalar: ScalarInfo<'i>,
        error: &mut dyn ErrorSink,
    ) {
        let present = self.nt.inner.get_or_insert_default();
        match path.clone().next() {
            Some(PathKind::Key(k)) => match k.as_str() {
                "identity" => {
                    if present.identity.is_some() {
                        error.report_error(
                            ParseError::new("Duplicate key .ntable.identity")
                                .with_context(k.span()),
                        );
                    } else if scalar.kind == ScalarKind::String {
                        let mut id = String::new();
                        let _ = scalar.raw.decode_scalar(&mut id, &mut ());
                        present.identity = Some(Identity {
                            identity: id,
                            id_span: scalar.raw.span(),
                            id_enc: scalar.raw.encoding().unwrap_or(Encoding::BasicString),
                        });
                    } else {
                        present.identity = Some(Identity {
                            identity: String::new(),
                            id_span: scalar.raw.span(),
                            id_enc: Encoding::BasicString,
                        });
                    }
                    if scalar.kind != ScalarKind::String {
                        error.report_error(
                            ParseError::new(format!(
                                "Expected a string for key .ntable.identity, got {}",
                                scalar.kind.description()
                            ))
                            .with_context(scalar.raw.span()),
                        );
                    }
                }
                "host" => {
                    match present.host {
                        Some(Host {
                            kind: HostKind::Host(..),
                            ..
                        }) => {
                            error.report_error(
                                ParseError::new("Duplicate key .ntable.host")
                                    .with_context(k.span()),
                            );
                        }
                        Some(Host {
                            kind: HostKind::Team { .. },
                            ..
                        }) => {
                            error.report_error(
                                ParseError::new("Key .ntable.host conflicts with .ntable.team")
                                    .with_context(k.span()),
                            );
                        }
                        None => {
                            if scalar.kind == ScalarKind::String {
                                let mut host = String::new();
                                let _ = scalar.raw.decode_scalar(&mut host, &mut ());
                                present.host = Some(Host {
                                    kind: HostKind::Host(
                                        host,
                                        scalar.raw.encoding().unwrap_or(Encoding::BasicString),
                                    ),
                                    span: scalar.raw.span(),
                                    path: k.span(),
                                });
                            } else {
                                present.host = Some(Host {
                                    kind: HostKind::Host(
                                        "localhost".to_string(),
                                        Encoding::BasicString,
                                    ),
                                    span: scalar.raw.span(),
                                    path: k.span(),
                                });
                            }
                        }
                    }
                    if scalar.kind != ScalarKind::String {
                        error.report_error(
                            ParseError::new(format!(
                                "Expected a string for key .ntable.host, got {}",
                                scalar.kind.description()
                            ))
                            .with_context(scalar.raw.span()),
                        );
                    }
                }
                "team" => {
                    match present.host {
                        Some(Host {
                            kind: HostKind::Host(..),
                            ..
                        }) => {
                            error.report_error(
                                ParseError::new("Key .ntable.team conflicts with .ntable.host")
                                    .with_context(k.span()),
                            );
                        }
                        Some(Host {
                            kind: HostKind::Team { .. },
                            ..
                        }) => {
                            error.report_error(
                                ParseError::new("Duplicate key .ntable.team")
                                    .with_context(k.span()),
                            );
                        }
                        None => {
                            if scalar.kind
                                == ScalarKind::Integer(toml_parser::decoder::IntegerRadix::Dec)
                            {
                                let mut team_str = String::new();
                                let _ = scalar.raw.decode_scalar(&mut team_str, &mut ());
                                let team = team_str.parse().ok();
                                if team.is_some() && team != self.nt.last_team {
                                    self.nt.last_team = team;
                                    self.nt.host_edit = team_str;
                                }
                                present.host = Some(Host {
                                    kind: HostKind::Team(team),
                                    span: scalar.raw.span(),
                                    path: k.span(),
                                });
                            } else {
                                present.host = Some(Host {
                                    kind: HostKind::Team(None),
                                    span: scalar.raw.span(),
                                    path: k.span(),
                                });
                            }
                        }
                    }
                    if scalar.kind != ScalarKind::Integer(toml_parser::decoder::IntegerRadix::Dec) {
                        error.report_error(
                            ParseError::new(format!(
                                "Expected a integer for key .ntable.team, got {}",
                                scalar.kind.description()
                            ))
                            .with_context(scalar.raw.span()),
                        );
                    }
                }
                _ => {
                    error.report_error(
                        ParseError::new(format!("Unexpected key .ntable{path}"))
                            .with_context(scalar.raw.span()),
                    );
                }
            },
            _ => unreachable!("Only a table should be passed to this visitor"),
        }
    }
    fn begin_array(&mut self, _path: RawsIter<'_, 'i>, _error: &mut dyn ErrorSink) -> bool {
        false
    }
    fn end_array(
        &mut self,
        path: RawsIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
        match path.clone().next() {
            Some(PathKind::Key(k)) if ["identity", "host", "team"].contains(&k.as_str()) => {
                error.report_error(
                    ParseError::new(format!(
                        "Expected a scalar for value .ntable.{}, got an array",
                        k.as_str(),
                    ))
                    .with_context(value),
                );
            }
            _ => {
                error.report_error(
                    ParseError::new(format!("Unexpected key .ntable{path}")).with_context(key),
                );
            }
        }
    }
    fn begin_table(&mut self, _path: RawsIter<'_, 'i>, _error: &mut dyn ErrorSink) -> bool {
        false
    }
    fn end_table(
        &mut self,
        path: RawsIter<'_, 'i>,
        key: Span,
        value: Span,
        error: &mut dyn ErrorSink,
    ) {
        match path.clone().next() {
            Some(PathKind::Key(k)) if ["identity", "host", "team"].contains(&k.as_str()) => {
                error.report_error(
                    ParseError::new(format!(
                        "Expected a scalar for value .ntable.{}, got a table",
                        k.as_str(),
                    ))
                    .with_context(value),
                );
            }
            _ => {
                error.report_error(
                    ParseError::new(format!("Unexpected key .ntable{path}")).with_context(key),
                );
            }
        }
    }
    fn finish(&mut self, _source: Source<'i>, error: &mut dyn ErrorSink) {
        'missing_keys: {
            if !self.spans.is_empty()
                && let Some(present) = &self.nt.inner
            {
                let msg = match (present.host.is_none(), present.identity.is_none()) {
                    (true, true) => "Missing keys: (.ntable.host | .ntable.team), .ntable.identity",
                    (false, true) => "Missing key: .ntable.identity",
                    (true, false) => "Missing key: .ntable.host | .ntable.team",
                    (false, false) => break 'missing_keys,
                };
                for span in self.spans.drain(..) {
                    error.report_error(ParseError::new(msg).with_context(span));
                }
            }
        }
    }
}
