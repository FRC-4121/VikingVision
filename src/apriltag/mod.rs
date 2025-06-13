#![cfg(feature = "apriltag")]

use crate::buffer::{Buffer, PixelFormat};
use crate::pipeline::prelude::Data;
use apriltag_sys::*;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Debug, Display, Formatter};
use std::mem::MaybeUninit;
use std::str::FromStr;
use thiserror::Error;

#[cfg(test)]
mod tests;

/// A pose calculated by the apriltag pose estimator.
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Pose {
    /// The translation vector.
    pub translation: [f64; 3],
    /// The rotation matrix stored in a row-major layout.
    pub rotation: [f64; 9],
}
impl Data for Pose {}
impl Default for Pose {
    fn default() -> Self {
        Self {
            translation: [0.0; 3],
            rotation: [1.0, 0.0, 0.0, 0.0, 1.0, 0.0, 0.0, 0.0, 1.0],
        }
    }
}

/// A pose calculated by the apriltag pose estimator, along with an error estimate
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct PoseEstimation {
    pub pose: Pose,
    pub error: f64,
}
impl Data for PoseEstimation {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("PoseEstimation")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, PartialEq, Error)]
#[error("Unknown tag family {0}")]
pub struct UnknownFamily(pub String);

/// A family of apriltags.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
#[allow(non_camel_case_types)]
pub enum TagFamily {
    tag16h5,
    tag25h9,
    tag36h11,
    tagCircle21h7,
    tagCircle49h12,
    tagCustom48h12,
    tagStandard41h12,
    tagStandard52h13,
}
impl Display for TagFamily {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        Debug::fmt(self, f)
    }
}
impl FromStr for TagFamily {
    type Err = UnknownFamily;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "tag16h5" => Ok(Self::tag16h5),
            "tag25h9" => Ok(Self::tag25h9),
            "tag36h11" => Ok(Self::tag36h11),
            "tagCircle21h7" => Ok(Self::tagCircle21h7),
            "tagCircle49h12" => Ok(Self::tagCircle49h12),
            "tagCustom48h12" => Ok(Self::tagCustom48h12),
            "tagStandard41h12" => Ok(Self::tagStandard41h12),
            "tagStandard52h13" => Ok(Self::tagStandard52h13),
            _ => Err(UnknownFamily(s.to_string())),
        }
    }
}
impl TagFamily {
    pub const fn create(self) -> unsafe extern "C" fn() -> *mut apriltag_family_t {
        use TagFamily::*;
        match self {
            tag16h5 => tag16h5_create,
            tag25h9 => tag25h9_create,
            tag36h11 => tag36h11_create,
            tagCircle21h7 => tagCircle21h7_create,
            tagCircle49h12 => tagCircle49h12_create,
            tagCustom48h12 => tagCustom48h12_create,
            tagStandard41h12 => tagStandard41h12_create,
            tagStandard52h13 => tagStandard52h13_create,
        }
    }
    pub const fn destroy(self) -> unsafe extern "C" fn(*mut apriltag_family_t) {
        use TagFamily::*;
        match self {
            tag16h5 => tag16h5_destroy,
            tag25h9 => tag25h9_destroy,
            tag36h11 => tag36h11_destroy,
            tagCircle21h7 => tagCircle21h7_destroy,
            tagCircle49h12 => tagCircle49h12_destroy,
            tagCustom48h12 => tagCustom48h12_destroy,
            tagStandard41h12 => tagStandard41h12_destroy,
            tagStandard52h13 => tagStandard52h13_destroy,
        }
    }
}

#[derive(Debug, Clone, Error)]
pub enum ParseFamilyError {
    #[error("Empty family name")]
    EmptyFamily,
    #[error("Empty bit count")]
    EmptyBits,
    #[error(transparent)]
    InvalidFamily(#[from] UnknownFamily),
    #[error(transparent)]
    InvalidBits(#[from] std::num::ParseIntError),
}

#[derive(Debug, Clone, Copy, PartialEq)]
pub struct TagFamilyWithBits {
    pub family: TagFamily,
    pub bits: u8,
}
impl Display for TagFamilyWithBits {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        write!(f, "{}?{}", self.family, self.bits)
    }
}
impl From<TagFamily> for TagFamilyWithBits {
    fn from(value: TagFamily) -> Self {
        Self {
            family: value,
            bits: 2,
        }
    }
}
impl From<(TagFamily, u8)> for TagFamilyWithBits {
    fn from(value: (TagFamily, u8)) -> Self {
        Self {
            family: value.0,
            bits: value.1,
        }
    }
}
impl FromStr for TagFamilyWithBits {
    type Err = ParseFamilyError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some(idx) = s.find('?') {
            let (family, tail) = s.split_at(idx);
            let bits = &tail[1..];
            if family.is_empty() {
                return Err(ParseFamilyError::EmptyFamily);
            }
            if bits.is_empty() {
                return Err(ParseFamilyError::EmptyBits);
            }
            let family = family.parse()?;
            let bits = bits.parse()?;
            Ok(Self { family, bits })
        } else {
            if s.is_empty() {
                return Err(ParseFamilyError::EmptyFamily);
            }
            let family = s.parse()?;
            Ok(Self { family, bits: 2 })
        }
    }
}
impl Serialize for TagFamilyWithBits {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_str(&format!("{}?{}", self.family, self.bits))
    }
}

mod de_tfwb {
    use super::*;

    #[derive(Deserialize)]
    struct StructShim {
        family: TagFamily,
        bits: u8,
    }
    #[derive(Deserialize)]
    #[serde(try_from = "String")]
    struct StringShim(TagFamilyWithBits);
    impl TryFrom<String> for StringShim {
        type Error = ParseFamilyError;

        fn try_from(value: String) -> Result<Self, Self::Error> {
            value.parse().map(StringShim)
        }
    }
    #[derive(Deserialize)]
    #[serde(untagged)]
    enum DeShim {
        Struct(StructShim),
        String(StringShim),
    }
    impl<'de> Deserialize<'de> for TagFamilyWithBits {
        fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
        where
            D: serde::Deserializer<'de>,
        {
            let shim = DeShim::deserialize(deserializer)?;
            match shim {
                DeShim::Struct(StructShim { family, bits }) => {
                    Ok(TagFamilyWithBits { family, bits })
                }
                DeShim::String(StringShim(s)) => Ok(s),
            }
        }
    }
}

mod family_list {
    use super::*;
    use serde::{Deserializer, Serializer};

    #[derive(Serialize)]
    enum SerFamilyList<'a> {
        Family(TagFamilyWithBits),
        Families(&'a Vec<TagFamilyWithBits>),
    }
    #[derive(Deserialize)]
    enum DeFamilyList {
        Family(TagFamilyWithBits),
        Families(Vec<TagFamilyWithBits>),
    }

    pub fn serialize<S: Serializer>(
        l: &Vec<TagFamilyWithBits>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        if l.len() == 1 {
            SerFamilyList::Family(l[0])
        } else {
            SerFamilyList::Families(l)
        }
        .serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Vec<TagFamilyWithBits>, D::Error> {
        match DeFamilyList::deserialize(deserializer)? {
            DeFamilyList::Family(f) => Ok(vec![f]),
            DeFamilyList::Families(fs) => Ok(fs),
        }
    }
}

/// Serializable detector configuration.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct DetectorConfig {
    #[serde(
        default,
        flatten,
        skip_serializing_if = "Vec::is_empty",
        with = "family_list"
    )]
    pub families: Vec<TagFamilyWithBits>,
    pub max_threads: Option<u8>,
    pub sigma: Option<f32>,
    pub decimate: Option<f32>,
}

#[derive(Debug)]
pub struct Detector {
    ptr: *mut apriltag_detector_t,
}
unsafe impl Send for Detector {}
unsafe impl Sync for Detector {}
impl Drop for Detector {
    fn drop(&mut self) {
        unsafe {
            if !self.ptr.is_null() {
                apriltag_detector_destroy(self.ptr);
            }
        }
    }
}
impl Default for Detector {
    fn default() -> Self {
        Self::new()
    }
}
impl Detector {
    /// Create a new `Detector` by taking ownership of a pointer.
    ///
    /// # Safety
    /// This pointer must be to a valid detector.
    pub const unsafe fn from_raw(ptr: *mut apriltag_detector_t) -> Self {
        Self { ptr }
    }
    /// Convert this detector back into its inner detector pointer.
    pub fn into_raw(mut self) -> *mut apriltag_detector_t {
        std::mem::replace(&mut self.ptr, std::ptr::null_mut())
    }
    /// Create a new [`Detector`].
    ///
    /// Families must be added with [`add_family`](Self::add_family) before this can detect any tags.
    pub fn new() -> Self {
        unsafe { Self::from_raw(apriltag_detector_create()) }
    }
    /// Create a new [`Detector`] from a serializable configuration.
    pub fn from_config(cfg: &DetectorConfig) -> Self {
        let mut this = Self::new();
        this.apply_config(cfg);
        this
    }
    /// Apply a serializable configuration to this detector.
    pub fn apply_config(&mut self, cfg: &DetectorConfig) -> &mut Self {
        self.clear_families();
        for &family in &cfg.families {
            self.add_family(family);
        }
        if let Some(threads) = cfg.max_threads {
            self.set_max_threads(threads);
        }
        if let Some(sigma) = cfg.sigma {
            self.set_sigma(sigma);
        }
        if let Some(decimate) = cfg.decimate {
            self.set_decimate(decimate);
        }
        self
    }
    /// Add a family to be detected.
    ///
    /// It's recommended to set `bits` to 2 as a default.
    pub fn add_family(&mut self, family: impl Into<TagFamilyWithBits>) -> &mut Self {
        let TagFamilyWithBits { family, bits } = family.into();
        unsafe {
            let family = family.create()();
            apriltag_detector_add_family_bits(self.ptr, family, bits as _);
        }
        self
    }
    /// Remove a family from the detector.
    pub fn remove_family(&mut self, family: TagFamily) -> &mut Self {
        unsafe {
            let fam = family.create()();
            apriltag_detector_remove_family(self.ptr, fam);
            family.destroy()(fam);
        }
        self
    }
    /// Clear all families from the detector.
    pub fn clear_families(&mut self) -> &mut Self {
        unsafe {
            apriltag_detector_clear_families(self.ptr);
        }
        self
    }
    pub fn set_max_threads(&mut self, threads: u8) -> &mut Self {
        unsafe {
            (*self.ptr).nthreads = threads as _;
        }
        self
    }
    pub fn set_sigma(&mut self, sigma: f32) -> &mut Self {
        unsafe {
            (*self.ptr).quad_sigma = sigma;
        }
        self
    }
    pub fn set_decimate(&mut self, decimate: f32) -> &mut Self {
        unsafe {
            (*self.ptr).quad_decimate = decimate;
        }
        self
    }
    pub fn set_refine(&mut self, refine: bool) -> &mut Self {
        unsafe {
            (*self.ptr).refine_edges = refine as _;
        }
        self
    }
    /// Take an image and detect any april tags in it.
    pub fn detect(&mut self, mut buffer: Buffer<'_>) -> DetectionIterator {
        match buffer.format {
            PixelFormat::Gray | PixelFormat::Luma => {}
            PixelFormat::GrayA
            | PixelFormat::Rgb
            | PixelFormat::Rgba
            | PixelFormat::Hsv
            | PixelFormat::Hsva => buffer.convert_inplace(PixelFormat::Gray),
            PixelFormat::LumaA | PixelFormat::YCbCr | PixelFormat::YCbCrA | PixelFormat::Yuyv => {
                buffer.convert_inplace(PixelFormat::Luma)
            }
        }
        assert_eq!(
            buffer.width as usize * buffer.height as usize,
            buffer.data.len(),
            "Buffer lengths don't match"
        );
        unsafe {
            let raw = image_u8 {
                width: buffer.width as _,
                height: buffer.height as _,
                stride: buffer.width as _,
                buf: buffer.data.as_ptr() as *mut u8,
            };
            DetectionIterator::new(apriltag_detector_detect(
                self.ptr,
                &raw as *const _ as *mut _,
            ))
        }
    }
}

/// A wrapper around a vector of apriltag detections, as given to us from a call to [`apriltag_detector_detect`].
#[derive(Debug)]
pub struct DetectionIterator {
    ptr: *mut zarray_t,
    min: usize,
    max: usize,
}
unsafe impl Send for DetectionIterator {}
unsafe impl Sync for DetectionIterator {}
impl Drop for DetectionIterator {
    fn drop(&mut self) {
        if !self.ptr.is_null() {
            self.by_ref().for_each(drop);
            unsafe {
                let arr = *self.ptr;
                libc::free(arr.data as *mut libc::c_void);
                libc::free(self.ptr as *mut libc::c_void);
            }
        }
    }
}
impl DetectionIterator {
    /// Create a new `DetectionIterator` from a pointer that hasn't been used yet.
    ///
    /// # Safety
    /// This pointer must be to a valid array that holds [`*mut apriltag_detection_t`]s.
    pub unsafe fn new(ptr: *mut zarray_t) -> Self {
        unsafe {
            let arr = *ptr;
            debug_assert_eq!(arr.el_sz, size_of::<*mut apriltag_detection_t>());
            Self {
                ptr,
                min: 0,
                max: arr.size as _,
            }
        }
    }
    /// Create a new `DetectionIterator` by taking ownership of a pointer.
    ///
    /// # Safety
    /// This pointer must be to a valid array, holding [`Detection`]s, and the indices must match the number of elements taken.
    pub const unsafe fn from_raw(ptr: *mut zarray_t, min: usize, max: usize) -> Self {
        Self { ptr, min, max }
    }
    /// Create an empty iterator.
    pub const fn empty() -> Self {
        Self {
            ptr: std::ptr::null_mut(),
            min: 0,
            max: 0,
        }
    }
    /// Convert this array back into its inner pointer, along with its start and end indices.
    pub fn into_raw(mut self) -> (*mut zarray_t, usize, usize) {
        (
            std::mem::replace(&mut self.ptr, std::ptr::null_mut()),
            self.min,
            self.max,
        )
    }
}
impl Iterator for DetectionIterator {
    type Item = Detection;

    fn next(&mut self) -> Option<Self::Item> {
        unsafe {
            (self.min < self.max).then(|| {
                let idx = self.min;
                self.min += 1;
                let arr = *self.ptr;
                Detection::from_raw(*(arr.data as *mut *mut apriltag_detection_t).add(idx))
            })
        }
    }
}
impl DoubleEndedIterator for DetectionIterator {
    fn next_back(&mut self) -> Option<Self::Item> {
        unsafe {
            (self.min < self.max).then(|| {
                let idx = self.min;
                self.max -= 1;
                let arr = *self.ptr;
                Detection::from_raw(*(arr.data as *mut *mut apriltag_detection_t).add(idx))
            })
        }
    }
}
impl ExactSizeIterator for DetectionIterator {
    fn len(&self) -> usize {
        self.max - self.min
    }
}

pub struct Detection {
    ptr: *mut apriltag_detection_t,
}
unsafe impl Send for Detection {}
unsafe impl Sync for Detection {}
impl Data for Detection {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
}
impl Debug for Detection {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("Detection")
            .field("ptr", &self.ptr)
            .field("id", &self.id())
            .field("hamming", &self.hamming())
            .field("corners", &self.corners())
            .field("center", &self.center())
            .finish()
    }
}
impl Clone for Detection {
    fn clone(&self) -> Self {
        unsafe {
            let ptr = libc::malloc(size_of::<apriltag_detection_t>()) as *mut _;
            std::ptr::copy_nonoverlapping(self.ptr, ptr, 1);
            (*ptr).H = libc::malloc(size_of::<matd_t>()) as *mut _;
            std::ptr::copy_nonoverlapping((*ptr).H, (*self.ptr).H, 1);
            Self { ptr }
        }
    }
}
impl Drop for Detection {
    fn drop(&mut self) {
        unsafe {
            apriltag_detection_destroy(self.ptr);
        }
    }
}
impl Detection {
    /// Create a new `Detection` by taking ownership of a pointer.
    ///
    /// # Safety
    /// This pointer must be to a valid detection.
    pub const unsafe fn from_raw(ptr: *mut apriltag_detection_t) -> Self {
        Self { ptr }
    }
    /// Convert this detection back into its inner pointer.
    pub fn into_raw(mut self) -> *mut apriltag_detection_t {
        std::mem::replace(&mut self.ptr, std::ptr::null_mut())
    }
    /// Get the ID of this tag.
    pub fn id(&self) -> i32 {
        unsafe { (*self.ptr).id }
    }
    /// Get the Hamming distance for this detection.
    pub fn hamming(&self) -> i32 {
        unsafe { (*self.ptr).hamming }
    }
    /// Get the homography matrix for this detection.
    pub fn homography(&self) -> [f64; 9] {
        unsafe {
            let mut buf = [0f64; 9];
            let data = *self.ptr;
            let mat = &*data.H;
            assert_eq!(mat.ncols, 3);
            assert_eq!(mat.nrows, 3);
            buf.copy_from_slice(mat.data.as_slice(9));
            buf
        }
    }
    /// Get the coordinates of the corners of the tag, in pixel coordinates.
    pub fn corners(&self) -> [[f64; 2]; 4] {
        unsafe { (*self.ptr).p }
    }
    /// Get the coordinates of the center of the tag, in pixel coordinates.
    pub fn center(&self) -> [f64; 2] {
        unsafe { (*self.ptr).c }
    }
    /// Estimate the pose of this tag.
    pub fn estimate_pose(&self, params: PoseParams) -> PoseEstimation {
        unsafe {
            let [fx, fy] = params.fov;
            let [cx, cy] = params.center;
            let mut info = apriltag_detection_info_t {
                det: self.ptr,
                tagsize: params.tag_size,
                fx,
                fy,
                cx,
                cy,
            };
            let mut pose = MaybeUninit::uninit();
            let error = estimate_tag_pose(&mut info as _, pose.as_mut_ptr());
            let calc = pose.assume_init();
            let mut pose = Pose::default();
            let mat = &*calc.R;
            assert_eq!(mat.nrows, 3);
            assert_eq!(mat.ncols, 3);
            std::ptr::copy_nonoverlapping(mat.data.as_ptr(), pose.rotation.as_mut_ptr(), 9);
            let mat = &*calc.t;
            assert_eq!(mat.nrows, 3);
            assert_eq!(mat.ncols, 1);
            std::ptr::copy_nonoverlapping(mat.data.as_ptr(), pose.translation.as_mut_ptr(), 3);
            PoseEstimation { pose, error }
        }
    }
}

/// Deserialization utilities for deserializing a `f64` with special values for tag size.
pub mod tag_size {
    use super::*;
    use serde::Deserializer;

    #[derive(Deserialize)]
    #[allow(non_camel_case_types)]
    enum ConstTag {
        #[serde(alias = "FRC_IN")]
        FRC_INCHES,
        #[serde(alias = "FRC_M")]
        FRC_METERS,
        #[serde(alias = "FRC_CENTIMETERS")]
        FRC_CM,
    }

    #[derive(Deserialize)]
    #[serde(untagged)]
    enum TagSize {
        Const(ConstTag),
        Other(f64),
    }

    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<f64, D::Error> {
        match TagSize::deserialize(deserializer)? {
            TagSize::Const(ConstTag::FRC_INCHES) => Ok(FRC_TAG_SIZE_INCHES),
            TagSize::Const(ConstTag::FRC_METERS) => Ok(FRC_TAG_SIZE_METERS),
            TagSize::Const(ConstTag::FRC_CM) => Ok(FRC_TAG_SIZE_CM),
            TagSize::Other(size) => Ok(size),
        }
    }
}

/// Parameters for pose estimation.
#[derive(Debug, Clone, Copy, PartialEq, Serialize, Deserialize)]
pub struct PoseParams {
    /// Length of the tag, in the same units as the output translation vector.
    ///
    /// When deserializing, the special string values of `FRC_INCHES`/`FRC_IN`, `FRC_METERS`/`FRC_M`, and `FRC_CENTIMETERS`/`FRC_CM` are recognized as constant values.
    #[serde(deserialize_with = "tag_size::deserialize")]
    pub tag_size: f64,
    /// Center of the image, in pixel coordinates.
    pub center: [f64; 2],
    /// Field of view, in pixels.
    pub fov: [f64; 2],
}
impl PoseParams {
    /// Create a set of parameters from a camera width, height, and FOV.
    ///
    /// The tag size will be 0.
    pub fn from_dimensions(width: u32, height: u32, fov_radians: f64) -> Self {
        let width = width as f64;
        let height = height as f64;
        let cx = width * 0.5;
        let cy = height * 0.5;
        let f = (fov_radians * 0.5).tan() * cx;
        PoseParams {
            tag_size: 0.0,
            center: [cx, cy],
            fov: [f; 2],
        }
    }
}

/// Tag size used in the FIRST robotics competition, in inches.
pub const FRC_TAG_SIZE_INCHES: f64 = 6.5;
/// Tag size used in the FIRST robotics competition, in meters.
pub const FRC_TAG_SIZE_METERS: f64 = 0.1651;
/// Tag size used in the FIRST robotics competition, in meters.
pub const FRC_TAG_SIZE_CM: f64 = 16.51;
