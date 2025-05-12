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
impl Default for Buffer<'_> {
    fn default() -> Self {
        Self::empty_rgb()
    }
}
impl Buffer<'_> {
    /// Create an empty buffer with the given format.
    pub const fn empty(format: PixelFormat) -> Self {
        Self {
            width: 0,
            height: 0,
            format,
            data: Cow::Owned(Vec::new()),
        }
    }
    /// Convenience alias for `Buffer::empty(Rgb)`.
    pub const fn empty_rgb() -> Self {
        Self::empty(PixelFormat::Rgb)
    }
    /// Create a buffer of the given size filled with zeroes.
    pub fn zeroed(width: u32, height: u32, format: PixelFormat) -> Self {
        Self {
            width,
            height,
            format,
            data: vec![0; width as usize * height as usize * format.pixel_size() as usize].into(),
        }
    }
    /// Create a buffer of a single repeated color. `color` must equal `format.pixel_size()`.
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
        use par_broadcast2 as pb;
        macro_rules! maybe {
            (true => $($body:tt)*) => {
                $($body)*
            };
            (false => $($body:tt)*) => {};
        }
        macro_rules! base_impl {
            (($tr:expr, $iadd:expr, $oadd:expr), $from:expr => $to:expr, $yuyv_in:tt $yuyv_out:tt) => {
                maybe!($yuyv_in => {
                    if $from == Yuyv {
                        match $to {
                            YCbCr => pb::<4, { (3 + $oadd) * 2 }, _, _>(compose(yuyv::ycc, double($tr(iden))), self, out),
                            Luma => pb::<4, { (1 + $oadd) * 2 }, _, _>(compose(yuyv::luma, double($tr(iden))), self, out),
                            Rgb => pb::<4, { (3 + $oadd) * 2 }, _, _>(compose(yuyv::ycc, double($tr(ycc::rgb))), self, out),
                            _ => {
                                base_impl!(@from_rgb (|conv| compose(yuyv::ycc, double($tr(compose(ycc::rgb, conv)))), 4, $oadd, 2), $to, false);
                            }
                        }
                        return;
                    }
                });
                match $from {
                    Luma => match $to {
                        YCbCr => pb::<{ 1 + $iadd }, { 3 + $oadd }, _, _>($tr(luma::ycc), self, out),
                        Rgb => pb::<{ 1 + $iadd }, { 3 + $oadd }, _, _>($tr(luma::rgb), self, out),
                        _ => {
                            maybe!($yuyv_out => {
                                if $to == Yuyv {
                                    pb::<{ (1 + $iadd) * 2 }, 4, _, _>(compose(double($tr(iden)), luma::yuyv), self, out);
                                    return;
                                }
                            });
                            base_impl!(@from_rgb ((|conv| $tr(compose(luma::rgb, conv))), 1 + $iadd, $oadd, 1), $to, false);
                        }
                    },
                    Gray => {
                        base_impl!(@to_rgb ($tr => gray::rgb, 1 + $iadd, $oadd), $to, $yuyv_out);
                    }
                    Hsv => {
                        base_impl!(@to_rgb ($tr => hsv::rgb, 3 + $iadd, $oadd), $to, $yuyv_out);
                    }
                    YCbCr => {
                        base_impl!(@to_rgb ($tr => ycc::rgb, 3 + $iadd, $oadd), $to, $yuyv_out);
                    }
                    _ => unreachable!("attempted to convert {} to {}", $from, $to),
                }
            };
            (@to_rgb ($tr:expr => $conv:expr, $i:expr, $oadd:expr), $to:expr, $yuyv_out:tt) => {
                if $to == Rgb {
                    pb::<{ $i }, { 3 + $oadd }, _, _>($tr($conv), self, out)
                } else {
                    base_impl!(@from_rgb ((|conv| $tr(compose($conv, conv))), $i, $oadd, 1), $to, $yuyv_out);
                }
            };
            (@from_rgb ($tr:expr, $i:expr, $oadd:expr, $omul:expr), $to:expr, $yuyv_out:tt) => {
                match $to {
                    Hsv => pb::<{ $i }, { (3 + $oadd) * $omul }, _, _>($tr(rgb::hsv), self, out),
                    YCbCr => pb::<{ $i }, { (3 + $oadd) * $omul }, _, _>($tr(rgb::ycc), self, out),
                    Luma => pb::<{ $i }, { (1 + $oadd) * $omul }, _, _>($tr(rgb::luma), self, out),
                    Gray => pb::<{ $i }, { (1 + $oadd) * $omul }, _, _>($tr(rgb::gray), self, out),
                    _ => {
                        maybe!($yuyv_out => {
                            if $to == Yuyv {
                                pb::<{ $i * 2 }, 4, _, _>(compose(double($tr(rgb::ycc)), ycc::yuyv), self, out);
                            }
                        });
                    }
                }
            };
        }
        out.data.to_mut().resize(
            self.width as usize * self.height as usize * out.format.pixel_size() as usize,
            0,
        );
        out.width = self.width;
        out.height = self.height;
        if self.format == out.format {
            out.data.to_mut().copy_from_slice(&self.data);
            return;
        }
        if let Some(sd) = self.format.drop_alpha() {
            if sd == out.format {
                match sd.pixel_size() {
                    1 => pb(drop_alpha::<[u8; 2]>, self, out),
                    3 => pb(drop_alpha::<[u8; 4]>, self, out),
                    _ => unreachable!("attempted to convert {} to {}", sd, out.format),
                }
                return;
            }
            if let Some(od) = self.format.drop_alpha() {
                base_impl!((lift_alpha, 1, 1), sd => od, false false);
            } else {
                base_impl!((|conv| compose(drop_alpha, conv), 1, 0), sd => out.format, false true);
            }
        } else if let Some(od) = out.format.drop_alpha() {
            if self.format == od {
                match od.pixel_size() {
                    1 => pb(add_alpha::<[u8; 1]>, self, out),
                    3 => pb(add_alpha::<[u8; 3]>, self, out),
                    _ => unreachable!("attempted to convert {} to {}", self.format, od),
                }
                return;
            }
            base_impl!(((|conv| compose(conv, add_alpha)), 0, 1), self.format => od, true false);
        } else {
            base_impl!((|conv| conv, 0, 0), self.format => out.format, true true);
        }
    }
    pub fn convert_inplace(&mut self, to: PixelFormat) {
        use PixelFormat::*;
        use conv::*;
        if self.format == to {
            return;
        }
        if self.format.pixel_size() == to.pixel_size() {
            self.format = to;
            match (self.format, to) {
                (Rgb, Hsv) => par_broadcast1(to_inplace(rgb::hsv), self),
                (Rgb, YCbCr) => par_broadcast1(to_inplace(rgb::ycc), self),
                (Hsv, Rgb) => par_broadcast1(to_inplace(hsv::rgb), self),
                (Hsv, YCbCr) => par_broadcast1(to_inplace(compose(hsv::rgb, rgb::ycc)), self),
                (YCbCr, Rgb) => par_broadcast1(to_inplace(ycc::rgb), self),
                (YCbCr, Hsv) => par_broadcast1(to_inplace(compose(ycc::rgb, rgb::hsv)), self),
                (Rgba, Hsva) => par_broadcast1::<4, _>(to_inplace(lift_alpha(rgb::hsv)), self),
                (Rgba, YCbCrA) => par_broadcast1::<4, _>(to_inplace(lift_alpha(rgb::ycc)), self),
                (Hsva, Rgba) => par_broadcast1::<4, _>(to_inplace(lift_alpha(hsv::rgb)), self),
                (Hsva, YCbCrA) => par_broadcast1::<4, _>(
                    to_inplace(lift_alpha(compose(hsv::rgb, rgb::ycc))),
                    self,
                ),
                (YCbCrA, Rgba) => par_broadcast1::<4, _>(to_inplace(lift_alpha(ycc::rgb)), self),
                (YCbCrA, Hsva) => par_broadcast1::<4, _>(
                    to_inplace(lift_alpha(compose(ycc::rgb, rgb::hsv))),
                    self,
                ),
                (Yuyv, LumaA) => par_broadcast1(yuyv::ilumaa, self),
                (Yuyv, GrayA) => par_broadcast1(
                    to_inplace(compose(
                        yuyv::ycc,
                        double(compose(compose(ycc::rgb, rgb::gray), add_alpha)),
                    )),
                    self,
                ),
                (LumaA, Yuyv) => par_broadcast1(lumaa::iyuyv, self),
                (LumaA, GrayA) => par_broadcast1::<2, _>(
                    to_inplace(lift_alpha(compose(luma::rgb, rgb::gray))),
                    self,
                ),
                (GrayA, LumaA) => par_broadcast1::<2, _>(
                    to_inplace(lift_alpha(compose(gray::rgb, rgb::luma))),
                    self,
                ),
                (GrayA, Yuyv) => par_broadcast1(
                    to_inplace(compose(
                        double(compose(drop_alpha, compose(gray::rgb, rgb::ycc))),
                        ycc::yuyv,
                    )),
                    self,
                ),
                (Luma, Gray) => par_broadcast1(to_inplace(compose(luma::rgb, rgb::gray)), self),
                (Gray, Luma) => par_broadcast1(to_inplace(compose(gray::rgb, rgb::luma)), self),
                _ => unreachable!("attempted to convert {} to {}", self.format, to),
            }
        } else {
            warn!(from = %self.format, %to, "in-place pixel sizes don't match");
            let buf = self.convert(to);
            *self = buf;
        }
    }
    pub fn convert(&self, format: PixelFormat) -> Buffer<'static> {
        let mut out = Buffer::zeroed(self.width, self.height, format);
        self.convert_into(&mut out);
        out
    }
    /// Copy the contents of another buffer into this one, taking ownership of the current buffer.
    pub fn copy_from(&mut self, src: Buffer<'_>) {
        self.height = src.height;
        self.width = src.width;
        self.format = src.format;
        if let Cow::Owned(data) = src.data {
            self.data = Cow::Owned(data);
            return;
        }
        if let Cow::Owned(data) = &mut self.data {
            if data.capacity() >= src.data.len() {
                if data.len() >= src.data.len() {
                    data.truncate(src.data.len());
                    data.copy_from_slice(&src.data);
                } else {
                    let (head, tail) = src.data.split_at(data.len());
                    data.copy_from_slice(head);
                    data.extend_from_slice(tail);
                }
                return;
            }
        }
        match &mut self.data {
            Cow::Borrowed(_) => self.data = Cow::Owned(src.data.to_vec()),
            Cow::Owned(data) => {
                data.resize(src.data.len(), 0);
                data.copy_from_slice(&src.data);
            }
        }
    }
}
