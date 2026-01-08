use crate::buffer::{Buffer, PixelFormat};
use crate::pipeline::{PipelineId, PipelineName, prelude::*};
use crate::vision::*;
use crate::vision_debug;
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
    fn output_kind(&self, name: &str) -> OutputKind {
        if name.is_empty() {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(buffer) = context.get_as::<Buffer<'static>>(None).and_log_err() else {
            return;
        };
        context.submit("", buffer.convert(self.format));
    }
}
#[typetag::serde(name = "color-space")]
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
#[serde(transparent)]
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
    fn output_kind(&self, name: &str) -> OutputKind {
        if name.is_empty() {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(img) = context.get_as::<Buffer>(None).and_log_err() else {
            return;
        };
        let mut filtered = Buffer::empty(PixelFormat::LUMA);
        color_filter(img.borrow(), &mut filtered, self.filter);
        context.submit("", filtered);
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
    f32::INFINITY
}
#[inline(always)]
const fn one_f32() -> f32 {
    1.0
}

/// A component that detects and filters blobs in binary images.
///
/// It can output blobs either as a collected vector on the primary channel or stream individual
/// blobs on the "elem" channel, filtered by size, pixel count, and aspect ratio constraints.
#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub struct BlobsComponent {
    /// Minimum width of blobs to emit.
    #[serde(default)]
    pub min_w: u32,
    /// Maximum width of blobs to emit.
    #[serde(default = "max_u32")]
    pub max_w: u32,
    /// Minimum height of blobs to emit.
    #[serde(default)]
    pub min_h: u32,
    /// Maximum height of blobs to emit.
    #[serde(default = "max_u32")]
    pub max_h: u32,
    /// Minimum pixel count of blobs to emit.
    #[serde(default)]
    pub min_px: usize,
    /// Maximum pixel count of blobs to emit.
    #[serde(default = "max_usize")]
    pub max_px: usize,
    /// Minimum fill ratio (pixels / (width * height)) of blobs to emit.
    #[serde(default)]
    pub min_fill: f32,
    /// Maximum fill ratio (pixels / (width * height)) of blobs to emit.
    #[serde(default = "one_f32")]
    pub max_fill: f32,
    /// Minimum aspect ratio (height / width) of blobs to emit.
    #[serde(default)]
    pub min_aspect: f32,
    /// Maximum aspect ratio (height / width) of blobs to emit.
    #[serde(default = "max_f32")]
    pub max_aspect: f32,
}
impl Default for BlobsComponent {
    fn default() -> Self {
        Self {
            min_w: 0,
            max_w: u32::MAX,
            min_h: 0,
            max_h: u32::MAX,
            min_px: 0,
            max_px: usize::MAX,
            min_fill: 0.0,
            max_fill: 1.0,
            min_aspect: 0.0,
            max_aspect: f32::INFINITY,
        }
    }
}
impl Component for BlobsComponent {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, name: &str) -> OutputKind {
        match name {
            "" => OutputKind::Multiple,
            "vec" => OutputKind::Single,
            _ => OutputKind::None,
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(img) = context.get_as::<Buffer>(None).and_log_err() else {
            return;
        };
        let blobs = BlobsIterator::from_buffer(&img);
        let collect = context.listening("vec");
        let stream = context.listening("");
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
            if self.min_fill > 0.0 || self.max_fill < 1.0 {
                let frac = blob.pixels as f32 / (w as f32 * h as f32);
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
            context.submit("", vec);
        }
    }
}
#[typetag::serde(name = "blobs")]
impl ComponentFactory for BlobsComponent {
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

#[derive(Deserialize)]
struct GaussianShim {
    sigma: f32,
    width: usize,
    height: usize,
}

#[derive(Debug, Error)]
enum FromShimError {
    #[error("window width must be odd")]
    EvenWidth,
    #[error("window height must be odd")]
    EvenHeight,
    #[error("pixel index must be less than the window size")]
    IndexOob,
    #[error("sigma must be positive")]
    NonPositiveSigma,
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "FilterShim")]
pub struct PercentileFilterComponent {
    pub width: usize,
    pub height: usize,
    pub index: usize,
}
impl TryFrom<FilterShim> for PercentileFilterComponent {
    type Error = FromShimError;

    fn try_from(value: FilterShim) -> Result<Self, Self::Error> {
        if value.width & 1 == 0 {
            return Err(FromShimError::EvenWidth);
        }
        if value.height & 1 == 0 {
            return Err(FromShimError::EvenHeight);
        }
        let Some(len) = value.width.checked_mul(value.height) else {
            return Err(FromShimError::IndexOob);
        };
        if value.index >= len {
            return Err(FromShimError::IndexOob);
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
    fn output_kind(&self, name: &str) -> OutputKind {
        if name.is_empty() {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(img) = context.get_as::<Buffer>(None).and_log_err() else {
            return;
        };
        let mut dst = Buffer::empty_rgb();
        percentile_filter(img.borrow(), &mut dst, self.width, self.height, self.index);
        context.submit("", dst);
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
    type Error = FromShimError;

    fn try_from(value: BlurShim) -> Result<Self, Self::Error> {
        if value.width & 1 == 0 {
            return Err(FromShimError::EvenWidth);
        }
        if value.height & 1 == 0 {
            return Err(FromShimError::EvenHeight);
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
    fn output_kind(&self, name: &str) -> OutputKind {
        if name.is_empty() {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(img) = context.get_as::<Buffer>(None).and_log_err() else {
            return;
        };
        let mut img = img.clone_static();
        box_blur(&mut img, &mut Buffer::empty_rgb(), self.width, self.height);
        context.submit("", img);
    }
}
#[typetag::serde(name = "box-blur")]
impl ComponentFactory for BoxBlurComponent {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(*self)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "GaussianShim")]
pub struct GaussianBlurComponent {
    pub sigma: f32,
    pub width: usize,
    pub height: usize,
}
impl TryFrom<GaussianShim> for GaussianBlurComponent {
    type Error = FromShimError;

    fn try_from(value: GaussianShim) -> Result<Self, Self::Error> {
        if value.width & 1 == 0 {
            return Err(FromShimError::EvenWidth);
        }
        if value.height & 1 == 0 {
            return Err(FromShimError::EvenHeight);
        }
        if value.sigma <= 0.0 {
            return Err(FromShimError::NonPositiveSigma);
        }
        Ok(Self {
            sigma: value.sigma,
            width: value.width,
            height: value.height,
        })
    }
}
impl Component for GaussianBlurComponent {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, name: &str) -> OutputKind {
        if name.is_empty() {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(img) = context.get_as::<Buffer>(None).and_log_err() else {
            return;
        };
        let mut img = img.clone_static();
        gaussian_blur(
            &mut img,
            &mut Buffer::empty_rgb(),
            self.sigma,
            self.width,
            self.height,
        );
        context.submit("", img);
    }
}
#[typetag::serde(name = "gaussian-blur")]
impl ComponentFactory for GaussianBlurComponent {
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
    type Error = FromShimError;

    fn try_from(value: BlurShim) -> Result<Self, Self::Error> {
        if value.width & 1 == 0 {
            return Err(FromShimError::EvenWidth);
        }
        if value.height & 1 == 0 {
            return Err(FromShimError::EvenHeight);
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
    type Error = FromShimError;

    fn try_from(value: BlurShim) -> Result<Self, Self::Error> {
        if value.width & 1 == 0 {
            return Err(FromShimError::EvenWidth);
        }
        if value.height & 1 == 0 {
            return Err(FromShimError::EvenHeight);
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
    type Error = FromShimError;

    fn try_from(value: BlurShim) -> Result<Self, Self::Error> {
        if value.width & 1 == 0 {
            return Err(FromShimError::EvenWidth);
        }
        if value.height & 1 == 0 {
            return Err(FromShimError::EvenHeight);
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

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResizeComponent {
    pub width: u32,
    pub height: u32,
}
impl Component for ResizeComponent {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, name: &str) -> OutputKind {
        if name.is_empty() {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(img) = context.get_as::<Buffer>(None).and_log_err() else {
            return;
        };
        let mut new = Buffer::empty_rgb();
        resize(img.borrow(), &mut new, self.width, self.height);
        context.submit("", new);
    }
}
#[typetag::serde(name = "resize")]
impl ComponentFactory for ResizeComponent {
    fn build(&self, _ctx: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(*self)
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct VisionDebugComponent {
    #[serde(flatten)]
    pub mode: Option<vision_debug::DebugMode>,
    #[serde(skip, default = "std::sync::Once::new")]
    pub logged_warning: std::sync::Once,
}
impl Clone for VisionDebugComponent {
    fn clone(&self) -> Self {
        Self {
            mode: self.mode.clone(),
            logged_warning: std::sync::Once::new(),
        }
    }
}
impl Component for VisionDebugComponent {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, _name: &str) -> OutputKind {
        OutputKind::None
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(image) = context.get_as::<Buffer>(None).and_log_err() else {
            return;
        };
        let Some(sender) = vision_debug::GLOBAL_SENDER.get() else {
            self.logged_warning
                .call_once(|| tracing::warn!("no debug handler registered"));
            return;
        };
        sender.send_image(vision_debug::DebugImage {
            image: image.clone_static(),
            name: context
                .context
                .request::<PipelineName>()
                .map_or_else(|| "<anon>".to_string(), |n| n.0.to_string()),
            id: context
                .context
                .request::<PipelineId>()
                .map_or(0, |id| id.0 as _)
                | ((context.comp_id().index() as u128) << 64),
            mode: self.mode.clone().unwrap_or_default(),
        });
    }
}
#[typetag::serde(name = "vision-debug")]
impl ComponentFactory for VisionDebugComponent {
    fn build(&self, _ctx: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(self.clone())
    }
}
