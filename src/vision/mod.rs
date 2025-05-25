use crate::broadcast::par_broadcast2;
use crate::buffer::{Buffer, PixelFormat};
use serde::{Deserialize, Serialize};
use std::collections::VecDeque;
use std::iter::FusedIterator;

#[cfg(test)]
mod tests;

/// A filter, along with a color space to filter in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ColorFilter {
    #[serde(rename_all = "kebab-case")]
    Luma { min_l: u8, max_l: u8 },
    #[serde(rename_all = "kebab-case")]
    LumaA {
        min_l: u8,
        max_l: u8,
        min_a: u8,
        max_a: u8,
    },
    #[serde(rename_all = "kebab-case")]
    Gray { min_v: u8, max_v: u8 },
    GrayA {
        min_v: u8,
        max_v: u8,
        min_a: u8,
        max_a: u8,
    },
    #[serde(rename_all = "kebab-case")]
    Rgb {
        min_r: u8,
        min_g: u8,
        min_b: u8,
        max_r: u8,
        max_g: u8,
        max_b: u8,
    },
    #[serde(rename_all = "kebab-case")]
    Rgba {
        min_r: u8,
        min_g: u8,
        min_b: u8,
        max_r: u8,
        max_g: u8,
        max_b: u8,
        min_a: u8,
        max_a: u8,
    },
    #[serde(rename_all = "kebab-case")]
    Hsv {
        min_h: u8,
        max_h: u8,
        min_s: u8,
        max_s: u8,
        min_v: u8,
        max_v: u8,
    },
    #[serde(rename_all = "kebab-case")]
    Hsva {
        min_h: u8,
        max_h: u8,
        min_s: u8,
        max_s: u8,
        min_v: u8,
        max_v: u8,
        min_a: u8,
        max_a: u8,
    },
    #[serde(rename_all = "kebab-case")]
    Yuyv {
        min_y: u8,
        max_y: u8,
        min_u: u8,
        max_u: u8,
        min_v: u8,
        max_v: u8,
    },
    #[serde(rename = "ycc", rename_all = "kebab-case")]
    YCbCr {
        min_y: u8,
        max_y: u8,
        min_b: u8,
        max_b: u8,
        min_r: u8,
        max_r: u8,
    },
    #[serde(rename = "ycca", rename_all = "kebab-case")]
    YCbCrA {
        min_y: u8,
        max_y: u8,
        min_b: u8,
        max_b: u8,
        min_r: u8,
        max_r: u8,
        min_a: u8,
        max_a: u8,
    },
}
impl ColorFilter {
    pub fn pixel_format(&self) -> PixelFormat {
        match self {
            Self::Luma { .. } => PixelFormat::Luma,
            Self::LumaA { .. } => PixelFormat::LumaA,
            Self::Gray { .. } => PixelFormat::Gray,
            Self::GrayA { .. } => PixelFormat::GrayA,
            Self::Rgb { .. } => PixelFormat::Rgb,
            Self::Rgba { .. } => PixelFormat::Rgba,
            Self::Hsv { .. } => PixelFormat::Hsv,
            Self::Hsva { .. } => PixelFormat::Hsva,
            Self::YCbCr { .. } => PixelFormat::YCbCr,
            Self::YCbCrA { .. } => PixelFormat::YCbCrA,
            Self::Yuyv { .. } => PixelFormat::Yuyv,
        }
    }
}

#[inline(always)]
fn filter_px<const N: usize>(min: [u8; N], max: [u8; N]) -> impl Fn(&[u8; N], &mut [u8; 1]) {
    move |from, to| {
        let matches = min
            .iter()
            .zip(&max)
            .zip(from)
            .all(|((min, max), val)| (min..=max).contains(&val));
        *to = if matches { [255] } else { [0] };
    }
}

/// Filter an image by color.
///
/// The destination image will have the same dimensions as the source, with a [`Gray`](PixelFormat::Gray) format.
/// A pixel in the range will have a value of 255, and one outside the range will have a value of 0.
pub fn filter_into(mut src: Buffer<'_>, dst: &mut Buffer<'_>, filter: ColorFilter) {
    use tracing::subscriber::*;
    dst.format = PixelFormat::Gray;
    dst.width = src.width;
    dst.height = src.height;
    dst.resize_data();
    with_default(NoSubscriber::new(), || {
        src.convert_inplace(filter.pixel_format())
    });
    match filter {
        ColorFilter::Luma { min_l, max_l } => {
            par_broadcast2(filter_px([min_l], [max_l]), &src, dst)
        }
        ColorFilter::LumaA {
            min_l,
            max_l,
            min_a,
            max_a,
        } => par_broadcast2(filter_px([min_l, min_a], [max_l, max_a]), &src, dst),
        ColorFilter::Gray { min_v, max_v } => {
            par_broadcast2(filter_px([min_v], [max_v]), &src, dst)
        }
        ColorFilter::GrayA {
            min_v,
            max_v,
            min_a,
            max_a,
        } => par_broadcast2(filter_px([min_v, min_a], [max_v, max_a]), &src, dst),
        ColorFilter::Rgb {
            min_r,
            min_g,
            min_b,
            max_r,
            max_g,
            max_b,
        } => par_broadcast2(
            filter_px([min_r, min_g, min_b], [max_r, max_g, max_b]),
            &src,
            dst,
        ),
        ColorFilter::Rgba {
            min_r,
            min_g,
            min_b,
            max_r,
            max_g,
            max_b,
            min_a,
            max_a,
        } => par_broadcast2(
            filter_px([min_r, min_g, min_b, min_a], [max_r, max_g, max_b, max_a]),
            &src,
            dst,
        ),
        ColorFilter::Hsv {
            min_h,
            min_s,
            min_v,
            max_h,
            max_s,
            max_v,
        } => par_broadcast2(
            filter_px([min_h, min_s, min_v], [max_h, max_s, max_v]),
            &src,
            dst,
        ),
        ColorFilter::Hsva {
            min_h,
            min_s,
            min_v,
            max_h,
            max_s,
            max_v,
            min_a,
            max_a,
        } => par_broadcast2(
            filter_px([min_h, min_s, min_v, min_a], [max_h, max_s, max_v, max_a]),
            &src,
            dst,
        ),
        ColorFilter::YCbCr {
            min_y,
            min_b,
            min_r,
            max_y,
            max_b,
            max_r,
        } => par_broadcast2(
            filter_px([min_y, min_b, min_r], [max_y, max_b, max_r]),
            &src,
            dst,
        ),
        ColorFilter::YCbCrA {
            min_y,
            min_b,
            min_r,
            max_y,
            max_b,
            max_r,
            min_a,
            max_a,
        } => par_broadcast2(
            filter_px([min_y, min_b, min_r, min_a], [max_y, max_b, max_r, max_a]),
            &src,
            dst,
        ),
        ColorFilter::Yuyv {
            min_y,
            max_y,
            min_u,
            max_u,
            min_v,
            max_v,
        } => par_broadcast2(
            |&[y1, u, y2, v], [a, b]| {
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
        ),
    }
}

/// Filter an image by color, returning a new image.
///
/// This is the same as [`filter_into`], but it returns a new buffer.
pub fn filter(src: Buffer<'_>, filter: ColorFilter) -> Buffer<'static> {
    let mut dst = Buffer::empty_rgb();
    filter_into(src, &mut dst, filter);
    dst
}

/// A contiguous blob of color in an image—a bounding rectangle and number of contained pixels.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Blob {
    pub min_x: u32,
    pub max_x: u32,
    pub min_y: u32,
    pub max_y: u32,
    pub pixels: usize,
}
impl Blob {
    pub fn from_row(min_x: u32, max_x: u32, y: u32) -> Self {
        Self {
            min_x,
            max_x,
            min_y: y,
            max_y: y + 1,
            pixels: (max_x - min_x) as _,
        }
    }
    /// Merge another blob into this one.
    pub fn absorb(&mut self, other: Self) {
        if other.min_x < self.min_x {
            self.min_x = other.min_x
        }
        if other.max_x > self.max_x {
            self.max_x = other.max_x
        }
        if other.min_y < self.min_y {
            self.min_y = other.min_y
        }
        if other.max_y > self.max_y {
            self.max_y = other.max_y
        }
        self.pixels += other.pixels;
    }
}

#[derive(Debug)]
struct BlobWithBottom {
    blob: Blob,
    min: u32,
    max: u32,
}
impl BlobWithBottom {
    fn from_row(min: u32, max: u32, y: u32) -> Self {
        Self {
            min,
            max,
            blob: Blob::from_row(min, max, y),
        }
    }
    fn eat_new(&mut self, min: u32, max: u32, y: u32) {
        self.min = min;
        self.max = max;
        self.blob.absorb(Blob::from_row(min, max, y));
    }
    fn eat_curr(&mut self, min: u32, max: u32) {
        self.max = max;
        if max > self.blob.max_x {
            self.blob.max_x = max
        }
        self.blob.pixels += (max - min) as usize;
    }
}

#[derive(Debug, Clone, Copy)]
enum State {
    Init,
    EatingFalse,
    EatingTrue(u32),
    Coalescing(u32, u32),
    Draining,
}

struct BlobsInRowState {
    x: u32,
    y: u32,
    remaining: usize,
    state: State,
}
impl BlobsInRowState {
    fn new(y: u32, remaining: usize) -> Self {
        Self {
            x: 0,
            y,
            remaining,
            state: State::Init,
        }
    }
    fn work(
        &mut self,
        incomplete: &mut VecDeque<BlobWithBottom>,
        mut iter: impl FusedIterator<Item = bool>,
    ) -> Option<Blob> {
        loop {
            match self.state {
                State::Init => {
                    self.state = match iter.next() {
                        Some(true) => State::EatingTrue(0),
                        Some(false) => State::EatingFalse,
                        None => State::Draining,
                    };
                }
                State::EatingFalse => loop {
                    self.x += 1;
                    match iter.next() {
                        Some(true) => self.state = State::EatingTrue(self.x),
                        Some(false) => continue,
                        None => self.state = State::Draining,
                    }
                    break;
                },
                State::EatingTrue(start) => loop {
                    self.x += 1;
                    if iter.next() != Some(true) {
                        self.state = State::Coalescing(start, self.x);
                        break;
                    }
                },
                State::Coalescing(min, max) => {
                    if self.remaining < incomplete.len() {
                        let mut blob = incomplete.pop_back().unwrap();
                        if blob.max >= min && blob.min <= max {
                            blob.eat_curr(min, max);
                            while self.remaining > 0 {
                                let b2 = incomplete.pop_front().unwrap();
                                if b2.min > max {
                                    incomplete.push_front(b2);
                                    break;
                                }
                                blob.blob.absorb(b2.blob);
                                self.remaining -= 1;
                            }
                            incomplete.push_back(blob);
                            self.state = State::EatingFalse;
                            continue;
                        } else {
                            incomplete.push_back(blob);
                        }
                    }
                    if self.remaining == 0 {
                        incomplete.push_back(BlobWithBottom::from_row(min, max, self.y));
                    } else {
                        while self.remaining > 0 {
                            let mut blob = incomplete.pop_front().unwrap();
                            if blob.max < min {
                                self.remaining -= 1;
                                return Some(blob.blob);
                            } else if blob.min > max {
                                incomplete.push_front(blob);
                                incomplete.push_back(BlobWithBottom::from_row(min, max, self.y));
                                break;
                            } else {
                                self.remaining -= 1;
                                blob.eat_new(min, max, self.y);
                                while self.remaining > 0 {
                                    let b2 = incomplete.pop_front().unwrap();
                                    if b2.min > max {
                                        incomplete.push_front(b2);
                                        break;
                                    }
                                    blob.blob.absorb(b2.blob);
                                    self.remaining -= 1;
                                }
                                incomplete.push_back(blob);
                                break;
                            }
                        }
                    }
                    self.state = State::EatingFalse;
                }
                State::Draining => {
                    if self.remaining > 0 {
                        self.remaining -= 1;
                        return Some(incomplete.pop_front().unwrap().blob);
                    }
                    return None;
                }
            }
        }
    }
}

pub struct BlobsIterator<I: Iterator<Item: IntoIterator>> {
    iter: I,
    row: Option<<I::Item as IntoIterator>::IntoIter>,
    state: BlobsInRowState,
    incomplete: VecDeque<BlobWithBottom>,
    y: u32,
}
impl<I: Iterator<Item: IntoIterator>> BlobsIterator<I> {
    pub fn new<I2: IntoIterator<IntoIter = I>>(iter: I2) -> Self {
        let mut iter = iter.into_iter();
        let row = iter.next().map(I::Item::into_iter);
        let state = BlobsInRowState::new(0, 0);
        Self {
            iter,
            row,
            state,
            incomplete: VecDeque::new(),
            y: 0,
        }
    }
}
impl<I: Iterator<Item: IntoIterator<Item = bool>>> Iterator for BlobsIterator<I> {
    type Item = Blob;

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            if let Some(row) = &mut self.row {
                let res = self.state.work(&mut self.incomplete, row.fuse());
                if let Some(blob) = res {
                    return Some(blob);
                }
                self.y += 1;
                self.row = self.iter.next().map(I::Item::into_iter);
                self.state = BlobsInRowState::new(self.y, self.incomplete.len());
            } else {
                return self.incomplete.pop_front().map(|b| b.blob);
            }
        }
    }
}
impl<I: Iterator<Item: Iterator<Item = bool>>> FusedIterator for BlobsIterator<I> {}
