use crate::broadcast::*;
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::fmt::{self, Debug, Display, Formatter};
use tracing::warn;

pub mod conv;

/// A format for the pixels
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum PixelFormat {
    Luma,
    LumaA,

    Gray,
    GrayA,

    Rgb,
    Rgba,

    Hsv,
    Hsva,

    Yuyv,
    YCbCr,
    YCbCrA,
}
impl Display for PixelFormat {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, f)
    }
}
impl PixelFormat {
    /// Number of bytes per pixel
    pub const fn pixel_size(&self) -> u8 {
        match self {
            Self::Luma | Self::Gray => 1,
            Self::Yuyv | Self::LumaA | Self::GrayA => 2,
            Self::Rgb | Self::Hsv | Self::YCbCr => 3,
            Self::Rgba | Self::Hsva | Self::YCbCrA => 4,
        }
    }
    pub const fn drop_alpha(self) -> Option<Self> {
        match self {
            Self::LumaA => Some(Self::Luma),
            Self::GrayA => Some(Self::Gray),
            Self::Rgba => Some(Self::Rgb),
            Self::Hsva => Some(Self::Hsv),
            Self::YCbCrA => Some(Self::YCbCr),
            _ => None,
        }
    }
    pub const fn add_alpha(self) -> Option<Self> {
        match self {
            Self::Luma => Some(Self::LumaA),
            Self::Gray => Some(Self::GrayA),
            Self::Rgb => Some(Self::Rgba),
            Self::Hsv => Some(Self::Hsva),
            Self::YCbCr => Some(Self::YCbCrA),
            _ => None,
        }
    }
}

/// A maybe-owned frame buffer.
#[derive(Clone, PartialEq, Eq, Hash)]
pub struct Buffer<'a> {
    pub width: u32,
    pub height: u32,
    pub format: PixelFormat,
    pub data: Cow<'a, [u8]>,
}
impl Debug for Buffer<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Buffer")
            .field("width", &self.width)
            .field("height", &self.height)
            .field("format", &self.format)
            .field("data", &format!("[u8; {}]", self.data.len()))
            .finish()
    }
}
impl Display for Buffer<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{}x{} {} buffer ({} bytes)",
            self.width,
            self.height,
            self.format,
            self.data.len()
        )
    }
}
impl Buffer<'_> {
    pub fn zeroed(width: u32, height: u32, format: PixelFormat) -> Self {
        Self {
            width,
            height,
            format,
            data: vec![0; width as usize * height as usize * format.pixel_size() as usize].into(),
        }
    }
    pub fn monochrome(width: u32, height: u32, format: PixelFormat, color: &[u8]) -> Self {
        assert_eq!(format.pixel_size() as usize, color.len());
        Self {
            width,
            height,
            format,
            data: color.repeat(width as usize * height as usize).into(),
        }
    }
    /// Get a `Buffer` that borrows from this one.
    pub fn borrow(&self) -> Buffer<'_> {
        let Buffer {
            width,
            height,
            format,
            ref data,
        } = *self;
        Buffer {
            width,
            height,
            format,
            data: (&**data).into(),
        }
    }
    /// Get an owned `Buffer` without consuming this one.
    pub fn clone_static(&self) -> Buffer<'static> {
        let Buffer {
            width,
            height,
            format,
            ref data,
        } = *self;
        Buffer {
            width,
            height,
            format,
            data: data.to_vec().into(),
        }
    }
    /// Make the current buffer into an owned one.
    pub fn into_static(self) -> Buffer<'static> {
        let Buffer {
            width,
            height,
            format,
            data,
        } = self;
        Buffer {
            width,
            height,
            format,
            data: data.into_owned().into(),
        }
    }

    pub fn convert_into(&self, out: &mut Buffer<'_>) {
        use PixelFormat::*;
        use conv::*;
        use std::convert::identity;
        macro_rules! maybe {
            (true => $($body:tt)*) => {
                $($body)*
            };
            (false => $($body:tt)*) => {};
        }
        macro_rules! base_impl {
            ($tr:expr, $from:expr => $to:expr, $yuyv_in:tt $yuyv_out:tt) => {
                match $from {
                    Luma => match $to {
                        YCbCr => pb($tr(luma::ycc), self, out),
                        Rgb => pb($tr(luma::rgb), self, out),
                        _ => {
                            maybe!($yuyv_out => {
                                if $to == Yuyv {
                                    pb($tr(luma::yuyv), self, out);
                                    return;
                                }
                            });
                            base_impl!(@from_rgb (|conv| $tr(compose(luma::rgb, conv))), $to, false);
                        }
                    },
                    Gray => {
                        base_impl!(@to_rgb $tr => gray::rgb, $to, $yuyv_out);
                    }
                    Hsv => {
                        base_impl!(@to_rgb $tr => luma::rgb, $to, $yuyv_out);
                    }
                    YCbCr => {
                        base_impl!(@to_rgb $tr => ycc::rgb, $to, $yuyv_out);
                    }
                    _ => unreachable!(),
                }
            };
            (@to_rgb $tr:expr => $conv:expr, $to:expr, $yuyv_out:tt) => {
                if $to == Rgb {
                    pb($tr($conv), self, out)
                } else {
                    base_impl!(@from_rgb (|conv| $tr(compose($conv, conv))), $to, $yuyv_out);
                }
            };
            (@from_rgb $tr:expr, $to:expr, $yuyv_out:tt) => {
                match $to {
                    Hsv => pb($tr(rgb::hsv), self, out),
                    YCbCr => pb($tr(rgb::ycc), self, out),
                    Luma => pb($tr(rgb::luma), self, out),
                    Gray => pb($tr(rgb::gray), self, out),
                    _ => {
                        maybe!($yuyv_out => {
                            if $to == Yuyv {
                                pb(compose(double($tr(rgb::ycc)), ycc::yuyv), self, out);
                            }
                        });
                    }
                }
            };
        }
        use par_broadcast2 as pb;
        assert_eq!(self.width, out.width);
        assert_eq!(self.height, out.height);
        if self.format == out.format {
            out.data.to_mut().copy_from_slice(&self.data);
            return;
        }
        if let Some(sd) = self.format.drop_alpha() {
            if sd == out.format {
                match sd.pixel_size() {
                    1 => pb(drop_alpha::<1>, self, out),
                    3 => pb(drop_alpha::<3>, self, out),
                    _ => unreachable!(),
                }
                return;
            }
            if let Some(od) = self.format.drop_alpha() {
                base_impl!(lift_alpha, sd => od, false false);
            } else {
                base_impl!(|conv| compose(drop_alpha, conv), sd => out.format, false true);
            }
        } else if let Some(od) = out.format.drop_alpha() {
            base_impl!(|conv| compose(conv, add_alpha), self.format => od, true false);
        } else {
            base_impl!(identity, self.format => out.format, true true);
        }
    }
    pub fn convert_inplace(&mut self, to: PixelFormat) {
        use PixelFormat::*;
        use conv::*;
        if self.format == to {
            return;
        }
        if self.format.pixel_size() == to.pixel_size() {
            match (self.format, to) {
                (Rgb, Hsv) => par_broadcast1(to_inplace(rgb::hsv), self),
                (Rgb, YCbCr) => par_broadcast1(to_inplace(rgb::ycc), self),
                (Hsv, Rgb) => par_broadcast1(to_inplace(hsv::rgb), self),
                (Hsv, YCbCr) => par_broadcast1(to_inplace(compose(hsv::rgb, rgb::ycc)), self),
                (YCbCr, Rgb) => par_broadcast1(to_inplace(ycc::rgb), self),
                (YCbCr, Hsv) => par_broadcast1(to_inplace(compose(ycc::rgb, rgb::hsv)), self),
                (Rgba, Hsva) => par_broadcast1(to_inplace(rgb::hsv), self),
                (Rgba, YCbCrA) => par_broadcast1(to_inplace(rgb::ycc), self),
                (Hsva, Rgba) => par_broadcast1(to_inplace(hsv::rgb), self),
                (Hsva, YCbCrA) => par_broadcast1(to_inplace(compose(hsv::rgb, rgb::ycc)), self),
                (YCbCrA, Rgba) => par_broadcast1(to_inplace(ycc::rgb), self),
                (YCbCrA, Hsva) => par_broadcast1(to_inplace(compose(ycc::rgb, rgb::hsv)), self),
                _ => unreachable!(),
            }
        } else {
            warn!(from = %self.format, %to, "in-place pixel sizes don't match");
        }
    }
    pub fn convert(&self, format: PixelFormat) -> Buffer<'static> {
        let mut out = Buffer::zeroed(self.width, self.height, format);
        self.convert_into(&mut out);
        out
    }
}
