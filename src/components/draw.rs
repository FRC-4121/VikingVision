use crate::buffer::{Buffer, PixelFormat};
use crate::pipeline::prelude::*;
use crate::vision::{Blob, Color};
use serde::{Deserialize, Serialize};
use std::cmp::Ordering;
use std::fmt::{self, Display, Formatter};
use std::marker::PhantomData;
use std::ops::{Add, AddAssign, Neg, Sub, SubAssign};
use std::sync::Mutex;

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

/// An iterator that uses Bresenham's line algorithm to draw the pixels in a line
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
            let e2 = self.err.double();
            if e2 >= self.dy {
                match self.x0.partial_cmp(&self.x1) {
                    Some(Ordering::Less) => {
                        self.x0 += T::Signed::ONE;
                    }
                    Some(Ordering::Greater) => {
                        self.x0 -= T::Signed::ONE;
                    }
                    _ => return None,
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
                    _ => return None,
                }
                self.err += self.dy;
            }
            if let (Ok(x), Ok(y)) = (x.try_into(), y.try_into()) {
                return Some((x, y));
            }
        }
    }
}

pub trait Drawable: Data {
    fn draw(&self, color: &[u8], buffer: &mut Buffer);
}
impl Drawable for Blob {
    fn draw(&self, color: &[u8], buffer: &mut Buffer) {
        let data = buffer.data.to_mut();
        if buffer.format == PixelFormat::Yuyv {
            let &[y, u, v] = color else {
                return;
            };
            let yuyv = [y, u, y, v];
            let width = buffer.width as usize;
            let start = self.min_x.min(buffer.width.saturating_sub(1)) as usize;
            let end = self.max_x.min(buffer.width.saturating_sub(1)) as usize;
            let w = 4;
            let fill = self.filled();
            if self.min_y < buffer.width {
                let row_start = width * self.min_y as usize;
                let Some(row) = data.get_mut(((row_start + start) * 2)..((row_start + end) * 2))
                else {
                    return;
                };
                for chunk in row.chunks_exact_mut(4) {
                    chunk.copy_from_slice(&yuyv);
                }
            }
            if self.max_y < buffer.width {
                let row_start = width * self.max_y as usize;
                let Some(row) = data.get_mut(((row_start + start) * w)..((row_start + end) * w))
                else {
                    return;
                };
                for chunk in row.chunks_exact_mut(w) {
                    chunk.copy_from_slice(&yuyv);
                }
            }
            for y in self.min_y..self.max_y {
                let row_start = width * y as usize;
                let start = row_start + start;
                let end = row_start + end;
                let Some(px) = data.get_mut((start * w)..((start + 1) * w)) else {
                    return;
                };
                px.copy_from_slice(color);
                let Some(px) = data.get_mut(((end - 1) * w)..(end * w)) else {
                    return;
                };
                px.copy_from_slice(color);
                if fill > 0.0 {
                    let row = &mut data[((start + 1) * w)..((end - 1) * w)];
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
        } else {
            let width = buffer.width as usize;
            let start = self.min_x.min(buffer.width.saturating_sub(1)) as usize;
            let end = self.max_x.min(buffer.width.saturating_sub(1)) as usize;
            let w = color.len();
            let fill = self.filled();
            if self.min_y < buffer.width {
                let row_start = width * self.min_y as usize;
                let Some(row) = data.get_mut(((row_start + start) * w)..((row_start + end) * w))
                else {
                    return;
                };
                for chunk in row.chunks_exact_mut(w) {
                    chunk.copy_from_slice(color);
                }
            }
            if self.max_y < buffer.width {
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
                let Some(px) = data.get_mut((start * w)..((start + 1) * w)) else {
                    return;
                };
                px.copy_from_slice(color);
                let Some(px) = data.get_mut(((end - 1) * w)..(end * w)) else {
                    return;
                };
                px.copy_from_slice(color);
                if fill > 0.0 {
                    let row = &mut data[((start + 1) * w)..((end - 1) * w)];
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
impl<T: Drawable> Drawable for Vec<T> {
    fn draw(&self, color: &[u8], buffer: &mut Buffer) {
        for elem in self {
            elem.draw(color, buffer);
        }
    }
}

pub struct DrawComponent<T> {
    pub color: Color,
    _marker: PhantomData<fn(T)>,
}
impl<T: Drawable> DrawComponent<T> {
    pub const fn new(color: Color) -> Self {
        Self {
            color,
            _marker: PhantomData,
        }
    }
    pub fn new_boxed(color: Color) -> Box<dyn Component> {
        Box::new(Self::new(color))
    }
}
impl<T: Drawable> Component for DrawComponent<T> {
    fn inputs(&self) -> Inputs {
        Inputs::Named(vec!["canvas".to_string(), "elem".to_string()])
    }
    fn output_kind(&self, name: Option<&str>) -> OutputKind {
        if name == Some("echo") {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'r, '_, 's>) {
        let Ok(canvas) = context.get_as::<Mutex<Buffer>>("canvas").and_log_err() else {
            return;
        };
        let Ok(elem) = context.get_as::<T>("elem").and_log_err() else {
            return;
        };
        {
            let Ok(mut lock) = canvas.lock() else {
                tracing::error!("attempted to lock poisoned mutex");
                return;
            };
            let fmt = self.color.pixel_format();
            lock.convert_inplace(fmt);
            elem.draw(&self.color.bytes(), &mut lock);
        }
        context.submit("echo", canvas);
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(try_from = "DrawShim")]
pub struct DrawFactory {
    /// The type of things to draw.
    ///
    /// Currently supported types are:
    /// - [`Blob`] as `blob`
    /// - [`Line`] as `line`
    /// - [`apriltag::Detection`](crate::apriltag::Detection) as `apriltag`
    /// - a [`Vec`] of any of the previous types, as the previous wrapped in brackets e.g. `[blob]` for `Vec<Blob>`
    pub draw: String,
    /// The color to draw in.
    ///
    /// The image will be converted to the specified colorspace first.
    #[serde(flatten)]
    pub color: Color,
    /// The actual construction function.
    ///
    /// This is skipped in de/serialization, and looked up based on the type name
    #[serde(skip)]
    pub factory: fn(Color) -> Box<dyn Component>,
}
#[typetag::serde(name = "draw")]
impl ComponentFactory for DrawFactory {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
        (self.factory)(self.color)
    }
}

#[derive(Deserialize)]
struct DrawShim {
    draw: String,
    #[serde(flatten)]
    color: Color,
}
impl TryFrom<DrawShim> for DrawFactory {
    type Error = String;

    fn try_from(value: DrawShim) -> Result<Self, Self::Error> {
        let factory = match &*value.draw {
            "blob" => DrawComponent::<Blob>::new_boxed,
            "line" => DrawComponent::<Line>::new_boxed,
            #[cfg(feature = "apriltag")]
            "apriltag" => DrawComponent::<crate::apriltag::Detection>::new_boxed,
            "[blob]" => DrawComponent::<Vec<Blob>>::new_boxed,
            "[line]" => DrawComponent::<Vec<Line>>::new_boxed,
            #[cfg(feature = "apriltag")]
            "[apriltag]" => DrawComponent::<Vec<crate::apriltag::Detection>>::new_boxed,
            name => return Err(format!("Unrecognized type {name:?}")),
        };
        Ok(DrawFactory {
            draw: value.draw,
            color: value.color,
            factory,
        })
    }
}
