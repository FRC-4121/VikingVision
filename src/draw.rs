use crate::buffer::{Buffer, PixelFormat};
use crate::pipeline::component::Data;
use crate::vision::Blob;
use std::borrow::Cow;
use std::cmp::Ordering;
use std::fmt::{self, Display, Formatter};
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};
use std::sync::Arc;

pub trait SignedCoordinate:
    Copy
    + Add<Output = Self>
    + AddAssign
    + Sub<Output = Self>
    + SubAssign
    + Neg<Output = Self>
    + PartialOrd
{
    const ONE: Self;
    fn abs(self) -> Self;
    fn double(self) -> Self;
    fn to_usize(self) -> usize;
}
pub trait PixelCoordinate: TryFrom<Self::Signed> + TryInto<Self::Signed> {
    type Signed: SignedCoordinate;
}
macro_rules! impl_pixel_coord {
    ($u:ty => $s:ty, $($rest:tt)*) => {
        impl PixelCoordinate for $u {
            type Signed = $s;
        }
        impl PixelCoordinate for $s {
            type Signed = $s;
        }
        impl SignedCoordinate for $s {
            const ONE: Self = 1;
            fn abs(self) -> Self {
                <$s>::abs(self)
            }
            fn double(self) -> Self {
                self << 1
            }
            #[allow(clippy::identity_op)]
            fn to_usize(self) -> usize {
                self as _
            }
        }
        impl_pixel_coord!($($rest)*);
    };
    ($s:ty, $($rest:tt)*) => {
        impl PixelCoordinate for $s {
            type Signed = $s;
        }
        impl SignedCoordinate for $s {
            const ONE: Self = 1.0;
            fn abs(self) -> Self {
                <$s>::abs(self)
            }
            fn double(self) -> Self {
                self * 2.0
            }
            fn to_usize(self) -> usize {
                self.max(0.0).ceil() as _
            }
        }
        impl_pixel_coord!($($rest)*);
    };
    () => {};
}
impl_pixel_coord!(
    u8 => i8,
    u16 => i16,
    u32 => i32,
    u64 => i64,
    usize => isize,
    f32, f64,
);

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Line {
    pub x0: u32,
    pub y0: u32,
    pub x1: u32,
    pub y1: u32,
}
impl Display for Line {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let Line { x0, y0, x1, y1 } = self;
        write!(f, "({x0}, {y0})..({x1}, {y1})")
    }
}
impl Data for Line {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Display::fmt(self, f)
    }
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(*self)
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["x0", "y0", "x1", "y1"]
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        match field {
            "x0" => Some(Cow::Borrowed(&self.x0)),
            "y0" => Some(Cow::Borrowed(&self.y0)),
            "x1" => Some(Cow::Borrowed(&self.x1)),
            "y1" => Some(Cow::Borrowed(&self.y1)),
            _ => None,
        }
    }
}
impl Drawable for Line {
    fn draw(&self, color: &[u8], buffer: &mut Buffer) {
        let Line { x0, y0, x1, y1 } = *self;
        let Ok(it) = DrawLineIterator::new(x0, y0, x1, y1) else {
            return;
        };
        for (x, y) in it {
            buffer.set_pixel(x, y, color);
        }
    }
}

/// An iterator that uses Bresenham's line algorithm to draw the pixels in a line.
///
/// Note that this implementation excludes the last pixel.
#[derive(Debug, Clone, PartialEq)]
pub struct DrawLineIterator<T: PixelCoordinate> {
    x0: T::Signed,
    y0: T::Signed,
    x1: T::Signed,
    y1: T::Signed,
    dx: T::Signed,
    dy: T::Signed,
    err: T::Signed,
}
impl<T: PixelCoordinate> DrawLineIterator<T> {
    pub fn new(x0: T, y0: T, x1: T, y1: T) -> Result<Self, <T as TryInto<T::Signed>>::Error> {
        let x0 = x0.try_into()?;
        let y0 = y0.try_into()?;
        let x1 = x1.try_into()?;
        let y1 = y1.try_into()?;
        let dx = (x1 - x0).abs();
        let dy = -(y1 - y0).abs();
        let err = dx + dy;
        Ok(Self {
            x0,
            y0,
            x1,
            y1,
            dx,
            dy,
            err,
        })
    }
}
impl<T: PixelCoordinate> Iterator for DrawLineIterator<T> {
    type Item = (T, T);

    fn next(&mut self) -> Option<Self::Item> {
        loop {
            let x = self.x0;
            let y = self.y0;
            if x == self.x1 && y == self.y1 {
                return None;
            }
            let e2 = self.err.double();
            if e2 >= self.dy {
                match self.x0.partial_cmp(&self.x1) {
                    Some(Ordering::Less) => {
                        self.x0 += T::Signed::ONE;
                    }
                    Some(Ordering::Greater) => {
                        self.x0 -= T::Signed::ONE;
                    }
                    _ => {}
                }
                self.err += self.dy;
            }
            if e2 <= self.dx {
                match self.y0.partial_cmp(&self.y1) {
                    Some(Ordering::Less) => {
                        self.y0 += T::Signed::ONE;
                    }
                    Some(Ordering::Greater) => {
                        self.y0 -= T::Signed::ONE;
                    }
                    _ => {}
                }
                self.err += self.dx;
            }
            if let (Ok(x), Ok(y)) = (x.try_into(), y.try_into()) {
                return Some((x, y));
            }
        }
    }
    fn size_hint(&self) -> (usize, Option<usize>) {
        let len = self.len();
        (len, Some(len))
    }
}
impl<T: PixelCoordinate> std::iter::FusedIterator for DrawLineIterator<T> {}
impl<T: PixelCoordinate> ExactSizeIterator for DrawLineIterator<T> {
    fn len(&self) -> usize {
        let dx = (self.x1 - self.x0).abs();
        let dy = (self.y1 - self.y0).abs();
        dx.to_usize().max(dy.to_usize())
    }
}

pub trait Drawable: Data {
    fn draw(&self, color: &[u8], buffer: &mut Buffer);
}
impl Drawable for Blob {
    fn draw(&self, color: &[u8], buffer: &mut Buffer) {
        if buffer.format == PixelFormat::YUYV {
            tracing::error!("YUYV blobs aren't supported!");
        } else {
            let data = buffer.data.to_mut();
            let width = buffer.width as usize;
            let start = self.min_x.min(buffer.width.saturating_sub(1)) as usize;
            let end = self.max_x.min(buffer.width.saturating_sub(1)) as usize;
            let w = color.len();
            let fill = self.filled();
            if self.min_y < buffer.height {
                let row_start = width * self.min_y as usize;
                let Some(row) = data.get_mut(((row_start + start) * w)..((row_start + end) * w))
                else {
                    return;
                };
                for chunk in row.chunks_exact_mut(w) {
                    chunk.copy_from_slice(color);
                }
            }
            if self.max_y < buffer.height {
                let row_start = width * self.max_y as usize;
                let Some(row) = data.get_mut(((row_start + start) * w)..((row_start + end) * w))
                else {
                    return;
                };
                for chunk in row.chunks_exact_mut(w) {
                    chunk.copy_from_slice(color);
                }
            }
            for y in self.min_y..self.max_y {
                let row_start = width * y as usize;
                let start = row_start + start;
                let end = row_start + end;
                let start2 = (start + 1) * w;
                let end2 = (end - 1) * w;
                let Some(px) = data.get_mut((start * w)..start2) else {
                    return;
                };
                px.copy_from_slice(color);
                let Some(px) = data.get_mut(end2..(end * w)) else {
                    return;
                };
                px.copy_from_slice(color);
                if start2 < end2 && fill > 0.0 {
                    let row = &mut data[start2..end2];
                    for chunk in row.chunks_exact_mut(w) {
                        if fill == 1.0 {
                            chunk.copy_from_slice(color);
                        } else {
                            for (old, new) in chunk.iter_mut().zip(color) {
                                *old = (*old as f64 * (1.0 - fill) + *new as f64 * fill) as u8;
                            }
                        }
                    }
                }
            }
        }
    }
}
#[cfg(feature = "apriltag")]
impl Drawable for crate::apriltag::Detection {
    fn draw(&self, color: &[u8], buffer: &mut Buffer) {
        let corners = self.corners();
        for i in 0..4 {
            let [x0, y0] = corners[i];
            let [x1, y1] = corners[(i + 1) % 4];
            let Ok(it) = DrawLineIterator::new(x0 as i32, y0 as i32, x1 as i32, y1 as i32);
            for (x, y) in it {
                let (Ok(x), Ok(y)) = (x.try_into(), y.try_into()) else {
                    continue;
                };
                buffer.set_pixel(x, y, color);
            }
        }
    }
}
impl<T: Drawable + Clone> Drawable for Vec<T> {
    fn draw(&self, color: &[u8], buffer: &mut Buffer) {
        for elem in self {
            elem.draw(color, buffer);
        }
    }
}
