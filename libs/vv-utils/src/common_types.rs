#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::fmt::{self, Debug, Display, Formatter};
use std::hash::{BuildHasher, BuildHasherDefault, DefaultHasher, Hash};
#[cfg(feature = "supply")]
use supply::prelude::*;

/// A comparable ID for pipeline runs.
///
/// This can be used to help components hold state between runs.
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct PipelineId(pub u64);
impl PipelineId {
    /// Create a pipeline ID from a hashable value.
    pub fn from_hash(val: impl Hash) -> Self {
        Self(BuildHasherDefault::<DefaultHasher>::new().hash_one(val))
    }
    /// Create a pipeline ID form a pointer.
    ///
    /// This gives a different value from [`from_hash`](Self::from_hash) being used with a pointer argument.
    pub fn from_ptr(val: *const impl ?Sized) -> Self {
        Self(val as *const () as usize as u64)
    }
}
impl Display for PipelineId {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{:0>16x}", self.0)
    }
}

/// A pretty name for a pipeline run.
#[derive(Clone, Copy)]
pub struct PipelineName<'a>(pub &'a dyn Display);
impl Debug for PipelineName<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        struct AsDebug<'a>(&'a dyn Display);
        impl Debug for AsDebug<'_> {
            fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
                Display::fmt(self.0, f)
            }
        }
        f.debug_tuple("PipelineName")
            .field(&AsDebug(self.0))
            .finish()
    }
}
impl Display for PipelineName<'_> {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Display::fmt(self.0, f)
    }
}

/// A [`Provider`] implementation that provides a [`PipelineName`] and [`PipelineId`] for requests through [`supply`].
pub struct PipelineProvider<T> {
    pub id: PipelineId,
    pub name: T,
}
impl<T> PipelineProvider<T> {
    pub fn from_ptr(ptr: *const impl ?Sized, name: T) -> Self {
        Self {
            id: PipelineId::from_ptr(ptr),
            name,
        }
    }
    pub fn from_hash(val: impl Hash, name: T) -> Self {
        Self {
            id: PipelineId::from_hash(val),
            name,
        }
    }
    pub const fn from_raw(id: u64, name: T) -> Self {
        Self {
            id: PipelineId(id),
            name,
        }
    }
}
#[cfg(feature = "supply")]
impl<'r, T: Display> Provider<'r> for PipelineProvider<T> {
    type Lifetimes = l!['r];

    fn provide(&'r self, want: &mut dyn supply::Want<Self::Lifetimes>) {
        want.provide_value(PipelineName(&self.name))
            .provide_value(self.id);
    }
}

/// Type tag for [`PipelineId`].
#[cfg_attr(feature = "supply", ty_tag::tag)]
pub type PipelineIdTag = PipelineId;

/// Type tag for [`PipelineName`].
#[cfg_attr(feature = "supply", ty_tag::tag)]
pub type PipelineNameTag<'a> = PipelineName<'a>;

/// Typed identifier for the field of view for a camera.
///
/// This is given in degrees.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(transparent))]
pub struct Fov(pub f64);

/// Typed identifier for the expected size from a camera.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct FrameSize {
    pub width: u32,
    pub height: u32,
}

#[cfg(feature = "supply")]
#[ty_tag::tag]
pub type FovTag = Fov;

#[cfg(feature = "supply")]
#[ty_tag::tag]
pub type FrameSizeTag = FrameSize;
