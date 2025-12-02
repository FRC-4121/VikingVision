use eframe::{App, CreationContext, egui};
use std::error::Error;
use std::io;
use std::path::PathBuf;

struct VikingVision {
    contents: String,
    loaded: PathBuf,
    document: Result<toml_edit::DocumentMut, toml_edit::TomlError>,
    contents_persisted: bool,
    path_persisted: bool,
}
impl VikingVision {
    fn new(ctx: &CreationContext) -> io::Result<Self> {
        let mut contents = String::new();
        let mut loaded = PathBuf::new();
        if let Some(storage) = ctx.storage {
            if let Some(c) = storage.get_string("file_contents") {
                contents = c;
            }
            if let Some(l) = storage.get_string("file_path") {
                loaded = l.into();
            }
        }
        let document = contents.parse();
        Ok(Self {
            contents,
            loaded,
            document,
            contents_persisted: true,
            path_persisted: true,
        })
    }
    fn new_boxed(ctx: &CreationContext) -> Result<Box<dyn App>, Box<dyn Error + Send + Sync>> {
        Self::new(ctx)
            .map(|a| Box::new(a) as _)
            .map_err(|e| Box::new(e) as _)
    }
}
impl App for VikingVision {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {}
    fn save(&mut self, storage: &mut dyn eframe::Storage) {
        if !self.contents_persisted {
            self.contents_persisted = true;
            storage.set_string("file_contents", self.contents.clone());
        }
        if !self.path_persisted {
            self.path_persisted = true;
            storage.set_string("file_path", self.loaded.display().to_string());
        }
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
