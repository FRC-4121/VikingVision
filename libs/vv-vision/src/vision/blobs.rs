// use crate::pipeline::prelude::Data;
use smallvec::{SmallVec, smallvec};
// use std::borrow::Cow;
use std::collections::VecDeque;
use std::iter::FusedIterator;
// use std::sync::Arc;

fn filter_pixel(v: &[u8]) -> bool {
    v.iter().any(|&v| v > 0)
}

/// An iterator over the pixels in a row, mapped so that all-zero pixels give false and all others give true.
#[derive(Clone)]
pub struct FilterRow<'a>(std::slice::Chunks<'a, u8>);
impl Iterator for FilterRow<'_> {
    type Item = bool;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(filter_pixel)
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}
impl ExactSizeIterator for FilterRow<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }
}
impl FusedIterator for FilterRow<'_> {}
impl DoubleEndedIterator for FilterRow<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back().map(filter_pixel)
    }
}

/// A 2D iterator over the pixels in an image, suitable to pass into [`BlobsIterator`].
#[derive(Clone)]
pub struct PixelsIterator<'a>(std::slice::Chunks<'a, u8>, usize);
impl<'a> Iterator for PixelsIterator<'a> {
    type Item = FilterRow<'a>;
    fn next(&mut self) -> Option<Self::Item> {
        self.0.next().map(|r| FilterRow(r.chunks(self.1)))
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        self.0.size_hint()
    }
}
impl ExactSizeIterator for PixelsIterator<'_> {
    fn len(&self) -> usize {
        self.0.len()
    }
}
impl FusedIterator for PixelsIterator<'_> {}
impl DoubleEndedIterator for PixelsIterator<'_> {
    fn next_back(&mut self) -> Option<Self::Item> {
        self.0.next_back().map(|r| FilterRow(r.chunks(self.1)))
    }
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
        let mul = !do_absorb as usize;
        for i in 0..self.ranges.len() {
            let (rmin, rmax, new) = self.ranges[i * mul]; // get the ith element if we aren't absorbing, or 0 if we are
            if new {
                break;
            }
            if rmax < min {
                // this range is entirely before the range we want to absorb
                if do_absorb {
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
        match self.ranges.first() {
            None => EatState::Return,
            Some((_, _, false)) => EatState::QueueFront,
            Some((_, _, true)) => {
                assert!(do_absorb);
                for r in &mut self.ranges {
                    r.2 = false;
                }
                EatState::QueueBack
            }
        }
    }
}

/// Search for the position to insert a blob
///
/// This uses a linear search for small slices and a binary search for bigger ones. The threshold is currently 16 elements
fn search(slice: &[BlobWithBottom], min: u32) -> usize {
    if slice.len() > 16 {
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
            // the draining queue is entirely in the first slice
            let idx = search(&slice[1..], new_min); // skip the first element
            idx > 1 && {
                // if we aren't already sorted, rotate those elements
                // idx returned an inclusive index in the slice without the first element, which is the same as an exclusive index in the slide with the first
                slice[..idx].rotate_left(1);
                true
            }
        } else {
            let idx = search(&front[1..], new_min);
            if idx + 1 < front.len() {
                idx > 1 && {
                    front[..idx].rotate_left(1);
                    true
                }
            } else {
                let slice = &mut back[..(remaining - front.len())];
                let idx = search(slice, new_min);
                if idx == 0 {
                    if front.len() < 2 {
                        return false;
                    }
                    // the element belongs before the start of the second slice, but after all of the elements in the first, i.e. it should be the last in the first slice
                    front.rotate_left(1);
                } else {
                    // move the element from the first slice to the second, rotate the new first element of the front slice, and then rotate the back slice
                    std::mem::swap(&mut slice[0], &mut front[0]);
                    front.rotate_left(1);
                    if idx == slice.len() {
                        slice.rotate_left(1);
                    } else {
                        slice[..=idx].rotate_left(1);
                    }
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
                                                let point = blob.ranges.partition_point(|x| !x.2);
                                                let mut idx = point;
                                                blob.ranges.insert_many(
                                                    point,
                                                    b2.ranges
                                                        .drain_filter(|x| !x.2)
                                                        .inspect(|_| idx += 1),
                                                );
                                                blob.ranges.append(&mut b2.ranges);
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
                                            st => panic!("unexpected blob: {b2:?}, state: {st:?}"),
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
impl<'a> BlobsIterator<PixelsIterator<'a>> {
    /// Create an iterator over the blobs in an image from raw image data, width, and pixel size
    pub fn from_slice(data: &'a [u8], width: usize, pixel_size: usize) -> Self {
        Self::new(PixelsIterator(data.chunks(width * pixel_size), pixel_size))
    }
    /// Create an iterator over the blobs in an image from a buffer
    pub fn from_buffer(buffer: &'a super::Buffer<'_>) -> Self {
        Self::from_slice(
            &buffer.data,
            buffer.width as usize,
            buffer.format.pixel_size(),
        )
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
