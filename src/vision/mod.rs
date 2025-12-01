//! Vision algorithms and utilities

use std::cell::Cell;

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

#[repr(transparent)]
struct AssertSync<T>(T);
unsafe impl<T> Send for AssertSync<T> {}
unsafe impl<T> Sync for AssertSync<T> {}

/// Box blur an image.
///
/// The width and height must be odd numbers.
/// This blurs the image in-place, with an auxiliary buffer.
pub fn box_blur(img: &mut Buffer<'_>, aux: &mut Buffer<'_>, width: usize, height: usize) {
    assert_ne!(
        img.format,
        PixelFormat::YUYV,
        "Box blurring isn't implemented for YUYV images"
    );
    assert!(width & 1 == 1, "Window width must be an odd number");
    assert!(height & 1 == 1, "Window height must be an odd number");
    if width <= 1 && height <= 1 {
        return;
    }
    aux.width = img.width;
    aux.height = img.height;
    aux.format = img.format;
    let pxlen = img.format.pixel_size();
    let buf_width = img.width as usize;
    let buf_height = img.height as usize;
    let src = img.resize_data();
    let dst = aux.resize_data();
    let xmul = pxlen;
    let ymul = buf_width * pxlen;
    if height > 1 {
        let half = height / 2;
        {
            let dst = unsafe { std::mem::transmute::<&[u8], &[AssertSync<Cell<u8>>]>(dst) };
            (0..buf_width).into_par_iter().for_each_init(
                || [0; 200],
                |buf, x| {
                    for y in 0..buf_height {
                        let px_start = y * ymul + x * xmul;
                        let px_end = px_start + pxlen;
                        let px = unsafe { dst.get_unchecked(px_start..px_end) }; // already bouds checked
                        let min = y.saturating_sub(half);
                        let max = y.saturating_add(half + 1).min(buf_height);
                        buf[..pxlen].fill(0);
                        for y2 in min..max {
                            let px_start = y2 * ymul + x * xmul;
                            let px_end = px_start + pxlen;
                            let px = unsafe { src.get_unchecked(px_start..px_end) }; // already bouds checked
                            for (sum, chan) in buf.iter_mut().zip(px) {
                                *sum += *chan as usize;
                            }
                        }
                        let size = max - min;
                        for (sum, chan) in buf.iter().zip(px) {
                            chan.0.set((sum / size) as u8);
                        }
                    }
                },
            );
        }
        std::mem::swap(src, dst);
    }
    if width > 1 {
        let half = width / 2;
        {
            let dst = unsafe { std::mem::transmute::<&[u8], &[AssertSync<Cell<u8>>]>(dst) };
            (0..buf_height).into_par_iter().for_each_init(
                || [0; 200],
                |buf, y| {
                    for x in 0..buf_width {
                        let px_start = y * ymul + x * xmul;
                        let px_end = px_start + pxlen;
                        let px = unsafe { dst.get_unchecked(px_start..px_end) }; // already bouds checked
                        let min = x.saturating_sub(half);
                        let max = x.saturating_add(half + 1).min(buf_width);
                        buf[..pxlen].fill(0);
                        for x2 in min..max {
                            let px_start = y * ymul + x2 * xmul;
                            let px_end = px_start + pxlen;
                            let px = unsafe { src.get_unchecked(px_start..px_end) }; // already bouds checked
                            for (sum, chan) in buf.iter_mut().zip(px) {
                                *sum += *chan as usize;
                            }
                        }
                        let size = max - min;
                        for (sum, chan) in buf.iter().zip(px) {
                            chan.0.set((sum / size) as u8);
                        }
                    }
                },
            );
        }
        std::mem::swap(src, dst);
    }
}

const GAUSSIAN_SCALE: f32 = 26145.082; // 65536/sqrt(2pi)

/// Box blur an image.
///
/// The width and height must be odd numbers.
/// This blurs the image in-place, with an auxiliary buffer.
pub fn gaussian_blur(
    img: &mut Buffer<'_>,
    aux: &mut Buffer<'_>,
    sigma: f32,
    width: usize,
    height: usize,
) {
    assert_ne!(
        img.format,
        PixelFormat::YUYV,
        "Gaussian blurring isn't implemented for YUYV images"
    );
    assert!(width & 1 == 1, "Window width must be an odd number");
    assert!(height & 1 == 1, "Window height must be an odd number");
    if width <= 1 && height <= 1 {
        return;
    }
    aux.width = img.width;
    aux.height = img.height;
    aux.format = img.format;
    let pxlen = img.format.pixel_size();
    let buf_width = img.width as usize;
    let buf_height = img.height as usize;
    let src = img.resize_data();
    let dst = aux.resize_data();
    let xmul = pxlen;
    let ymul = buf_width * pxlen;
    let scale = GAUSSIAN_SCALE / sigma;
    let half_width = width / 2;
    let half_height = height / 2;
    let coeffs = (0..=half_width.max(half_height))
        .map(|v| (std::f32::consts::E.powf((v as f32).powi(-2)) * scale) as usize)
        .collect::<Vec<_>>();
    let cums = coeffs
        .iter()
        .scan(0, |acc, x| {
            *acc += x;
            Some(*acc)
        })
        .collect::<Vec<_>>();
    if height > 1 {
        let dst_cells = unsafe { std::mem::transmute::<&[u8], &[AssertSync<Cell<u8>>]>(dst) };
        (0..buf_width).into_par_iter().for_each_init(
            || [0; 200],
            |buf, x| {
                for y in 0..buf_height {
                    let px_start = y * ymul + x * xmul;
                    let px_end = px_start + pxlen;
                    let px = unsafe { dst_cells.get_unchecked(px_start..px_end) }; // already bouds checked
                    let min = y.saturating_sub(half_height);
                    let max = y.saturating_add(half_height + 1).min(buf_height);
                    buf[..pxlen].fill(0);
                    for y2 in min..max {
                        let px_start = y2 * ymul + x * xmul;
                        let px_end = px_start + pxlen;
                        let px = unsafe { src.get_unchecked(px_start..px_end) }; // already bouds checked
                        let c = coeffs[y2.abs_diff(y)];
                        for (sum, chan) in buf.iter_mut().zip(px) {
                            *sum += *chan as usize * c;
                        }
                    }
                    let total = cums[max - y - 1] + cums[y - min];
                    for (sum, chan) in buf.iter().zip(px) {
                        chan.0.set((sum / total) as u8);
                    }
                }
            },
        );
        std::mem::swap(src, dst);
    }
    if width > 1 {
        let dst_cells = unsafe { std::mem::transmute::<&[u8], &[AssertSync<Cell<u8>>]>(dst) };
        (0..buf_height).into_par_iter().for_each_init(
            || [0; 200],
            |buf, y| {
                for x in 0..buf_width {
                    let px_start = y * ymul + x * xmul;
                    let px_end = px_start + pxlen;
                    let px = unsafe { dst_cells.get_unchecked(px_start..px_end) }; // already bouds checked
                    let min = x.saturating_sub(half_width);
                    let max = x.saturating_add(half_width + 1).min(buf_width);
                    buf[..pxlen].fill(0);
                    for x2 in min..max {
                        let px_start = y * ymul + x2 * xmul;
                        let px_end = px_start + pxlen;
                        let px = unsafe { src.get_unchecked(px_start..px_end) }; // already bouds checked
                        let c = coeffs[x2.abs_diff(x)];
                        for (sum, chan) in buf.iter_mut().zip(px) {
                            *sum += *chan as usize * c;
                        }
                    }
                    let total = cums[max - x - 1] + cums[x - min] - cums[0];
                    for (sum, chan) in buf.iter().zip(px) {
                        chan.0.set((sum / total) as u8);
                    }
                }
            },
        );
        std::mem::swap(src, dst);
    }
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
