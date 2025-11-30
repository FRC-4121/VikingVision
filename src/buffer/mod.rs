use crate::broadcast::*;
use polonius_the_crab::{ForLt, Placeholder, PoloniusResult, polonius};
use serde::{Deserialize, Serialize};
use std::borrow::Cow;
use std::error::Error;
use std::fmt::{self, Debug, Display, Formatter};
use std::io;
use std::num::{NonZero, ParseIntError};
use thiserror::Error;
use tracing::{error, info_span};
use zune_jpeg::{JpegDecoder, errors::DecodeErrors as JpegDecodeErrors};
use zune_png::zune_core::{colorspace::ColorSpace, options::DecoderOptions};
use zune_png::{PngDecoder, error::PngDecodeErrors};

pub mod conv;

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct UnrecognizedFourCC(pub [u8; 4]);
impl Display for UnrecognizedFourCC {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.write_str("Unrecognized fourcc: \"")?;
        for ch in &self.0 {
            if ch.is_ascii_alphanumeric() {
                f.write_str(std::str::from_utf8(std::slice::from_ref(ch)).unwrap())?;
            } else {
                write!(f, "\\x{ch:0>2}")?;
            }
        }
        f.write_str("\"")
    }
}
impl Error for UnrecognizedFourCC {}

#[derive(Debug, Clone, Error)]
pub enum FormatParseError {
    #[error(transparent)]
    ParseInt(ParseIntError),
    #[error("Expected a number of channels in 1..200, found {0}")]
    OutOfRange(u8),
    #[error("unrecognized format")]
    UnrecognizedStr,
}

/// A transparent wrapper for a [`PixelFormat`] that formats as is used for its [`Serialize`] implementation
#[derive(Clone, Copy)]
pub struct DisplayAsSerialize(PixelFormat);
impl Display for DisplayAsSerialize {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(s) = self.0.name_lower() {
            f.write_str(s)
        } else {
            write!(f, "?{}", self.0.0)
        }
    }
}

/// A format for the pixels in a buffer
#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub struct PixelFormat(pub NonZero<u8>);
impl PixelFormat {
    pub const LUMA: Self = Self(NonZero::new(255).unwrap());
    pub const RGB: Self = Self(NonZero::new(254).unwrap());
    pub const HSV: Self = Self(NonZero::new(253).unwrap());
    pub const YCC: Self = Self(NonZero::new(252).unwrap());
    pub const RGBA: Self = Self(NonZero::new(250).unwrap());
    pub const YUYV: Self = Self(NonZero::new(240).unwrap());
    pub const ANON_1: Self = Self::anon(1).unwrap();
    pub const ANON_2: Self = Self::anon(2).unwrap();
    pub const ANON_3: Self = Self::anon(3).unwrap();
    pub const ANON_4: Self = Self::anon(4).unwrap();
    pub const MAX_ANON: NonZero<u8> = NonZero::new(200).unwrap();
    pub const NAMED: &[Self] = &[
        Self::LUMA,
        Self::RGB,
        Self::HSV,
        Self::YCC,
        Self::RGBA,
        Self::YUYV,
    ];
    /// Create an anonymous format with a number of channels
    pub const fn anon(channels: u8) -> Option<Self> {
        if channels > Self::MAX_ANON.get() {
            None
        } else if let Some(inner) = NonZero::new(channels) {
            Some(PixelFormat(inner))
        } else {
            None
        }
    }
    /// If this is a known color space, get the name in lower case, as is used for serialization
    pub const fn name_lower(&self) -> Option<&'static str> {
        match *self {
            Self::LUMA => Some("luma"),
            Self::RGB => Some("rgb"),
            Self::HSV => Some("hsv"),
            Self::YCC => Some("ycc"),
            Self::RGBA => Some("rgba"),
            Self::YUYV => Some("yuyv"),
            Self(v) => {
                let v = v.get();
                if v > Self::MAX_ANON.get() {
                    Some("<reserved>")
                } else {
                    None
                }
            }
        }
    }
    /// If this is a known color space, get the name in upper case, as is used for debug printing
    pub const fn name_upper(&self) -> Option<&'static str> {
        match *self {
            Self::LUMA => Some("LUMA"),
            Self::RGB => Some("RGB"),
            Self::HSV => Some("HSV"),
            Self::YCC => Some("YCC"),
            Self::RGBA => Some("RGBA"),
            Self::YUYV => Some("YUYV"),
            Self(v) => {
                let v = v.get();
                if v > Self::MAX_ANON.get() {
                    Some("<reserved>")
                } else {
                    None
                }
            }
        }
    }
    /// If this a known color space, get the name as it should be pretty-printed
    pub const fn name_pretty(&self) -> Option<&'static str> {
        match *self {
            Self::LUMA => Some("Luma"),
            Self::RGB => Some("RGB"),
            Self::HSV => Some("HSV"),
            Self::YCC => Some("YCbCr"),
            Self::RGBA => Some("RGBA"),
            Self::YUYV => Some("YUYV"),
            Self(v) => {
                let v = v.get();
                if v > Self::MAX_ANON.get() {
                    Some("<reserved>")
                } else {
                    None
                }
            }
        }
    }
    #[inline(always)]
    pub const fn is_anon(&self) -> bool {
        self.0.get() <= Self::MAX_ANON.get()
    }
    /// Get the number of bytes per pixel
    pub const fn pixel_size(&self) -> usize {
        match *self {
            Self::LUMA => 1,
            Self::YUYV => 2,
            Self::RGB | Self::HSV | Self::YCC => 3,
            Self::RGBA => 4,
            Self(v) => {
                let v = v.get();
                if v > Self::MAX_ANON.get() { 0 } else { v as _ }
            }
        }
    }
    #[cfg(feature = "v4l")]
    pub const fn known_fourcc(fourcc: v4l::FourCC) -> bool {
        matches!(&fourcc.repr, b"YUYV" | b"RGB8" | b"RGBA" | b"MJPG")
    }
    /// Some bright color
    ///
    /// This is red in color spaces that have colors, and white for others
    pub const fn bright_color(&self) -> &'static [u8] {
        match *self {
            Self::LUMA => &[255],
            Self::RGB => &[255, 0, 0],
            Self::HSV => &[255, 255, 255],
            Self::YCC => &[255, 0, 255],
            Self::RGBA => &[255, 0, 0, 255],
            Self::YUYV => &[255, 0, 255],
            Self(v) => {
                let v = v.get();
                if v > Self::MAX_ANON.get() {
                    &[]
                } else {
                    let (head, _) = FULL_BYTES.split_at(v as _); // slicing isn't const
                    head
                }
            }
        }
    }
    /// Parse a string, in the format used for de/serialization.
    ///
    /// This takes a few different cases for the named formats, or allows a number of channels preceeded by a `?`, like `"?3"` for an anonymous format with three channels.
    pub fn parse_str(s: &str) -> Result<Self, FormatParseError> {
        if let Some(rest) = s.strip_prefix('?') {
            rest.parse()
                .map_err(FormatParseError::ParseInt)
                .and_then(|v| Self::anon(v).ok_or(FormatParseError::OutOfRange(v)))
        } else {
            match s {
                "luma" | "Luma" | "LUMA" => Ok(Self::LUMA),
                "rgb" | "RGB" => Ok(Self::RGB),
                "hsv" | "HSV" => Ok(Self::HSV),
                "ycc" | "YCC" | "ycbcr" | "YCbCr" => Ok(Self::YCC),
                "rgba" | "RGBA" => Ok(Self::RGBA),
                "yuyv" | "YUYV" => Ok(Self::YUYV),
                _ => Err(FormatParseError::UnrecognizedStr),
            }
        }
    }
}
impl Serialize for PixelFormat {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        if serializer.is_human_readable() {
            serializer.collect_str(&DisplayAsSerialize(*self))
        } else {
            serializer.serialize_u8(self.0.get())
        }
    }
}
impl<'de> Deserialize<'de> for PixelFormat {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        struct PFVisitor;
        impl<'de> serde::de::Visitor<'de> for PFVisitor {
            type Value = PixelFormat;
            fn expecting(&self, formatter: &mut fmt::Formatter) -> fmt::Result {
                formatter.write_str("a pixel format")
            }
            fn visit_u8<E>(self, v: u8) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                NonZero::new(v)
                    .map(PixelFormat)
                    .ok_or_else(|| E::invalid_value(serde::de::Unexpected::Unsigned(v as _), &self))
            }
            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                v.try_into()
                    .ok()
                    .and_then(NonZero::new)
                    .map(PixelFormat)
                    .ok_or_else(|| E::invalid_value(serde::de::Unexpected::Unsigned(v as _), &self))
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: serde::de::Error,
            {
                PixelFormat::parse_str(v)
                    .map_err(|_| E::invalid_value(serde::de::Unexpected::Str(v), &self))
            }
        }
        if deserializer.is_human_readable() {
            deserializer.deserialize_str(PFVisitor)
        } else {
            deserializer.deserialize_u8(PFVisitor)
        }
    }
}

static FULL_BYTES: [u8; PixelFormat::MAX_ANON.get() as usize] =
    [255; PixelFormat::MAX_ANON.get() as usize];

impl Debug for PixelFormat {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(s) = self.name_upper() {
            f.write_str(s)
        } else {
            write!(f, "PixelFormat({})", self.0)
        }
    }
}
impl Display for PixelFormat {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        if let Some(s) = self.name_pretty() {
            f.write_str(s)
        } else {
            write!(f, "{}-channel", self.0)
        }
    }
}
#[cfg(feature = "v4l")]
impl TryFrom<v4l::FourCC> for PixelFormat {
    type Error = UnrecognizedFourCC;
    fn try_from(value: v4l::FourCC) -> Result<Self, Self::Error> {
        match &value.repr {
            b"YUYV" => Ok(Self::YUYV),
            b"RGB8" => Ok(Self::RGB),
            b"RGBA" => Ok(Self::RGBA),
            b"MJPG" => Ok(Self::RGB), // we decode JPEG to RGB
            &repr => Err(UnrecognizedFourCC(repr)),
        }
    }
}
impl TryFrom<ColorSpace> for PixelFormat {
    type Error = ColorSpace;
    fn try_from(value: ColorSpace) -> Result<Self, Self::Error> {
        match value {
            ColorSpace::RGB => Ok(PixelFormat::RGB),
            ColorSpace::RGBA => Ok(PixelFormat::RGBA),
            ColorSpace::Luma => Ok(PixelFormat::LUMA),
            ColorSpace::HSV => Ok(PixelFormat::HSV),
            v => Err(v),
        }
    }
}

/// An error that can occur while decoding image data.
#[derive(Debug, Error)]
pub enum DecodeDataError {
    /// The magic bytes didn't match a recognized format.
    #[error("Unknown image format")]
    UnknownFormat,
    /// An error occurred decoding a PNG image.
    #[error(transparent)]
    Png(#[from] zune_png::error::PngDecodeErrors),
    /// An error occurred decoding a JPEG image.
    #[error(transparent)]
    Jpeg(#[from] zune_jpeg::errors::DecodeErrors),
}
impl From<DecodeDataError> for io::Error {
    fn from(value: DecodeDataError) -> Self {
        io::Error::new(io::ErrorKind::InvalidData, value)
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
impl<'a> Buffer<'a> {
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
        Self::empty(PixelFormat::RGB)
    }
    /// Create a buffer of the given size filled with zeroes.
    pub fn zeroed(width: u32, height: u32, format: PixelFormat) -> Self {
        Self {
            width,
            height,
            format,
            data: vec![0; width as usize * height as usize * format.pixel_size()].into(),
        }
    }
    /// Create a buffer of a single repeated color. `color` must equal `format.pixel_size()`.
    pub fn monochrome(width: u32, height: u32, format: PixelFormat, color: &[u8]) -> Self {
        assert_eq!(format.pixel_size(), color.len());
        Self {
            width,
            height,
            format,
            data: color.repeat(width as usize * height as usize).into(),
        }
    }
    /// Decode data in the PNG format.
    pub fn decode_png_data(data: &[u8]) -> Result<Self, PngDecodeErrors> {
        let _guard = info_span!("decoding PNG image", data.len = data.len()).entered();
        let mut decoder = PngDecoder::new_with_options(
            data,
            DecoderOptions::new_fast()
                .png_set_strip_to_8bit(false)
                .png_set_decode_animated(false),
        );
        decoder
            .decode_headers()
            .inspect_err(|err| error!(%err, "failed to decode PNG headers"))?;
        let (width, height) = decoder.get_dimensions().unwrap();
        let data = decoder
            .decode()
            .inspect_err(|err| error!(%err, "failed to decode PNG image"))?
            .u8()
            .unwrap();
        let space = decoder.get_colorspace().unwrap();
        let Ok(format) = space.try_into() else {
            error!(?space, "unimplemented color space");
            return Err(PngDecodeErrors::GenericStatic("unknown color space")); // should be unreachable?
        };
        Ok(Self {
            width: width as _,
            height: height as _,
            format,
            data: Cow::Owned(data),
        })
    }
    /// Decode data in the PNG format.
    pub fn decode_jpeg_data(data: &[u8]) -> Result<Self, JpegDecodeErrors> {
        let _guard = info_span!("decoding JPEG image", data.len = data.len()).entered();
        let mut decoder = JpegDecoder::new_with_options(
            data,
            DecoderOptions::new_fast().jpeg_set_out_colorspace(ColorSpace::RGB),
        );
        decoder
            .decode_headers()
            .inspect_err(|err| error!(%err, "failed to decode JPEG headers"))?;
        let (width, height) = decoder.dimensions().unwrap();
        let data = decoder
            .decode()
            .inspect_err(|err| error!(%err, "failed to decode JPEG image"))?;
        Ok(Self {
            width: width as _,
            height: height as _,
            format: PixelFormat::RGB,
            data: Cow::Owned(data),
        })
    }
    /// Decode data, guessing the format (currently either PNG or JPEG) from magic bytes.
    pub fn decode_img_data(data: &[u8]) -> Result<Self, DecodeDataError> {
        if data.starts_with(&[0xff, 0xd8]) {
            Self::decode_jpeg_data(data).map_err(From::from)
        } else if data.starts_with(&[0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a]) {
            Self::decode_png_data(data).map_err(From::from)
        } else {
            Err(DecodeDataError::UnknownFormat)
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
        out.width = self.width;
        out.height = self.height;
        let of = out.format;
        let data = out.resize_data();
        if self.format == of {
            data.copy_from_slice(&self.data);
            return;
        }
        par_broadcast2(conv::ConvertBroadcast::new(self.format, of), self, data);
    }
    pub fn convert_inplace(&mut self, to: PixelFormat) {
        use conv::*;
        if self.format == to {
            return;
        }
        if self.format.pixel_size() == to.pixel_size() {
            let old = std::mem::replace(&mut self.format, to);
            if old.is_anon() || to.is_anon() {
                return;
            }
            match (old, to) {
                (PixelFormat::RGB, PixelFormat::HSV) => par_broadcast1(to_inplace(rgb2hsv), self),
                (PixelFormat::RGB, PixelFormat::YCC) => par_broadcast1(to_inplace(rgb2ycc), self),
                (PixelFormat::HSV, PixelFormat::RGB) => par_broadcast1(to_inplace(hsv2rgb), self),
                (PixelFormat::HSV, PixelFormat::YCC) => {
                    par_broadcast1(to_inplace(compose(hsv2rgb, rgb2ycc)), self)
                }
                (PixelFormat::YCC, PixelFormat::RGB) => par_broadcast1(to_inplace(ycc2rgb), self),
                (PixelFormat::YCC, PixelFormat::HSV) => {
                    par_broadcast1(to_inplace(compose(ycc2rgb, rgb2hsv)), self)
                }
                _ => unreachable!("attempted to convert {} to {}", self.format, to),
            }
        } else {
            let buf = self.convert(to);
            *self = buf;
        }
    }
    /// Convert this buffer into one with another format.
    ///
    /// This always allocates a new buffer.
    pub fn convert(&self, format: PixelFormat) -> Buffer<'static> {
        let mut out = Buffer::zeroed(self.width, self.height, format);
        self.convert_into(&mut out);
        out
    }
    /// Convert this buffer into another format, or borrow from this one if the formats match.
    pub fn convert_cow(&self, format: PixelFormat) -> Buffer<'_> {
        if self.format == format {
            self.borrow()
        } else {
            self.convert(format)
        }
    }
    /// Convert this buffer into another format, consuming and possibly reusing this one.
    pub fn converted_into(mut self, format: PixelFormat) -> Self {
        self.convert_inplace(format);
        self
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
        if let Cow::Owned(data) = &mut self.data
            && data.capacity() >= src.data.len()
        {
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
        match &mut self.data {
            Cow::Borrowed(_) => self.data = Cow::Owned(src.data.to_vec()),
            Cow::Owned(data) => {
                data.resize(src.data.len(), 0);
                data.copy_from_slice(&src.data);
            }
        }
    }
    /// Resize the internal data to match the size, shape, and pixel format, returning the mutable buffer.
    pub fn resize_data(&mut self) -> &mut Vec<u8> {
        let len = self.width as usize * self.height as usize * self.format.pixel_size();
        let res = polonius::<_, _, ForLt!(&mut Vec<u8>)>(&mut self.data, |data| {
            if len == 0 {
                if let Cow::Owned(vec) = data {
                    vec.clear();
                    PoloniusResult::Borrowing(vec)
                } else {
                    PoloniusResult::Owned {
                        value: true,
                        input_borrow: Placeholder,
                    }
                }
            } else {
                PoloniusResult::Owned {
                    value: false,
                    input_borrow: Placeholder,
                }
            }
        });
        match res {
            PoloniusResult::Borrowing(vec) => vec,
            PoloniusResult::Owned {
                value: true,
                input_borrow,
            } => {
                *input_borrow = Cow::Owned(Vec::new());
                input_borrow.to_mut()
            }
            PoloniusResult::Owned {
                value: false,
                input_borrow,
            } => {
                let vec = input_borrow.to_mut();
                vec.resize(len, 0);
                vec
            }
        }
    }
    /// Get the slice of data for a single pixel.
    ///
    /// Note that for YUYV images, it returns the pair of pixels that share the data.
    pub fn pixel(&self, mut x: u32, y: u32) -> Option<&[u8]> {
        if self.format == PixelFormat::YUYV {
            x &= !1;
            if x + 1 >= self.width {
                return None;
            }
        }
        if x >= self.width || y >= self.height {
            return None;
        }
        let px_idx = y as usize * self.width as usize + x as usize;
        let px_len = self.format.pixel_size();
        if self.format == PixelFormat::YUYV {
            let start = px_idx * px_len;
            self.data.get(start..(start + 4))
        } else {
            self.data.get((px_idx * px_len)..((px_idx + 1) * px_len))
        }
    }
    /// Get the mutable slice of data for a single pixel.
    ///
    /// Note that for YUYV images, it returns the pair of pixels that share the data.
    pub fn pixel_mut(&mut self, mut x: u32, y: u32) -> Option<&mut [u8]> {
        if self.format == PixelFormat::YUYV {
            x &= !1;
            if x + 1 >= self.width {
                return None;
            }
        }
        if x >= self.width || y >= self.height {
            return None;
        }
        let px_idx = y as usize * self.width as usize + x as usize;
        let px_len = self.format.pixel_size();
        let data = self.data.to_mut();
        if self.format == PixelFormat::YUYV {
            let start = px_idx * px_len;
            data.get_mut(start..(start + 4))
        } else {
            data.get_mut((px_idx * px_len)..((px_idx + 1) * px_len))
        }
    }
    /// Set the pixel with a given color, if it's available.
    ///
    /// This is similar to `self.pixel_mut(x, y)?.copy_from_slice(color)`, but it handles YUYV buffers properly.
    pub fn set_pixel(&mut self, x: u32, y: u32, color: &[u8]) -> bool {
        let is_yuyv = self.format == PixelFormat::YUYV;
        let Some(px) = self.pixel_mut(x, y) else {
            return false;
        };
        if is_yuyv {
            let &[y, u, v] = color else { return false };
            px[1] = u;
            px[3] = v;
            if x & 1 != 0 {
                px[2] = y;
            } else {
                px[0] = y;
            }
        } else {
            px.copy_from_slice(color);
        }
        true
    }
}
