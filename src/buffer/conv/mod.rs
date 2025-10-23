//! All of these conversion functions take an input and output array and can be used directly with [`broadcast2`](crate::broadcast::broadcast2) and [`par_broadcast2`](crate::broadcast::par_broadcast2).
//! All functions have the convention of `<input format>2<output format>``, with `i` being used as a function prefix for in-place operations.
//! Note that any YUYV conversions need two pixels to operate on rather than just one.

use std::cmp::Ordering;

use crate::broadcast::{Broadcast2, ParBroadcast2};
use crate::buffer::PixelFormat;

#[cfg(test)]
mod tests;

/// Sequence two in-place operations.
#[inline(always)]
pub fn sequence<const N: usize>(
    f1: impl Fn(&mut [u8; N]) + Send + Sync,
    f2: impl Fn(&mut [u8; N]) + Send + Sync,
) -> impl Fn(&mut [u8; N]) + Send + Sync {
    move |buf| {
        f1(buf);
        f2(buf);
    }
}
/// Make a conversion operate in place
#[inline(always)]
pub fn to_inplace<const N: usize>(f: impl Fn([u8; N]) -> [u8; N]) -> impl Fn(&mut [u8; N]) {
    move |buf| *buf = f(*buf)
}
#[inline(always)]
pub fn compose<A, B, C>(
    f1: impl Fn(A) -> B + Send + Sync,
    f2: impl Fn(B) -> C + Send + Sync,
) -> impl Fn(A) -> C + Send + Sync {
    move |x| f2(f1(x))
}

pub fn hsv2rgb(from: [u8; 3]) -> [u8; 3] {
    let [h, s, v] = from.map(|c| c as u16);
    if s == 0 {
        return [v as _; 3];
    }
    let region = h / 43;
    let c = (v * s) >> 8;
    let x = (c * (43 - (h % 85).abs_diff(43))) / 43;
    let m = v - c;
    let c = (c + m).clamp(0, 255) as u8;
    let x = (x + m).clamp(0, 255) as u8;
    let m = m as u8;
    match region {
        0 => [c, x, m],
        1 => [x, c, m],
        2 => [m, c, x],
        3 => [m, x, c],
        4 => [x, m, c],
        _ => [c, m, x],
    }
}
pub fn rgb2hsv(from: [u8; 3]) -> [u8; 3] {
    let [r, g, b] = from.map(|c| c as i16);
    let max = r.max(g).max(b);
    let min = r.min(g).min(b);
    let delta = max - min;
    let v = max as u8;
    if delta == 0 {
        [0, 0, v]
    } else {
        let s = ((255 * delta) / max) as u8;
        #[allow(clippy::identity_op)]
        let h16 = if max == r {
            0 + 43 * (g - b) / delta
        } else if max == g {
            85 + 43 * (b - r) / delta
        } else {
            171 + 43 * (r - g) / delta
        };
        let h = (h16 & 255) as u8;
        [h, s, v]
    }
}
pub fn rgb2ycc(from: [u8; 3]) -> [u8; 3] {
    let [r, g, b] = from.map(|c| c as i32);
    let y = ((r * 77 + g * 150 + b * 29) / 256).clamp(0, 255) as u8;
    let cb = ((-43 * r - 85 * g + 128 * b) / 256 + 128).clamp(0, 255) as u8;
    let cr = ((128 * r - 107 * g - 21 * b) / 256 + 128).clamp(0, 255) as u8;
    [y, cb, cr]
}
pub fn rgb2luma(from: [u8; 3]) -> u8 {
    let [r, g, b] = from.map(|c| c as u16);
    ((r * 77 + g * 150 + b * 29) >> 8).min(255) as u8
}
pub fn ycc2rgb(from: [u8; 3]) -> [u8; 3] {
    let [y, cb, cr] = from.map(|c| c as i32);
    let r = ((256 * y + 359 * (cr - 128)) / 256).clamp(0, 255) as u8;
    let g = ((256 * y - (88 * (cb - 128) + 183 * (cr - 128))) / 256).clamp(0, 255) as u8;
    let b = ((256 * y + 454 * (cb - 128)) / 256).clamp(0, 255) as u8;
    [r, g, b]
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct ConvertBroadcast {
    pub src: PixelFormat,
    pub dst: PixelFormat,
}
impl ConvertBroadcast {
    pub const fn new(src: PixelFormat, dst: PixelFormat) -> Self {
        Self { src, dst }
    }
}
impl Broadcast2<&[u8], &mut [u8], ()> for ConvertBroadcast {
    fn sizes(&self) -> [usize; 2] {
        match (self.src, self.dst) {
            (PixelFormat::YUYV, PixelFormat::YUYV) => [4, 4],
            (PixelFormat::YUYV, dst) => [4, dst.pixel_size() * 2],
            (src, PixelFormat::YUYV) => [src.pixel_size() * 2, 4],
            (src, dst) => [src.pixel_size(), dst.pixel_size()],
        }
    }
    fn run(&mut self, a1: &[u8], a2: &mut [u8]) {
        self.par_run(a1, a2);
    }
}
impl ParBroadcast2<&[u8], &mut [u8], ()> for ConvertBroadcast {
    fn par_run(&self, a1: &[u8], a2: &mut [u8]) {
        if self.src == self.dst {
            a2.copy_from_slice(a1);
        }
        if self.src.is_anon() || self.dst.is_anon() {
            match a1.len().cmp(&a2.len()) {
                Ordering::Less => {
                    let (head, tail) = a2.split_at_mut(a1.len());
                    head.copy_from_slice(a1);
                    tail.fill(0);
                }
                Ordering::Greater => {
                    a2.copy_from_slice(&a1[..a2.len()]);
                }
                Ordering::Equal => a2.copy_from_slice(a1),
            }
            return;
        }
        match self.src {
            PixelFormat::LUMA => {
                let y = a1[0];
                match self.dst {
                    PixelFormat::RGB => {
                        a2.fill(y);
                    }
                    PixelFormat::HSV => {
                        a2.copy_from_slice(&rgb2hsv([y; 3]));
                    }
                    PixelFormat::YCC => {
                        if let Some((first, rest)) = a2.split_first_mut() {
                            *first = y;
                            rest.fill(128);
                        }
                    }
                    PixelFormat::RGBA => {
                        a2[..3].fill(y);
                        a2[3] = 255;
                    }
                    PixelFormat::YUYV => {
                        let y1 = y;
                        let y2 = a1[1];
                        a2.copy_from_slice(&[y1, 128, y2, 128]);
                    }
                    _ => {}
                }
            }
            PixelFormat::RGB => {
                let mut rgb = [0; 3];
                rgb.copy_from_slice(&a1[..3]);
                match self.dst {
                    PixelFormat::LUMA => {
                        a2[0] = rgb2luma(rgb);
                    }
                    PixelFormat::HSV => {
                        a2.copy_from_slice(&rgb2hsv(rgb));
                    }
                    PixelFormat::YCC => {
                        a2.copy_from_slice(&rgb2ycc(rgb));
                    }
                    PixelFormat::RGBA => {
                        a2[..3].copy_from_slice(&rgb);
                        a2[3] = 255;
                    }
                    PixelFormat::YUYV => {
                        let [y1, b1, r1] = rgb2ycc(rgb);
                        rgb.copy_from_slice(&a1[3..6]);
                        let [y2, b2, r2] = rgb2ycc(rgb);
                        let r = r1.midpoint(r2);
                        let b = b1.midpoint(b2);
                        a2.copy_from_slice(&[y1, b, y2, r]);
                    }
                    _ => {}
                }
            }
            PixelFormat::HSV => {
                let mut hsv = [0; 3];
                hsv.copy_from_slice(&a1[..3]);
                let rgb = hsv2rgb(hsv);
                match self.dst {
                    PixelFormat::LUMA => {
                        a2[0] = rgb2luma(rgb);
                    }
                    PixelFormat::RGB => {
                        a2.copy_from_slice(&rgb);
                    }
                    PixelFormat::YCC => {
                        a2.copy_from_slice(&rgb2ycc(rgb));
                    }
                    PixelFormat::RGBA => {
                        a2[..3].copy_from_slice(&rgb);
                        a2[3] = 255;
                    }
                    PixelFormat::YUYV => {
                        let [y1, b1, r1] = rgb2ycc(rgb);
                        hsv.copy_from_slice(&a1[3..6]);
                        let rgb = hsv2rgb(hsv);
                        let [y2, b2, r2] = rgb2ycc(rgb);
                        let r = r1.midpoint(r2);
                        let b = b1.midpoint(b2);
                        a2.copy_from_slice(&[y1, b, y2, r]);
                    }
                    _ => {}
                }
            }
            PixelFormat::YCC => {
                let mut ycc = [0; 3];
                ycc.copy_from_slice(&a1[..3]);
                match self.dst {
                    PixelFormat::LUMA => {
                        a2[0] = rgb2luma(ycc2rgb(ycc));
                    }
                    PixelFormat::RGB => {
                        a2.copy_from_slice(&ycc2rgb(ycc));
                    }
                    PixelFormat::HSV => {
                        a2.copy_from_slice(&rgb2hsv(ycc2rgb(ycc)));
                    }
                    PixelFormat::RGBA => {
                        let rgb = ycc2rgb(ycc);
                        a2[..3].copy_from_slice(&rgb);
                        a2[3] = 255;
                    }
                    PixelFormat::YUYV => {
                        let [y1, b1, r1] = ycc;
                        ycc.copy_from_slice(&a1[3..6]);
                        let [y2, b2, r2] = ycc;
                        let r = r1.midpoint(r2);
                        let b = b1.midpoint(b2);
                        a2.copy_from_slice(&[y1, b, y2, r]);
                    }
                    _ => {}
                }
            }
            PixelFormat::RGBA => {
                let mut rgb = [0; 3];
                rgb.copy_from_slice(&a1[..3]);
                match self.dst {
                    PixelFormat::LUMA => {
                        a2[0] = rgb2luma(rgb);
                    }
                    PixelFormat::RGB => {
                        a2.copy_from_slice(&rgb);
                    }
                    PixelFormat::HSV => {
                        a2.copy_from_slice(&rgb2hsv(rgb));
                    }
                    PixelFormat::YCC => {
                        a2.copy_from_slice(&rgb2ycc(rgb));
                    }
                    PixelFormat::YUYV => {
                        let [y1, b1, r1] = rgb2ycc(rgb);
                        rgb.copy_from_slice(&a1[4..7]);
                        let [y2, b2, r2] = rgb2ycc(rgb);
                        let r = r1.midpoint(r2);
                        let b = b1.midpoint(b2);
                        a2.copy_from_slice(&[y1, b, y2, r]);
                    }
                    _ => {}
                }
            }
            PixelFormat::YUYV => {
                let mut yuyv = [0; 4];
                yuyv.copy_from_slice(a1);
                let [y1, u, y2, v] = yuyv;
                match self.dst {
                    PixelFormat::LUMA => {
                        a2.copy_from_slice(&[y1, y2]);
                    }
                    PixelFormat::YCC => {
                        a2.copy_from_slice(&[y1, u, v, y2, u, v]);
                    }
                    PixelFormat::RGB => {
                        let [r1, g1, b1] = ycc2rgb([y1, u, v]);
                        let [r2, g2, b2] = ycc2rgb([y2, u, v]);
                        a2.copy_from_slice(&[r1, g1, b1, r2, g2, b2]);
                    }
                    PixelFormat::RGBA => {
                        let [r1, g1, b1] = ycc2rgb([y1, u, v]);
                        let [r2, g2, b2] = ycc2rgb([y2, u, v]);
                        a2.copy_from_slice(&[r1, g1, b1, 255, r2, g2, b2, 255]);
                    }
                    PixelFormat::HSV => {
                        let [h1, s1, v1] = rgb2hsv(ycc2rgb([y1, u, v]));
                        let [h2, s2, v2] = rgb2hsv(ycc2rgb([y2, u, v]));
                        a2.copy_from_slice(&[h1, s1, v1, h2, s2, v2]);
                    }
                    _ => {}
                }
            }
            _ => {}
        }
    }
}
