//! All of these conversion functions take an input and output array and can be used directly with [`broadcast2`](crate::broadcast::broadcast2) and [`par_broadcast2`](crate::broadcast::par_broadcast2).
//! All functions have the convention of `<input format>::<output format>``, with `i` being used as a function prefix for in-place operations.
//! Note that any YUYV conversions need two pixels to operate on rather than just one.

pub mod gray;
pub mod hsv;
pub mod luma;
pub mod rgb;
pub mod ycc;
pub mod yuyv;

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
pub fn to_inplace<const N: usize>(f: impl Fn(&[u8; N], &mut [u8; N])) -> impl Fn(&mut [u8; N]) {
    move |buf| {
        let from = *buf;
        f(&from, buf);
    }
}
/// Compose two conversions
#[inline(always)]
pub fn compose<const N1: usize, const N2: usize, const N3: usize>(
    f1: impl Fn(&[u8; N1], &mut [u8; N2]) + Send + Sync,
    f2: impl Fn(&[u8; N2], &mut [u8; N3]) + Send + Sync,
) -> impl Fn(&[u8; N1], &mut [u8; N3]) + Send + Sync {
    move |from, to| {
        let mut buf = [0u8; N2];
        f1(from, &mut buf);
        f2(&buf, to);
    }
}
/// Add an alpha of 255 to a conversion that didn't have it
#[inline(always)]
pub fn add_alpha<P: HasAlpha + AsRef<[u8]> + AsMut<[u8]>>(from: &P, to: &mut P::Alpha) {
    let (px, a) = to.split_mut();
    px.as_mut().copy_from_slice(from.as_ref());
    *a = 255;
}
/// Drop the alpha of a color
#[inline(always)]
pub fn drop_alpha<P: AlphaPixel>(from: &P, to: &mut P::Alphaless) {
    to.as_mut().copy_from_slice(from.split().0.as_ref());
}
/// Identity conversion
#[inline(always)]
pub fn iden<const N: usize>(from: &[u8; N], to: &mut [u8; N]) {
    to.copy_from_slice(from);
}
/// Identity conversion
#[inline(always)]
pub fn iden3(from: &[u8; 3], to: &mut [u8; 3]) {
    to.copy_from_slice(from);
}
/// Lift an operation from colors without alpha to one that preserves alpha
#[inline(always)]
pub fn lift_alpha<P1: AlphaPixel, P2: AlphaPixel>(
    op: impl Fn(&P1::Alphaless, &mut P2::Alphaless) + Send + Sync,
) -> impl Fn(&P1, &mut P2) + Send + Sync {
    move |from, to| {
        let (px1, a1) = from.split();
        let (px2, a2) = to.split_mut();
        op(px1, px2);
        *a2 = *a1;
    }
}
/// Double an operation to work on two
#[inline(always)]
pub fn double<P1: DoublePixel, P2: DoublePixel>(
    op: impl Fn(&P1::Single, &mut P2::Single),
) -> impl Fn(&P1, &mut P2) {
    move |from, to| {
        let [f1, f2] = from.split();
        let [t1, t2] = to.split_mut();
        op(f1, t1);
        op(f2, t2);
    }
}

/// Lift an in-place operation from colors without alpha to one that preserves alpha
#[inline(always)]
pub fn ignore_alpha<P: AlphaPixel>(
    op: impl Fn(&mut P::Alphaless) + Send + Sync,
) -> impl Fn(&mut P) + Send + Sync {
    move |buf| {
        op(buf.split_mut().0);
    }
}

pub trait AlphaPixel: AsRef<[u8]> + AsMut<[u8]> {
    type Alphaless: AsRef<[u8]> + AsMut<[u8]>;

    fn split(&self) -> (&Self::Alphaless, &u8);
    fn split_mut(&mut self) -> (&mut Self::Alphaless, &mut u8);
}
impl AlphaPixel for [u8; 2] {
    type Alphaless = [u8; 1];

    fn split(&self) -> (&[u8; 1], &u8) {
        let (head, tail) = self.split_first_chunk().unwrap();
        (head, &tail[0])
    }
    fn split_mut(&mut self) -> (&mut [u8; 1], &mut u8) {
        let (head, tail) = self.split_first_chunk_mut().unwrap();
        (head, &mut tail[0])
    }
}
impl AlphaPixel for [u8; 4] {
    type Alphaless = [u8; 3];

    fn split(&self) -> (&[u8; 3], &u8) {
        let (head, tail) = self.split_first_chunk().unwrap();
        (head, &tail[0])
    }
    fn split_mut(&mut self) -> (&mut [u8; 3], &mut u8) {
        let (head, tail) = self.split_first_chunk_mut().unwrap();
        (head, &mut tail[0])
    }
}
pub trait DoublePixel: AsRef<[u8]> + AsMut<[u8]> {
    type Single: AsRef<[u8]> + AsMut<[u8]>;

    fn split(&self) -> [&Self::Single; 2];
    fn split_mut(&mut self) -> [&mut Self::Single; 2];
}
impl DoublePixel for [u8; 2] {
    type Single = [u8; 1];

    fn split(&self) -> [&Self::Single; 2] {
        let (head, tail) = self.split_at(1);
        [head.try_into().unwrap(), tail.try_into().unwrap()]
    }
    fn split_mut(&mut self) -> [&mut Self::Single; 2] {
        let (head, tail) = self.split_at_mut(1);
        [head.try_into().unwrap(), tail.try_into().unwrap()]
    }
}
impl DoublePixel for [u8; 4] {
    type Single = [u8; 2];

    fn split(&self) -> [&Self::Single; 2] {
        let (head, tail) = self.split_at(2);
        [head.try_into().unwrap(), tail.try_into().unwrap()]
    }
    fn split_mut(&mut self) -> [&mut Self::Single; 2] {
        let (head, tail) = self.split_at_mut(2);
        [head.try_into().unwrap(), tail.try_into().unwrap()]
    }
}
impl DoublePixel for [u8; 6] {
    type Single = [u8; 3];

    fn split(&self) -> [&Self::Single; 2] {
        let (head, tail) = self.split_at(3);
        [head.try_into().unwrap(), tail.try_into().unwrap()]
    }
    fn split_mut(&mut self) -> [&mut Self::Single; 2] {
        let (head, tail) = self.split_at_mut(3);
        [head.try_into().unwrap(), tail.try_into().unwrap()]
    }
}
impl DoublePixel for [u8; 8] {
    type Single = [u8; 4];

    fn split(&self) -> [&Self::Single; 2] {
        let (head, tail) = self.split_at(4);
        [head.try_into().unwrap(), tail.try_into().unwrap()]
    }
    fn split_mut(&mut self) -> [&mut Self::Single; 2] {
        let (head, tail) = self.split_at_mut(4);
        [head.try_into().unwrap(), tail.try_into().unwrap()]
    }
}
pub trait HasAlpha {
    type Alpha: AlphaPixel<Alphaless = Self>;
}
impl HasAlpha for [u8; 1] {
    type Alpha = [u8; 2];
}
impl HasAlpha for [u8; 3] {
    type Alpha = [u8; 4];
}

pub mod lumaa {
    pub fn iyuyv(buf: &mut [u8; 4]) {
        buf[1] = 128;
        buf[3] = 128;
    }
}
