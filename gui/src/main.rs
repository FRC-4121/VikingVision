use crate::trace::ToColor32;
use eframe::egui::containers::menu::MenuConfig;
use eframe::{App, CreationContext, egui};
use std::io;
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;

mod camera;
mod dispatch;
mod dyn_elem;
mod edit;
mod editor;
mod map;
mod nt;
mod trace;
mod visit;

fn now() -> time::OffsetDateTime {
    time::OffsetDateTime::now_local()
        .ok()
        .unwrap_or_else(time::OffsetDateTime::now_utc)
}

struct VikingVision {
    editor: editor::EditorState,
    logs: trace::LogWidget,
    nt: nt::NtConfig,
    cameras: map::MapConfig<dyn_elem::DynElemConfig<camera::CameraConfig>>,
    components: map::MapConfig<()>,
}
impl VikingVision {
    fn new(ctx: &CreationContext, logs: trace::LogWidget) -> io::Result<Self> {
        let editor = editor::EditorState::load(ctx.storage);
        Ok(Self {
            editor,
            logs,
            nt: Default::default(),
            cameras: map::MapConfig::new("camera"),
            components: map::MapConfig::new("component"),
        })
    }
}
impl App for VikingVision {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        self.editor.poll_futures();
        egui::TopBottomPanel::top("toolbar").show(ctx, |ui| {
            egui::MenuBar::new()
                .config(
                    MenuConfig::new().close_behavior(egui::PopupCloseBehavior::CloseOnClickOutside),
                )
                .ui(ui, |ui| {
                    self.editor.file_menu(ui);
                    ui.menu_button("Debug", |ui| {
                        ui.menu_button("TOML Parsing", |ui| {
                            self.editor.parse_events(ui);
                        });
                        ui.menu_button("Egui Internals", |ui| {
                            ui.menu_button("Memory", |ui| ctx.memory_ui(ui));
                            ui.menu_button("Loaders", |ui| ctx.loaders_ui(ui));
                            ui.menu_button("Textures", |ui| ctx.texture_ui(ui));
                            ui.menu_button("Settings", |ui| ctx.settings_ui(ui));
                        });
                        ui.menu_button("Send events", |ui| {
                            if ui.button(tracing::Level::ERROR.to_rich_text()).clicked() {
                                tracing::error!("test ERROR");
                            }
                            if ui.button(tracing::Level::WARN.to_rich_text()).clicked() {
                                tracing::warn!("test WARN");
                            }
                            if ui.button(tracing::Level::INFO.to_rich_text()).clicked() {
                                tracing::info!("test INFO");
                            }
                            if ui.button(tracing::Level::DEBUG.to_rich_text()).clicked() {
                                tracing::debug!("test DEBUG");
                            }
                            if ui.button(tracing::Level::TRACE.to_rich_text()).clicked() {
                                tracing::trace!("test TRACE");
                            }
                        });
                    });
                });
        });
        egui::SidePanel::right("options").show(ctx, |ui| {
            ui.collapsing(egui::RichText::new("NetworkTables").heading(), |ui| {
                self.nt.show(ui, self.editor.edit());
            });
            let heading =
                egui::RichText::new(format!("Cameras ({})", self.cameras.elems().len())).heading();
            egui::CollapsingHeader::new(heading)
                .id_salt("Cameras")
                .show(ui, |ui| {
                    self.cameras.show(ui, self.editor.edit());
                });
            let heading =
                egui::RichText::new(format!("Components ({})", self.components.elems().len()))
                    .heading();
            egui::CollapsingHeader::new(heading)
                .id_salt("Components")
                .show(ui, |ui| {
                    self.components.show(ui, self.editor.edit());
                });
        });
        egui::SidePanel::left("editor").show(ctx, |ui| {
            self.editor.in_left(
                &mut dispatch::DispatchVisitor {
                    ntable: self.nt.visitor(),
                    cameras: self.cameras.visit(),
                    components: self.components.visit(),
                },
                ui,
            )
        });
        egui::TopBottomPanel::bottom("logging")
            .default_height(100.0)
            .resizable(true)
            .show(ctx, |ui| {
                self.logs.show(ui);
            });
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.editor.save(storage);
    }
}

fn main() {
    let (filter, layer, logs) = trace::create();
    tracing_subscriber::registry()
        .with(filter)
        .with(layer)
        .with(tracing_subscriber::fmt::layer())
        .init();
    let res = eframe::run_native(
        "VikingVision GUI",
        Default::default(),
        Box::new(|ctx| match VikingVision::new(ctx, logs) {
            Ok(app) => Ok(Box::new(app)),
            Err(err) => Err(Box::new(err)),
        }),
    );
    if let Err(err) = res {
        tracing::error!(%err, "error in app");
        std::process::exit(101);
    }
}
