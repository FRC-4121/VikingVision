//! All of these conversion functions take an input and output array and can be used directly with [`broadcast2`](crate::broadcast::broadcast2) and [`par_broadcast2`](crate::broadcast::par_broadcast2).
//! All functions have the convention of `<input format>::<output format>``, with `i` being used as a function prefix for in-place operations.
//! Note that any YUYV conversions need two pixels to operate on rather than just one.

pub mod gray;
pub mod hsv;
pub mod luma;
pub mod rgb;
pub mod ycc;
pub mod yuyv;

#[inline(always)]
fn first<const N: usize, T>(buf: &[T]) -> &[T; N] {
    buf[..N].try_into().unwrap()
}
#[inline(always)]
fn first_mut<const N: usize, T>(buf: &mut [T]) -> &mut [T; N] {
    (&mut buf[..N]).try_into().unwrap()
}

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
pub fn add_alpha<const N: usize>(from: &[u8; N], to: &mut [u8; N + 1])
where
    [(); N + 1]: Sized,
{
    to[..N].copy_from_slice(from);
    to[N] = 255;
}
/// Drop the alpha of a color
#[inline(always)]
pub fn drop_alpha<const N: usize>(from: &[u8; N + 1], to: &mut [u8; N])
where
    [(); N]: Sized,
{
    to.copy_from_slice(&from[..3]);
}
/// Lift an operation from colors without alpha to one that preserves alpha
#[inline(always)]
pub fn lift_alpha<const N1: usize, const N2: usize>(
    op: impl Fn(&[u8; N1], &mut [u8; N2]) + Send + Sync,
) -> impl Fn(&[u8; N1 + 1], &mut [u8; N2 + 1]) + Send + Sync
where
    [(); N1 + 1]: Sized,
    [(); N2 + 1]: Sized,
{
    move |from, to| {
        op(first(from), first_mut(to));
        to[N2] = from[N1];
    }
}
/// Double an operation to work on two
#[inline(always)]
pub fn double<const N1: usize, const N2: usize>(
    op: impl Fn(&[u8; N1], &mut [u8; N2]),
) -> impl Fn(&[u8; N1 * 2], &mut [u8; N2 * 2])
where
    [(); N1 * 2]: Sized,
    [(); N2 * 2]: Sized,
{
    move |from, to| {
        op(
            from[..N1].try_into().unwrap(),
            (&mut to[..N2]).try_into().unwrap(),
        );
        op(
            from[N1..].try_into().unwrap(),
            (&mut to[N2..]).try_into().unwrap(),
        );
    }
}

/// Lift an in-place operation from colors without alpha to one that preserves alpha
#[inline(always)]
pub fn ignore_alpha<const N: usize>(
    op: impl Fn(&mut [u8; N + 1]) + Send + Sync,
) -> impl Fn(&mut [u8; N]) + Send + Sync
where
    [(); N + 1]: Sized,
{
    move |buf| {
        op(first_mut(buf));
    }
}

pub mod lumaa {
    pub fn iyuyv(buf: &mut [u8; 4]) {
        buf[1] = 128;
        buf[3] = 128;
    }
}
