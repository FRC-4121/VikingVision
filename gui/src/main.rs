use eframe::egui::containers::menu::MenuConfig;
use eframe::{App, CreationContext, egui};
use std::error::Error;
use std::io;

mod editor;
mod visit;

struct VikingVision {
    editor: editor::EditorState,
}
impl VikingVision {
    fn new(ctx: &CreationContext) -> io::Result<Self> {
        let editor = editor::EditorState::load(ctx.storage);
        Ok(Self { editor })
    }
    fn new_boxed(ctx: &CreationContext) -> Result<Box<dyn App>, Box<dyn Error + Send + Sync>> {
        Self::new(ctx)
            .map(|a| Box::new(a) as _)
            .map_err(|e| Box::new(e) as _)
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
                        })
                    });
                });
        });
        egui::SidePanel::right("options").show(ctx, |ui| {
            ui.collapsing("NetworkTables", |ui| {});
            ui.collapsing("Cameras", |ui| {});
            ui.collapsing("Components", |ui| {});
        });
        egui::SidePanel::left("editor").show(ctx, |ui| self.editor.in_left(&mut (), ui));
    }
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        self.editor.save(storage);
    }
}

fn main() {
    tracing_subscriber::fmt().init();
    let res = eframe::run_native(
        "VikingVision GUI",
        Default::default(),
        Box::new(VikingVision::new_boxed),
    );
    if let Err(err) = res {
        tracing::error!(%err, "error in app");
        std::process::exit(101);
    }
}
