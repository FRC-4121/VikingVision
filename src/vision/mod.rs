use crate::broadcast::par_broadcast2;
use crate::buffer::{Buffer, PixelFormat};
use serde::{Deserialize, Serialize};
use smallvec::{SmallVec, smallvec};
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
    fn from_row(min_x: u32, max_x: u32, y: u32) -> Self {
        Self {
            min_x,
            max_x,
            min_y: y,
            max_y: y + 1,
            pixels: (max_x - min_x) as _,
        }
    }
    /// Merge another blob into this one.
    ///
    /// This creates a bounding box that covers both and adds their pixel counts.
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

/// State returned from [`BlobWithBottom::eat`]
#[derive(Debug)]
enum EatState {
    /// We absorbed the range into the current blob, push it back to the front
    Absorbed,
    /// We may have done some rotation, push this to the front and re-order as necessary
    QueueFront,
    /// We're done with this blob and found something, push it to the back
    QueueBack,
    /// We're done with this blob and it's not touching the bottom, push it to the back
    Return,
}

/// A [`Blob`] with tracking for where it touches the bottom row
#[derive(Debug)]
struct BlobWithBottom {
    blob: Blob,
    ranges: SmallVec<[(u32, u32, bool); 2]>,
}
impl BlobWithBottom {
    fn from_row(min: u32, max: u32, y: u32) -> Self {
        Self {
            blob: Blob::from_row(min, max, y),
            ranges: smallvec![(min, max, false)],
        }
    }
    /// Try to absorb this new span into the current blob
    fn eat(&mut self, min: u32, max: u32, y: u32, do_absorb: bool) -> EatState {
        let mut seen_curr = false;
        for _ in 0..self.ranges.len() {
            let (rmin, rmax, curr) = self.ranges[0];
            if curr {
                seen_curr = true;
            }
            if rmax < min {
                if curr {
                    self.ranges.rotate_left(1);
                } else {
                    self.ranges.remove(0);
                }
                continue;
            }
            if rmin > max {
                return EatState::QueueFront;
            }
            if do_absorb {
                self.ranges.push((min, max, true));
                self.blob.absorb(Blob::from_row(min, max, y));
            }
            return EatState::Absorbed;
        }
        if seen_curr {
            if do_absorb {
                self.ranges.retain_mut(|(_, _, v)| std::mem::take(v));
            }
            return EatState::QueueBack;
        }
        if self.ranges.iter().any(|x| x.2) {
            if do_absorb {
                self.ranges.retain_mut(|(_, _, v)| std::mem::take(v));
            }
            EatState::QueueBack
        } else {
            EatState::Return
        }
    }
}

fn search(slice: &[BlobWithBottom], min: u32) -> usize {
    if slice.len() > 4 {
        slice
            .binary_search_by_key(&min, |b| b.ranges[0].0)
            .unwrap_err()
    } else {
        slice
            .iter()
            .position(|i| i.ranges[0].0 > min)
            .unwrap_or(slice.len())
    }
}

/// Rotate the value at the front of an otherwise-sorted dequeue to where it needs to be
fn queue_front(remaining: usize, new_min: u32, incomplete: &mut VecDeque<BlobWithBottom>) -> bool {
    if remaining > 1 {
        let (front, back) = incomplete.as_mut_slices();
        if let Some(slice) = front.get_mut(..remaining) {
            let idx = search(&slice[1..], new_min);
            if idx > 0 {
                slice[..idx].rotate_left(1);
                return true;
            }
            false
        } else {
            let idx = search(&front[1..], new_min);
            if idx < front.len() - 1 {
                front[..idx].rotate_left(1);
                idx > 0
            } else {
                let slice = &mut back[..(remaining - front.len())];
                let idx = search(slice, new_min);
                if idx == 0 {
                    front.rotate_left(1);
                } else {
                    std::mem::swap(&mut slice[0], &mut front[0]);
                    front.rotate_left(1);
                    slice[..idx].rotate_left(1);
                }
                true
            }
        }
    } else {
        false
    }
}

/// Different states the [`BlobsInRowState`] can be in
#[derive(Debug, Clone, Copy)]
enum State {
    /// We just started
    Init,
    /// We're going through background pixels
    EatingFalse,
    /// We're going through foreground pixels, with the given start
    EatingTrue(u32),
    /// We're coalescing the given range into the previous row
    Coalescing(u32, u32),
    /// The row is finished, now we're just draining any old blobs
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
                    if self.remaining == 0 {
                        incomplete.push_back(BlobWithBottom::from_row(min, max, self.y));
                    } else {
                        let mut push = true;
                        while self.remaining > 0 {
                            let mut blob = incomplete.pop_front().unwrap();
                            match blob.eat(min, max, self.y, true) {
                                EatState::Absorbed => {
                                    while self.remaining > 1 {
                                        let mut b2 = incomplete.pop_front().unwrap();
                                        match b2.eat(min, max, self.y, false) {
                                            EatState::Absorbed => {
                                                blob.blob.absorb(b2.blob);
                                                let point = blob.ranges.partition_point(|x| x.2);
                                                blob.ranges.insert_many(
                                                    point,
                                                    b2.ranges.drain_filter(|x| !x.2),
                                                );
                                                let idx = blob.ranges.len();
                                                blob.ranges
                                                    .extend(b2.ranges.into_iter().filter(|x| x.2));
                                                blob.ranges[idx..].sort_unstable_by_key(|r| r.0);
                                                self.remaining -= 1;
                                            }
                                            EatState::QueueFront => {
                                                let new_min = b2.ranges[0].0;
                                                incomplete.push_front(b2);
                                                let acted = queue_front(
                                                    self.remaining - 1,
                                                    new_min,
                                                    incomplete,
                                                );
                                                if !acted {
                                                    break;
                                                }
                                            }
                                            EatState::QueueBack | EatState::Return => {
                                                unreachable!()
                                            }
                                        }
                                    }
                                    incomplete.push_front(blob);
                                    push = false;
                                    break;
                                }
                                EatState::QueueFront => {
                                    let new_min = blob.ranges[0].0;
                                    incomplete.push_front(blob);
                                    queue_front(self.remaining, new_min, incomplete);
                                    incomplete
                                        .push_back(BlobWithBottom::from_row(min, max, self.y));
                                    push = false;
                                    break;
                                }
                                EatState::QueueBack => {
                                    self.remaining -= 1;
                                    incomplete.push_back(blob);
                                }
                                EatState::Return => {
                                    self.remaining -= 1;
                                    return Some(blob.blob);
                                }
                            }
                        }
                        if push {
                            incomplete.push_back(BlobWithBottom::from_row(min, max, self.y));
                        }
                    }
                    self.state = State::EatingFalse;
                }
                State::Draining => {
                    while self.remaining > 0 {
                        let mut blob = incomplete.pop_front().unwrap();
                        self.remaining -= 1;
                        blob.ranges.retain_mut(|(_, _, c)| std::mem::take(c));
                        if blob.ranges.is_empty() {
                            return Some(blob.blob);
                        } else {
                            while blob.ranges[0].0 > blob.ranges.last().unwrap().0 {
                                blob.ranges.rotate_left(1);
                            }
                            incomplete.push_back(blob);
                        }
                    }
                    return None;
                }
            }
        }
    }
}

/// An iterator over the blobs in an "image".
///
/// This iterator wraps an iterator whose items implement [`IntoIterator`], which should in turn have items that are `bool`.
/// This gives the maximum flexibility since it makes no assumptions about where the image came from or how its pixels are stored.
/// If for some reason the rows are different lengths, it'll still work, and it'll pad the rest of the row as being part of the background.
///
/// For the simplest, most common cases, this will only allocate its queue for incomplete blobs, with a space complexity being proportional
/// to the maximum number of blobs in a row. Certain pathological patterns, where the bottom of a connected blob splits apart, can lead to more
/// allocation, but it's still bounded by a space complexity linear to the width of the image.
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
