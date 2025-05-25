use crate::buffer::{Buffer, PixelFormat};
use crate::pipeline::prelude::*;
use crate::vision::ColorFilter;
use serde::{Deserialize, Serialize};

/// A simple component to change the color space of a buffer.
///
/// This is useful for downstream components that use [`Buffer::clone_cow`].
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct ColorSpaceComponent {
    pub format: PixelFormat,
}
impl ColorSpaceComponent {
    pub const fn new(format: PixelFormat) -> Self {
        Self { format }
    }
}
impl Component for ColorSpaceComponent {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, name: Option<&str>) -> OutputKind {
        if name.is_none() {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'r, '_, 's>) {
        let Ok(buffer) = context.get_as::<Buffer<'static>>(None).and_log_err() else {
            return;
        };
        context.submit(None, std::sync::Arc::new(buffer.convert(self.format)));
    }
}
#[typetag::serde(name = "colorspace")]
impl ComponentFactory for ColorSpaceComponent {
    fn build(&self, _: &str) -> Box<dyn Component> {
        Box::new(self.clone())
    }
}

/// A component that filters an image in a given color space.
///
/// It outputs a [`Buffer`] with the [`Gray`](PixelFormat::Gray) format, with a value of 255 signifying  
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ColorFilterComponent {
    pub filter: ColorFilter,
}
impl ColorFilterComponent {
    pub const fn new(filter: ColorFilter) -> Self {
        Self { filter }
    }
}
