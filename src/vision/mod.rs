use crate::broadcast::{Broadcast2, ParBroadcast2, par_broadcast2};
use crate::buffer::{Buffer, PixelFormat};
use crate::pipeline::prelude::Data;
use rayon::prelude::*;
use serde::{Deserialize, Serialize};
use smallvec::{SmallVec, smallvec};
use std::collections::VecDeque;
use std::fmt::{self, Display, Formatter};
use std::iter::FusedIterator;
use std::ops::{Deref, DerefMut, RangeInclusive};

#[cfg(test)]
mod tests;

#[derive(Clone, Copy)]
pub enum ColorBytes {
    One([u8; 1]),
    Two([u8; 2]),
    Three([u8; 3]),
    Four([u8; 4]),
}
impl Deref for ColorBytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::One(s) => s,
            Self::Two(s) => s,
            Self::Three(s) => s,
            Self::Four(s) => s,
        }
    }
}
impl DerefMut for ColorBytes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::One(s) => s,
            Self::Two(s) => s,
            Self::Three(s) => s,
            Self::Four(s) => s,
        }
    }
}

/// Create a [`ColorBytes`] with the given values.
#[macro_export]
macro_rules! color_bytes {
    [$b0:expr $(,)?] => {
        $crate::vision::ColorBytes::One([$b0])
    };
    [$b0:expr, $b1:expr  $(,)?] => {
        $crate::vision::ColorBytes::Two([$b0, $b1])
    };
    [$b0:expr, $b1:expr, $b2:expr  $(,)?] => {
        $crate::vision::ColorBytes::Three([$b0, $b1, $b2])
    };
    [$b0:expr, $b1:expr, $b2:expr, $b3:expr  $(,)?] => {
        $crate::vision::ColorBytes::Four([$b0, $b1, $b2, $b3])
    };
}

/// A filter, along with a color space to filter in.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(
    rename_all = "lowercase",
    rename_all_fields = "kebab-case",
    tag = "space"
)]
pub enum ColorFilter {
    Luma {
        min_l: u8,
        max_l: u8,
    },
    Rgb {
        min_r: u8,
        min_g: u8,
        min_b: u8,
        max_r: u8,
        max_g: u8,
        max_b: u8,
    },
    Hsv {
        min_h: u8,
        max_h: u8,
        min_s: u8,
        max_s: u8,
        min_v: u8,
        max_v: u8,
    },
    Yuyv {
        min_y: u8,
        max_y: u8,
        min_u: u8,
        max_u: u8,
        min_v: u8,
        max_v: u8,
    },
    #[serde(rename = "ycc")]
    YCbCr {
        min_y: u8,
        max_y: u8,
        min_b: u8,
        max_b: u8,
        min_r: u8,
        max_r: u8,
    },
}
impl ColorFilter {
    pub fn pixel_format(&self) -> PixelFormat {
        match self {
            Self::Luma { .. } => PixelFormat::LUMA,
            Self::Rgb { .. } => PixelFormat::RGB,
            Self::Hsv { .. } => PixelFormat::HSV,
            Self::YCbCr { .. } => PixelFormat::YCC,
            Self::Yuyv { .. } => PixelFormat::YUYV,
        }
    }
    pub fn to_range(self) -> RangeInclusive<Color> {
        match self {
            Self::Luma { min_l, max_l } => Color::Luma { l: min_l }..=Color::Luma { l: max_l },

            Self::Rgb {
                min_r,
                min_g,
                min_b,
                max_r,
                max_g,
                max_b,
            } => {
                Color::Rgb {
                    r: min_r,
                    g: min_g,
                    b: min_b,
                }..=Color::Rgb {
                    r: max_r,
                    g: max_g,
                    b: max_b,
                }
            }

            Self::Hsv {
                min_h,
                max_h,
                min_s,
                max_s,
                min_v,
                max_v,
            } => {
                Color::Hsv {
                    h: min_h,
                    s: min_s,
                    v: min_v,
                }..=Color::Hsv {
                    h: max_h,
                    s: max_s,
                    v: max_v,
                }
            }

            Self::Yuyv {
                min_y,
                max_y,
                min_u,
                max_u,
                min_v,
                max_v,
            } => {
                Color::Yuyv {
                    y: min_y,
                    u: min_u,
                    v: min_v,
                }..=Color::Yuyv {
                    y: max_y,
                    u: max_u,
                    v: max_v,
                }
            }

            Self::YCbCr {
                min_y,
                max_y,
                min_b,
                max_b,
                min_r,
                max_r,
            } => {
                Color::YCbCr {
                    y: min_y,
                    b: min_b,
                    r: min_r,
                }..=Color::YCbCr {
                    y: max_y,
                    b: max_b,
                    r: max_r,
                }
            }
        }
    }
}
impl Display for ColorFilter {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let (min, max) = self.to_range().into_inner();
        write!(f, "{min}..={max}")
    }
}

/// A color, along with its color space.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase", tag = "space")]
pub enum Color {
    Luma {
        l: u8,
    },
    Rgb {
        r: u8,
        g: u8,
        b: u8,
    },
    Hsv {
        h: u8,
        s: u8,
        v: u8,
    },
    Yuyv {
        y: u8,
        u: u8,
        v: u8,
    },
    #[serde(rename = "ycc")]
    YCbCr {
        y: u8,
        b: u8,
        r: u8,
    },
}
impl Color {
    pub fn pixel_format(&self) -> PixelFormat {
        match self {
            Self::Luma { .. } => PixelFormat::LUMA,
            Self::Rgb { .. } => PixelFormat::RGB,
            Self::Hsv { .. } => PixelFormat::HSV,
            Self::YCbCr { .. } => PixelFormat::YCC,
            Self::Yuyv { .. } => PixelFormat::YUYV,
        }
    }
    pub fn bytes(&self) -> ColorBytes {
        match *self {
            Self::Luma { l } => color_bytes![l],
            Self::Rgb { r, g, b } => color_bytes![r, g, b],
            Self::Hsv { h, s, v } => color_bytes![h, s, v],
            Self::Yuyv { y, u, v } => color_bytes![y, u, v],
            Self::YCbCr { y, b, r } => color_bytes![y, b, r],
        }
    }
}
impl Display for Color {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Luma { l } => write!(f, "luma({l})"),
            Self::Rgb { r, g, b } => write!(f, "rgb({r}, {g}, {b})"),
            Self::Hsv { h, s, v } => write!(f, "hsv({h}, {s}, {v})"),
            Self::Yuyv { y, u, v } => write!(f, "yuv({y}, {u}, {v})"),
            Self::YCbCr { y, b, r } => write!(f, "ycc({y}, {b}, {r})"),
        }
    }
}

/// A [`Broadcast2`] implementor that outputs into a black/white image based
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
/// The destination image will have the same dimensions as the source, with a [`Gray`](PixelFormat::Gray) format.
/// A pixel in the range will have a value of 255, and one outside the range will have a value of 0.
pub fn filter_into(mut src: Buffer<'_>, dst: &mut Buffer<'_>, filter: ColorFilter) {
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
    /// Get the width of this blob.
    pub const fn width(&self) -> u32 {
        self.max_x - self.min_x
    }
    /// Get the height of this blob.
    pub const fn height(&self) -> u32 {
        self.max_y - self.min_y
    }
    /// Get the area of this blob.
    pub const fn area(&self) -> u64 {
        self.width() as u64 * self.height() as u64
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
    pub fn filled(&self) -> f64 {
        self.pixels as f64 / self.area() as f64
    }
}
impl Data for Blob {}

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
impl<I: Iterator<Item: IntoIterator<Item = bool>>> FusedIterator for BlobsIterator<I> {}

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

/// A [`Broadcast2`] implementor that reorders the channels in an image, with the input format being YUVY 4:2:2
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
