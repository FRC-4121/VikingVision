use crate::buffer::{Buffer, PixelFormat};
use crate::pipeline::prelude::*;
use crate::vision::*;
use serde::{Deserialize, Serialize};
use std::sync::Arc;

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
        context.submit(None, Arc::new(buffer.convert(self.format)));
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
/// It outputs a [`Buffer`] with the [`Gray`](PixelFormat::Gray) format, with a value of 255 for pixels within the range and 0 for pixels
/// outside of it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct ColorFilterComponent {
    pub filter: ColorFilter,
}
impl ColorFilterComponent {
    pub const fn new(filter: ColorFilter) -> Self {
        Self { filter }
    }
}
impl Component for ColorFilterComponent {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, name: Option<&str>) -> OutputKind {
        if name.is_none() {
            OutputKind::Single
        } else {
            OutputKind::Multiple
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'r, '_, 's>) {
        let Ok(img) = context.get_as::<Buffer>(None).and_log_err() else {
            return;
        };
        let filtered = filter(img.borrow(), self.filter);
        context.submit(None, Arc::new(filtered));
    }
}
#[typetag::serde(name = "filter")]
impl ComponentFactory for ColorFilterComponent {
    fn build(&self, _: &str) -> Box<dyn Component> {
        Box::new(*self)
    }
}

#[inline(always)]
const fn max_u32() -> u32 {
    u32::MAX
}
#[inline(always)]
const fn max_usize() -> usize {
    usize::MAX
}
#[inline(always)]
const fn max_f32() -> f32 {
    1.0
}

/// A component that filters an image in a given color space.
///
/// It outputs a [`Buffer`] with the [`Gray`](PixelFormat::Gray) format, with a value of 255 for pixels within the range and 0 for pixels
/// outside of it.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BlobComponent {
    /// Minimum width of blobs to emit.
    #[serde(default)]
    min_w: u32,
    /// Maximum width of blobs to emit.
    #[serde(default = "max_u32")]
    max_w: u32,
    /// Minimum height of blobs to emit.
    #[serde(default)]
    min_h: u32,
    /// Maximum height of blobs to emit.
    #[serde(default = "max_u32")]
    max_h: u32,
    /// Minimum pixel count of blobs to emit.
    #[serde(default)]
    min_px: usize,
    /// Maximum pixel count of blobs to emit.
    #[serde(default = "max_usize")]
    max_px: usize,
    /// Minimum aspect ratio (height / width) of blobs to emit.
    #[serde(default)]
    min_aspect: f32,
    /// Maximum aspect ratio (height / width) of blobs to emit.
    #[serde(default = "max_f32")]
    max_aspect: f32,
}
impl Default for BlobComponent {
    fn default() -> Self {
        Self {
            min_w: 0,
            max_w: u32::MAX,
            min_h: 0,
            max_h: u32::MAX,
            min_px: 0,
            max_px: usize::MAX,
            min_aspect: 0.0,
            max_aspect: 1.0,
        }
    }
}
impl Component for BlobComponent {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, name: Option<&str>) -> OutputKind {
        match name {
            None => OutputKind::Single,
            Some("elem") => OutputKind::Multiple,
            _ => OutputKind::None,
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'r, '_, 's>) {
        let Ok(img) = context.get_as::<Buffer>(None).and_log_err() else {
            return;
        };
        let px = img.format.pixel_size() as usize;
        let pixels = img
            .data
            .chunks(img.width as usize * px)
            .map(|r| r.chunks(px).map(|c| c.iter().any(|p| *p > 0)));
        let blobs = BlobsIterator::new(pixels);
        let collect = context.listening(None);
        let stream = context.listening("elem");
        let mut vec = Vec::new();
        for blob in blobs {
            let w = blob.width();
            let h = blob.height();
            if w < self.min_w || w > self.max_w {
                continue;
            }
            if h < self.min_h || h > self.max_h {
                continue;
            }
            if blob.pixels < self.min_px || blob.pixels > self.max_px {
                continue;
            }
            if self.min_aspect > 0.0 || self.max_aspect < 1.0 {
                let frac = h as f32 / w as f32;
                if frac < self.min_aspect || frac > self.max_aspect {
                    continue;
                }
            }
            if collect {
                vec.push(blob);
            }
            if stream {
                context.submit("elem", Arc::new(blob));
            }
        }
        if collect {
            context.submit(None, Arc::new(vec));
        }
    }
}
#[typetag::serde(name = "blob")]
impl ComponentFactory for BlobComponent {
    fn build(&self, _: &str) -> Box<dyn Component> {
        Box::new(*self)
    }
}
