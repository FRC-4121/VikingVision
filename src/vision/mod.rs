//! Vision algorithms and utilities

use crate::broadcast::{Broadcast2, ParBroadcast2, par_broadcast2};
use crate::buffer::{Buffer, PixelFormat};
use rayon::prelude::*;

mod blobs;
mod color;
#[cfg(test)]
mod tests;

pub use blobs::*;
pub use color::*;

/// A [`Broadcast2`] implementor that outputs into a black/white image based on a minimum and maximum channel range
#[derive(Debug, Clone, Copy)]
pub struct FilterPixel<'a> {
    pub min: &'a [u8],
    pub max: &'a [u8],
}
impl<'a> FilterPixel<'a> {
    #[inline(always)]
    pub const fn new(min: &'a [u8], max: &'a [u8]) -> Self {
        Self { min, max }
    }
}
impl Broadcast2<&[u8], &mut [u8], ()> for FilterPixel<'_> {
    fn sizes(&self) -> [usize; 2] {
        [self.min.len().min(self.max.len()), 1]
    }
    fn run(&mut self, a1: &[u8], a2: &mut [u8]) {
        self.par_run(a1, a2);
    }
}
impl ParBroadcast2<&[u8], &mut [u8], ()> for FilterPixel<'_> {
    fn par_run(&self, a1: &[u8], a2: &mut [u8]) {
        let matches = self
            .min
            .iter()
            .zip(self.max)
            .zip(a1)
            .all(|((min, max), val)| (min..=max).contains(&val));
        a2[0] = if matches { 255 } else { 0 };
    }
}

/// Filter an image by color.
///
/// The destination image will have the same dimensions as the source, with a [`LUMA`](PixelFormat::LUMA) format.
/// A pixel in the range will have a value of 255, and one outside the range will have a value of 0.
pub fn color_filter(mut src: Buffer<'_>, dst: &mut Buffer<'_>, filter: ColorFilter) {
    dst.format = PixelFormat::LUMA;
    dst.width = src.width;
    dst.height = src.height;
    src.convert_inplace(filter.pixel_format());
    if let ColorFilter::Yuyv {
        min_y,
        max_y,
        min_u,
        max_u,
        min_v,
        max_v,
    } = filter
    {
        par_broadcast2(
            |&[y1, u, y2, v]: &[u8; 4], [a, b]: &mut [u8; 2]| {
                if u < min_u || u > max_u || v < min_v || v > max_v {
                    *a = 0;
                    *b = 0;
                } else {
                    let yr = min_y..=max_y;
                    *a = if yr.contains(&y1) { 255 } else { 0 };
                    *b = if yr.contains(&y2) { 255 } else { 0 };
                }
            },
            &src,
            dst,
        )
    } else {
        let (min, max) = filter.to_range().into_inner();
        par_broadcast2(
            FilterPixel::new(&min.bytes(), &max.bytes()),
            &src,
            dst.resize_data(),
        );
    }
}

/// Percentile filter an image.
///
/// The width and height must be odd numbers, and the index must be less than their product.
/// An index of 0 is a dilation, an index of `(width * height)` is an erosion, and `(width * height / 2)` is a median filter.
/// The output buffer will have the same dimensions and format as the input buffer.
pub fn percentile_filter(
    src: Buffer<'_>,
    dst: &mut Buffer<'_>,
    width: usize,
    height: usize,
    index: usize,
) {
    assert_ne!(
        src.format,
        PixelFormat::YUYV,
        "Percentile filtering isn't implemented for YUYV images"
    );
    assert!(width & 1 == 1, "Window width must be an odd number");
    assert!(height & 1 == 1, "Window height must be an odd number");
    assert!(
        index < width * height,
        "Pixel index {index} is out of range for a {width}x{height} window"
    );
    dst.width = src.width;
    dst.height = src.height;
    dst.format = src.format;
    let data = dst.resize_data();
    if width == 1 && height == 1 {
        data.copy_from_slice(&src.data);
    }
    let pxlen = src.format.pixel_size();
    let buf_len = width * height;
    let buf_width = src.width as usize;
    let buf_height = src.height as usize;
    let half_width = width / 2;
    let half_height = height / 2;
    data.par_chunks_mut(pxlen).enumerate().for_each_init(
        || vec![Vec::with_capacity(buf_len); pxlen],
        |bufs, (n, px)| {
            bufs.iter_mut().for_each(Vec::clear);
            let y = n / buf_width;
            let x = n % buf_width;
            for y in y.saturating_sub(half_height)..=std::cmp::min(y + half_height, buf_height - 1)
            {
                for x in x.saturating_sub(half_width)..=std::cmp::min(x + half_width, buf_width - 1)
                {
                    for (buf, &val) in bufs.iter_mut().zip(src.pixel(x as _, y as _).unwrap()) {
                        buf.push(val);
                    }
                }
            }
            for (buf, val) in bufs.iter_mut().zip(px) {
                buf.sort_unstable();
                if buf.len() == buf_len {
                    *val = buf[index];
                } else {
                    *val = buf[index * buf.len() / buf_len];
                }
            }
        },
    );
}

/// Box blur an image.
///
/// The width and height must be odd numbers.
/// The output buffer will have the same dimensions and format as the input buffer.
pub fn box_blur(src: Buffer<'_>, dst: &mut Buffer<'_>, width: usize, height: usize) {
    assert_ne!(
        src.format,
        PixelFormat::YUYV,
        "Box blurring isn't implemented for YUYV images"
    );
    assert!(width & 1 == 1, "Window width must be an odd number");
    assert!(height & 1 == 1, "Window height must be an odd number");
    dst.width = src.width;
    dst.height = src.height;
    dst.format = src.format;
    let data = dst.resize_data();
    if width == 1 && height == 1 {
        data.copy_from_slice(&src.data);
    }
    if src.width == 0 || src.height == 0 {
        return;
    }
    let pxlen = src.format.pixel_size();
    let buf_len = width * height;
    let buf_width = src.width as usize;
    let buf_height = src.height as usize;
    let half_width = width / 2;
    let half_height = height / 2;
    data.par_chunks_mut(pxlen).enumerate().for_each_init(
        || vec![Vec::with_capacity(buf_len); pxlen],
        |bufs, (n, px)| {
            bufs.iter_mut().for_each(Vec::clear);
            let y = n / buf_width;
            let x = n % buf_width;
            for y in y.saturating_sub(half_height)..=std::cmp::min(y + half_height, buf_height - 1)
            {
                for x in x.saturating_sub(half_width)..=std::cmp::min(x + half_width, buf_width - 1)
                {
                    for (buf, &val) in bufs.iter_mut().zip(src.pixel(x as _, y as _).unwrap()) {
                        buf.push(val);
                    }
                }
            }
            for (buf, val) in bufs.iter_mut().zip(px) {
                *val = (buf.iter().map(|i| *i as usize).sum::<usize>() / buf.len()) as u8;
            }
        },
    );
}

/// A [`Broadcast2`] implementor that reorders the channels of an image
#[derive(Debug, Clone, Copy)]
pub struct Swizzle<'a> {
    pub num_in: u8,
    pub extract: &'a [u8],
}
impl<'a> Swizzle<'a> {
    pub const fn new(num_in: u8, extract: &'a [u8]) -> Self {
        Self { num_in, extract }
    }
}
impl Broadcast2<&[u8], &mut [u8], ()> for Swizzle<'_> {
    fn sizes(&self) -> [usize; 2] {
        [self.num_in as _, self.extract.len()]
    }
    fn run(&mut self, a1: &[u8], a2: &mut [u8]) {
        self.par_run(a1, a2);
    }
}
impl ParBroadcast2<&[u8], &mut [u8], ()> for Swizzle<'_> {
    fn par_run(&self, a1: &[u8], a2: &mut [u8]) {
        for (from, to) in self.extract.iter().zip(a2) {
            *to = a1.get(*from as usize).copied().unwrap_or(0);
        }
    }
}

/// A [`Broadcast2`] implementor that reorders the channels in an image, with the input format being YUYV 4:2:2
#[derive(Debug, Clone, Copy)]
pub struct YuyvSwizzle<'a> {
    pub extract: &'a [u8],
}
impl<'a> YuyvSwizzle<'a> {
    pub const fn new(extract: &'a [u8]) -> Self {
        Self { extract }
    }
}
impl Broadcast2<&[u8], &mut [u8], ()> for YuyvSwizzle<'_> {
    fn sizes(&self) -> [usize; 2] {
        [4, self.extract.len() * 2]
    }
    fn run(&mut self, a1: &[u8], a2: &mut [u8]) {
        self.par_run(a1, a2);
    }
}
impl ParBroadcast2<&[u8], &mut [u8], ()> for YuyvSwizzle<'_> {
    fn par_run(&self, a1: &[u8], a2: &mut [u8]) {
        let mut yuyv = [0; 4];
        yuyv.copy_from_slice(a1);
        let [y1, u, y2, v] = yuyv;
        let (px1, px2) = a2.split_at_mut(self.extract.len());
        for ((from, to1), to2) in self.extract.iter().zip(px1).zip(px2) {
            match *from {
                0 => {
                    *to1 = y1;
                    *to2 = y2;
                }
                1 => {
                    *to1 = u;
                    *to2 = u;
                }
                2 => {
                    *to1 = v;
                    *to2 = v;
                }
                _ => {
                    *to1 = 0;
                    *to2 = 0;
                }
            }
        }
    }
}

/// Reorder the channels in an image
pub fn swizzle(src: Buffer<'_>, dst: &mut Buffer<'_>, extract: &[u8]) {
    let Some(fmt) = extract.len().try_into().ok().and_then(PixelFormat::anon) else {
        panic!(
            "Swizzling needs to extract into 1..={} channels, got {}",
            PixelFormat::MAX_ANON,
            extract.len()
        );
    };
    dst.format = fmt;
    dst.width = src.width;
    dst.height = src.height;
    let data = dst.resize_data();
    if src.format == PixelFormat::YUYV {
        par_broadcast2(YuyvSwizzle::new(extract), &src, data);
    } else {
        par_broadcast2(
            Swizzle::new(src.format.pixel_size() as _, extract),
            &src,
            data,
        );
    }
}
