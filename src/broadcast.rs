//! Chunking broadcast operations
//!
//! Operations across all pixels in a buffer are very common, and this module defines a generic way to get chunks of data, along with helper funtions to apply a function across chunks.

use crate::buffer::Buffer;
use rayon::prelude::*;
use std::sync::OnceLock;

/// A collection that can have chunks taken. This is done as a trait to generically allow broadcasts across both shared and mutable slices.
pub trait Chunks {
    type Chunk;
    /// An iterator over the chunks. This is implemented through [`[T]::chunks_exact`] or [`[T]::chunks_exact_mut`].
    fn chunks(self, n: usize) -> impl Iterator<Item = Self::Chunk>;
}
/// An extension to [`Chunks`] that works with rayon, allowing parallel broadcasting.
pub trait ParChunks: Chunks + Send {
    fn par_chunks(self, n: usize) -> impl IndexedParallelIterator<Item = Self::Chunk>;
}

impl<'a, T> Chunks for &'a [T] {
    type Chunk = &'a [T];

    fn chunks(self, n: usize) -> impl Iterator<Item = Self::Chunk> {
        self.chunks_exact(n)
    }
}
impl<T: Sync> ParChunks for &[T] {
    fn par_chunks(self, n: usize) -> impl IndexedParallelIterator<Item = Self::Chunk> {
        self.par_chunks_exact(n)
    }
}
impl<'a, T> Chunks for &'a mut [T] {
    type Chunk = &'a mut [T];

    fn chunks(self, n: usize) -> impl Iterator<Item = Self::Chunk> {
        self.chunks_exact_mut(n)
    }
}
impl<T: Send + Sync> ParChunks for &mut [T] {
    fn par_chunks(self, n: usize) -> impl IndexedParallelIterator<Item = Self::Chunk> {
        self.par_chunks_exact_mut(n)
    }
}
impl<'a, const N: usize, T> Chunks for &'a [T; N] {
    type Chunk = &'a [T];

    fn chunks(self, n: usize) -> impl Iterator<Item = Self::Chunk> {
        self.chunks_exact(n)
    }
}
impl<const N: usize, T: Sync> ParChunks for &[T; N] {
    fn par_chunks(self, n: usize) -> impl IndexedParallelIterator<Item = Self::Chunk> {
        self.par_chunks_exact(n)
    }
}
impl<'a, const N: usize, T> Chunks for &'a mut [T; N] {
    type Chunk = &'a mut [T];

    fn chunks(self, n: usize) -> impl Iterator<Item = Self::Chunk> {
        self.chunks_exact_mut(n)
    }
}
impl<const N: usize, T: Send + Sync> ParChunks for &mut [T; N] {
    fn par_chunks(self, n: usize) -> impl IndexedParallelIterator<Item = Self::Chunk> {
        self.par_chunks_exact_mut(n)
    }
}
impl<'a, T> Chunks for &'a Vec<T> {
    type Chunk = &'a [T];

    fn chunks(self, n: usize) -> impl Iterator<Item = Self::Chunk> {
        self.chunks_exact(n)
    }
}
impl<T: Sync> ParChunks for &Vec<T> {
    fn par_chunks(self, n: usize) -> impl IndexedParallelIterator<Item = Self::Chunk> {
        self.par_chunks_exact(n)
    }
}
impl<'a, T> Chunks for &'a mut Vec<T> {
    type Chunk = &'a mut [T];

    fn chunks(self, n: usize) -> impl Iterator<Item = Self::Chunk> {
        self.chunks_exact_mut(n)
    }
}
impl<T: Send + Sync> ParChunks for &mut Vec<T> {
    fn par_chunks(self, n: usize) -> impl IndexedParallelIterator<Item = Self::Chunk> {
        self.par_chunks_exact_mut(n)
    }
}
impl<'a> Chunks for &'a Buffer<'_> {
    type Chunk = &'a [u8];

    fn chunks(self, n: usize) -> impl Iterator<Item = Self::Chunk> {
        self.data.chunks_exact(n)
    }
}
impl ParChunks for &Buffer<'_> {
    fn par_chunks(self, n: usize) -> impl IndexedParallelIterator<Item = Self::Chunk> {
        self.data.par_chunks_exact(n)
    }
}
impl<'a> Chunks for &'a mut Buffer<'_> {
    type Chunk = &'a mut [u8];

    fn chunks(self, n: usize) -> impl Iterator<Item = Self::Chunk> {
        self.data.to_mut().chunks_exact_mut(n)
    }
}
impl ParChunks for &mut Buffer<'_> {
    fn par_chunks(self, n: usize) -> impl IndexedParallelIterator<Item = Self::Chunk> {
        self.data.to_mut().par_chunks_exact_mut(n)
    }
}

pub trait ArrayRef {
    type Slice;
    const SIZE: usize;
    fn convert(slice: Self::Slice) -> Self;
}
impl<'a, T, const N: usize> ArrayRef for &'a [T; N] {
    type Slice = &'a [T];
    const SIZE: usize = N;
    fn convert(slice: Self::Slice) -> Self {
        slice.try_into().unwrap()
    }
}
impl<'a, T, const N: usize> ArrayRef for &'a mut [T; N] {
    type Slice = &'a mut [T];
    const SIZE: usize = N;
    fn convert(slice: Self::Slice) -> Self {
        slice.try_into().unwrap()
    }
}

pub trait Broadcast1<C1, Marker> {
    fn sizes(&self) -> [usize; 1];
    fn run(&mut self, a1: C1);
}
pub trait ParBroadcast1<C1, Marker>: Broadcast1<C1, Marker> + Send + Sync {
    fn par_run(&self, a1: C1);
}
pub trait Broadcast2<C1, C2, Marker> {
    fn sizes(&self) -> [usize; 2];
    fn run(&mut self, a1: C1, a2: C2);
}
pub trait ParBroadcast2<C1, C2, Marker>: Broadcast2<C1, C2, Marker> + Send + Sync {
    fn par_run(&self, a1: C1, a2: C2);
}
pub trait Broadcast3<C1, C2, C3, Marker> {
    fn sizes(&self) -> [usize; 3];
    fn run(&mut self, a1: C1, a2: C2, a3: C3);
}
pub trait ParBroadcast3<C1, C2, C3, Marker>: Broadcast3<C1, C2, C3, Marker> + Send + Sync {
    fn par_run(&self, a1: C1, a2: C2, a3: C3);
}
impl<A1: ArrayRef, F: FnMut(A1)> Broadcast1<A1::Slice, (A1,)> for F {
    fn sizes(&self) -> [usize; 1] {
        [A1::SIZE]
    }
    fn run(&mut self, a1: A1::Slice) {
        self(A1::convert(a1));
    }
}
impl<A1: ArrayRef, F: Fn(A1) + Send + Sync> ParBroadcast1<A1::Slice, (A1,)> for F {
    fn par_run(&self, a1: A1::Slice) {
        self(A1::convert(a1));
    }
}
impl<A1: ArrayRef, A2: ArrayRef, F: FnMut(A1, A2)> Broadcast2<A1::Slice, A2::Slice, (A1, A2)>
    for F
{
    fn sizes(&self) -> [usize; 2] {
        [A1::SIZE, A2::SIZE]
    }
    fn run(&mut self, a1: A1::Slice, a2: A2::Slice) {
        self(A1::convert(a1), A2::convert(a2));
    }
}
impl<A1: ArrayRef, A2: ArrayRef, F: Fn(A1, A2) + Send + Sync>
    ParBroadcast2<A1::Slice, A2::Slice, (A1, A2)> for F
{
    fn par_run(&self, a1: A1::Slice, a2: A2::Slice) {
        self(A1::convert(a1), A2::convert(a2));
    }
}
impl<A1: ArrayRef, A2: ArrayRef, A3: ArrayRef, F: FnMut(A1, A2, A3)>
    Broadcast3<A1::Slice, A2::Slice, A3::Slice, (A1, A2, A3)> for F
{
    fn sizes(&self) -> [usize; 3] {
        [A1::SIZE, A2::SIZE, A3::SIZE]
    }
    fn run(&mut self, a1: A1::Slice, a2: A2::Slice, a3: A3::Slice) {
        self(A1::convert(a1), A2::convert(a2), A3::convert(a3));
    }
}
impl<A1: ArrayRef, A2: ArrayRef, A3: ArrayRef, F: Fn(A1, A2, A3) + Send + Sync>
    ParBroadcast3<A1::Slice, A2::Slice, A3::Slice, (A1, A2, A3)> for F
{
    fn par_run(&self, a1: A1::Slice, a2: A2::Slice, a3: A3::Slice) {
        self(A1::convert(a1), A2::convert(a2), A3::convert(a3));
    }
}

pub fn broadcast1<Marker, A1: Chunks, B: Broadcast1<A1::Chunk, Marker>>(mut f: B, arr1: A1) {
    let [s1] = f.sizes();
    arr1.chunks(s1).for_each(|c1| f.run(c1));
}
pub fn broadcast2<Marker, A1: Chunks, A2: Chunks, B: Broadcast2<A1::Chunk, A2::Chunk, Marker>>(
    mut f: B,
    arr1: A1,
    arr2: A2,
) {
    let [s1, s2] = f.sizes();
    arr1.chunks(s1)
        .zip(arr2.chunks(s2))
        .for_each(|(c1, c2)| f.run(c1, c2));
}
pub fn broadcast3<
    Marker,
    A1: Chunks,
    A2: Chunks,
    A3: Chunks,
    B: Broadcast3<A1::Chunk, A2::Chunk, A3::Chunk, Marker>,
>(
    mut f: B,
    arr1: A1,
    arr2: A2,
    arr3: A3,
) {
    let [s1, s2, s3] = f.sizes();
    arr1.chunks(s1)
        .zip(arr2.chunks(s2))
        .zip(arr3.chunks(s3))
        .for_each(|((c1, c2), c3)| f.run(c1, c2, c3));
}
pub fn par_broadcast1<Marker, A1: ParChunks, B: ParBroadcast1<A1::Chunk, Marker>>(f: B, arr1: A1) {
    broadcast_pool().install(|| {
        let [s1] = f.sizes();
        let it = arr1.par_chunks(s1);
        // broadcast_pool().install(|| {
        it.for_each(|c1| f.par_run(c1));
    });
}
pub fn par_broadcast2<
    Marker,
    A1: ParChunks,
    A2: ParChunks,
    B: ParBroadcast2<A1::Chunk, A2::Chunk, Marker>,
>(
    f: B,
    arr1: A1,
    arr2: A2,
) {
    broadcast_pool().install(|| {
        let [s1, s2] = f.sizes();
        let it = arr1.par_chunks(s1).zip(arr2.par_chunks(s2));
        // broadcast_pool().install(|| {
        it.for_each(|(c1, c2)| f.par_run(c1, c2));
    });
}
pub fn par_broadcast3<
    Marker,
    A1: ParChunks,
    A2: ParChunks,
    A3: ParChunks,
    B: ParBroadcast3<A1::Chunk, A2::Chunk, A3::Chunk, Marker>,
>(
    f: B,
    arr1: A1,
    arr2: A2,
    arr3: A3,
) {
    broadcast_pool().install(|| {
        let [s1, s2, s3] = f.sizes();
        let it = arr1
            .par_chunks(s1)
            .zip(arr2.par_chunks(s2))
            .zip(arr3.par_chunks(s3));
        // broadcast_pool().install(|| {
        it.for_each(|((c1, c2), c3)| f.par_run(c1, c2, c3));
    });
}

pub static BROADCAST_POOL: OnceLock<rayon::ThreadPool> = OnceLock::new();

pub fn broadcast_pool() -> &'static rayon::ThreadPool {
    BROADCAST_POOL.get_or_init(default_broadcast_pool)
}

fn default_broadcast_pool() -> rayon::ThreadPool {
    rayon::ThreadPoolBuilder::new()
        .thread_name(|idx| format!("broadcast-{idx}"))
        .build()
        .expect("Failed to build thread pool!")
}
