//! Chunking broadcast operations
//!
//! Operations across all pixels in a buffer are very common, and this module defines a generic way to get chunks of data, along with helper funtions to apply a function across chunks.

use crate::buffer::Buffer;
use rayon::prelude::*;

/// A collection that can have chunks taken. This is done as a trait to generically allow broadcasts across both shared and mutable slices.
pub trait Chunks {
    /// A chunk of length N. Expected to be `&[T; N]` or similar.
    type Chunk<const N: usize>;

    /// An iterator over the chunks. Should have behavior similar to [`[T]::array_chunks`], although this implementation doesn't use that function because it's only available on nightly.
    fn chunks<const N: usize>(self) -> impl Iterator<Item = Self::Chunk<N>>;
}
/// An extension to [`Chunks`] that works with rayon, allowing parallel broadcasting.
pub trait ParChunks: Chunks {
    fn par_chunks<const N: usize>(self) -> impl IndexedParallelIterator<Item = Self::Chunk<N>>;
}
impl<'a, T> Chunks for &'a [T] {
    type Chunk<const N: usize> = &'a [T; N];

    fn chunks<const N: usize>(self) -> impl Iterator<Item = Self::Chunk<N>> {
        self.chunks_exact(N).map(|c| c.try_into().unwrap())
    }
}
impl<T: Sync> ParChunks for &[T] {
    fn par_chunks<const N: usize>(self) -> impl IndexedParallelIterator<Item = Self::Chunk<N>> {
        self.par_chunks_exact(N).map(|c| c.try_into().unwrap())
    }
}
impl<'a, T> Chunks for &'a mut [T] {
    type Chunk<const N: usize> = &'a mut [T; N];

    fn chunks<const N: usize>(self) -> impl Iterator<Item = Self::Chunk<N>> {
        self.chunks_exact_mut(N).map(|c| c.try_into().unwrap())
    }
}
impl<T: Send + Sync> ParChunks for &mut [T] {
    fn par_chunks<const N: usize>(self) -> impl IndexedParallelIterator<Item = Self::Chunk<N>> {
        self.par_chunks_exact_mut(N).map(|c| c.try_into().unwrap())
    }
}
impl<'a> Chunks for &'a Buffer<'_> {
    type Chunk<const N: usize> = &'a [u8; N];

    fn chunks<const N: usize>(self) -> impl Iterator<Item = Self::Chunk<N>> {
        self.data.chunks_exact(N).map(|c| c.try_into().unwrap())
    }
}
impl<'a, const M: usize, T> Chunks for &'a [T; M] {
    type Chunk<const N: usize> = &'a [T; N];

    fn chunks<const N: usize>(self) -> impl Iterator<Item = Self::Chunk<N>> {
        self.chunks_exact(N).map(|c| c.try_into().unwrap())
    }
}
impl<const M: usize, T: Sync> ParChunks for &[T; M] {
    fn par_chunks<const N: usize>(self) -> impl IndexedParallelIterator<Item = Self::Chunk<N>> {
        self.par_chunks_exact(N).map(|c| c.try_into().unwrap())
    }
}
impl<'a, const M: usize, T> Chunks for &'a mut [T; M] {
    type Chunk<const N: usize> = &'a mut [T; N];

    fn chunks<const N: usize>(self) -> impl Iterator<Item = Self::Chunk<N>> {
        self.chunks_exact_mut(N).map(|c| c.try_into().unwrap())
    }
}
impl<const M: usize, T: Send + Sync> ParChunks for &mut [T; M] {
    fn par_chunks<const N: usize>(self) -> impl IndexedParallelIterator<Item = Self::Chunk<N>> {
        self.par_chunks_exact_mut(N).map(|c| c.try_into().unwrap())
    }
}
impl ParChunks for &Buffer<'_> {
    fn par_chunks<const N: usize>(self) -> impl IndexedParallelIterator<Item = Self::Chunk<N>> {
        self.data.par_chunks_exact(N).map(|c| c.try_into().unwrap())
    }
}
impl<'a> Chunks for &'a mut Buffer<'_> {
    type Chunk<const N: usize> = &'a mut [u8; N];

    fn chunks<const N: usize>(self) -> impl Iterator<Item = Self::Chunk<N>> {
        self.data
            .to_mut()
            .chunks_exact_mut(N)
            .map(|c| c.try_into().unwrap())
    }
}
impl ParChunks for &mut Buffer<'_> {
    fn par_chunks<const N: usize>(self) -> impl IndexedParallelIterator<Item = Self::Chunk<N>> {
        self.data
            .to_mut()
            .par_chunks_exact_mut(N)
            .map(|c| c.try_into().unwrap())
    }
}

pub fn broadcast1<const N1: usize, A1: Chunks>(f: impl FnMut(A1::Chunk<N1>), arr1: A1) {
    arr1.chunks().for_each(f);
}
pub fn broadcast2<const N1: usize, const N2: usize, A1: Chunks, A2: Chunks>(
    mut f: impl FnMut(A1::Chunk<N1>, A2::Chunk<N2>),
    arr1: A1,
    arr2: A2,
) {
    arr1.chunks()
        .zip(arr2.chunks())
        .for_each(|(c1, c2)| f(c1, c2));
}
pub fn broadcast3<
    const N1: usize,
    const N2: usize,
    const N3: usize,
    A1: Chunks,
    A2: Chunks,
    A3: Chunks,
>(
    mut f: impl FnMut(A1::Chunk<N1>, A2::Chunk<N2>, A3::Chunk<N3>),
    arr1: A1,
    arr2: A2,
    arr3: A3,
) {
    arr1.chunks()
        .zip(arr2.chunks())
        .zip(arr3.chunks())
        .for_each(|((c1, c2), c3)| f(c1, c2, c3));
}
pub fn par_broadcast1<const N1: usize, A1: ParChunks>(
    f: impl Fn(A1::Chunk<N1>) + Send + Sync,
    arr1: A1,
) {
    arr1.par_chunks().for_each(f);
}
pub fn par_broadcast2<const N1: usize, const N2: usize, A1: ParChunks, A2: ParChunks>(
    f: impl Fn(A1::Chunk<N1>, A2::Chunk<N2>) + Send + Sync,
    arr1: A1,
    arr2: A2,
) {
    arr1.par_chunks()
        .zip(arr2.par_chunks())
        .for_each(|(c1, c2)| f(c1, c2));
}
pub fn par_broadcast3<
    const N1: usize,
    const N2: usize,
    const N3: usize,
    A1: ParChunks,
    A2: ParChunks,
    A3: ParChunks,
>(
    f: impl Fn(A1::Chunk<N1>, A2::Chunk<N2>, A3::Chunk<N3>) + Send + Sync,
    arr1: A1,
    arr2: A2,
    arr3: A3,
) {
    arr1.par_chunks()
        .zip(arr2.par_chunks())
        .zip(arr3.par_chunks())
        .for_each(|((c1, c2), c3)| f(c1, c2, c3));
}
