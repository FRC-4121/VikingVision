use crate::buffer::{Buffer, PixelFormat};
use crate::pipeline::prelude::*;
use crate::vision::*;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// A simple component to change the color space of a buffer.
///
/// This is useful for downstream components that use [`Buffer::convert_cow`].
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
        context.submit(None, buffer.convert(self.format));
    }
}
#[typetag::serde(name = "colorspace")]
impl ComponentFactory for ColorSpaceComponent {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(*self)
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
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'r, '_, 's>) {
        let Ok(img) = context.get_as::<Buffer>(None).and_log_err() else {
            return;
        };
        let filtered = filter(img.borrow(), self.filter);
        context.submit(None, filtered);
    }
}
#[typetag::serde(name = "filter")]
impl ComponentFactory for ColorFilterComponent {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
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

/// A component that detects and filters blobs in binary images.
///
/// It can output blobs either as a collected vector on the primary channel or stream individual
/// blobs on the "elem" channel, filtered by size, pixel count, and aspect ratio constraints.
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
            max_aspect: f32::INFINITY,
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
            if self.min_aspect > 0.0 || self.max_aspect < f32::INFINITY {
                let mut frac = h as f32 / w as f32;
                if frac.is_nan() {
                    frac = f32::INFINITY;
                }
                if frac < self.min_aspect || frac > self.max_aspect {
                    continue;
                }
            }
            if collect {
                vec.push(blob);
            }
            if stream {
                context.submit("elem", blob);
            }
        }
        if collect {
            context.submit(None, vec);
        }
    }
}
#[typetag::serde(name = "blob")]
impl ComponentFactory for BlobComponent {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(*self)
    }
}

#[derive(Deserialize)]
struct FilterShim {
    width: usize,
    height: usize,
    index: usize,
}
#[derive(Deserialize)]
struct BlurShim {
    width: usize,
    height: usize,
}

#[derive(Debug, Error)]
enum FromFilterError {
    #[error("window width must odd")]
    EvenWidth,
    #[error("window height must odd")]
    EvenHeight,
    #[error("pixel index must be less than the window size")]
    IndexOob,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "FilterShim")]
pub struct PercentileFilterComponent {
    pub width: usize,
    pub height: usize,
    pub index: usize,
}
impl TryFrom<FilterShim> for PercentileFilterComponent {
    type Error = FromFilterError;

    fn try_from(value: FilterShim) -> Result<Self, Self::Error> {
        if value.width & 1 == 0 {
            return Err(FromFilterError::EvenWidth);
        }
        if value.height & 1 == 0 {
            return Err(FromFilterError::EvenHeight);
        }
        let Some(len) = value.width.checked_mul(value.height) else {
            return Err(FromFilterError::IndexOob);
        };
        if value.index >= len {
            return Err(FromFilterError::IndexOob);
        }
        Ok(Self {
            width: value.width,
            height: value.height,
            index: value.index,
        })
    }
}
impl Component for PercentileFilterComponent {
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
        let Ok(img) = context.get_as::<Buffer>(None).and_log_err() else {
            return;
        };
        let mut dst = Buffer::empty_rgb();
        percentile_filter(img.borrow(), &mut dst, self.width, self.height, self.index);
        context.submit(None, dst);
    }
}
#[typetag::serde(name = "percent-filter")]
impl ComponentFactory for PercentileFilterComponent {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(*self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "BlurShim")]
pub struct BoxBlurComponent {
    pub width: usize,
    pub height: usize,
}
impl TryFrom<BlurShim> for BoxBlurComponent {
    type Error = FromFilterError;

    fn try_from(value: BlurShim) -> Result<Self, Self::Error> {
        if value.width & 1 == 0 {
            return Err(FromFilterError::EvenWidth);
        }
        if value.height & 1 == 0 {
            return Err(FromFilterError::EvenHeight);
        }
        Ok(Self {
            width: value.width,
            height: value.height,
        })
    }
}
impl Component for BoxBlurComponent {
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
        let mut dst = Buffer::empty_rgb();
        box_blur(img.borrow(), &mut dst, self.width, self.height);
        context.submit(None, dst);
    }
}
#[typetag::serde(name = "box-blur")]
impl ComponentFactory for BoxBlurComponent {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(*self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "BlurShim")]
pub struct DilateFactory {
    pub width: usize,
    pub height: usize,
}
impl TryFrom<BlurShim> for DilateFactory {
    type Error = FromFilterError;

    fn try_from(value: BlurShim) -> Result<Self, Self::Error> {
        if value.width & 1 == 0 {
            return Err(FromFilterError::EvenWidth);
        }
        if value.height & 1 == 0 {
            return Err(FromFilterError::EvenHeight);
        }
        Ok(Self {
            width: value.width,
            height: value.height,
        })
    }
}
#[typetag::serde(name = "dilate")]
impl ComponentFactory for DilateFactory {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(PercentileFilterComponent {
            width: self.width,
            height: self.height,
            index: 0,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "BlurShim")]
pub struct ErodeFactory {
    pub width: usize,
    pub height: usize,
}
impl TryFrom<BlurShim> for ErodeFactory {
    type Error = FromFilterError;

    fn try_from(value: BlurShim) -> Result<Self, Self::Error> {
        if value.width & 1 == 0 {
            return Err(FromFilterError::EvenWidth);
        }
        if value.height & 1 == 0 {
            return Err(FromFilterError::EvenHeight);
        }
        Ok(Self {
            width: value.width,
            height: value.height,
        })
    }
}
#[typetag::serde(name = "erode")]
impl ComponentFactory for ErodeFactory {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(PercentileFilterComponent {
            width: self.width,
            height: self.height,
            index: self.width * self.height - 1,
        })
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "BlurShim")]
pub struct MedianFilterFactory {
    pub width: usize,
    pub height: usize,
}
impl TryFrom<BlurShim> for MedianFilterFactory {
    type Error = FromFilterError;

    fn try_from(value: BlurShim) -> Result<Self, Self::Error> {
        if value.width & 1 == 0 {
            return Err(FromFilterError::EvenWidth);
        }
        if value.height & 1 == 0 {
            return Err(FromFilterError::EvenHeight);
        }
        Ok(Self {
            width: value.width,
            height: value.height,
        })
    }
}
#[typetag::serde(name = "median-filter")]
impl ComponentFactory for MedianFilterFactory {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(PercentileFilterComponent {
            width: self.width,
            height: self.height,
            index: (self.width * self.height) / 2,
        })
    }
}
