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
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        egui::SidePanel::right("options").show(ctx, |ui| {
            ui.collapsing("NetworkTables", |ui| {});
            ui.collapsing("Cameras", |ui| {});
            ui.collapsing("Components", |ui| {});
        });
        egui::SidePanel::left("editor").show(ctx, |ui| self.editor.render(&mut (), ui));
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
