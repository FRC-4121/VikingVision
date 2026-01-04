use crate::buffer::Buffer;
use crate::pipeline::{PipelineId, PipelineName};
use serde::{Deserialize, Serialize};
use std::any::Any;
use std::fmt::{self, Debug, Formatter};
use std::io;
use std::ops::{Deref, DerefMut};
use std::time::{Duration, Instant};
use supply::prelude::*;
use tracing::{debug, error, info, info_span};

pub mod capture;
pub mod frame;

/// Typed identifier for the field of view for a camera.
///
/// This is given in degrees.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Fov(pub f64);

/// Typed identifier for the expected size from a camera.
#[derive(Debug, Clone, Copy, PartialEq, PartialOrd, Serialize, Deserialize)]
pub struct FrameSize {
    pub width: u32,
    pub height: u32,
}

#[ty_tag::tag]
pub type FovTag = Fov;

#[ty_tag::tag]
pub type ExpectedSizeTag = FrameSize;

pub trait CameraImpl: Any + Send + Sync {
    /// Get the expected size of the frame.
    fn frame_size(&self) -> FrameSize;
    /// Load a frame that can be retrieved with [`Self::get_frame`].
    fn load_frame(&mut self) -> io::Result<()>;
    /// Get the frame loaded with [`Self::load_frame`].
    fn get_frame(&self) -> Buffer<'_>;

    /// Try to reload the camera. Return false if we should stop trying.
    fn reload(&mut self) -> bool {
        false
    }
    /// Debug this implementation to a formatter. This is used in the `Debug` impl for the trait.
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        disqualified::ShortName::of::<Self>().fmt(f)
    }
}
impl Debug for dyn CameraImpl {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        self.debug(f)
    }
}

/// Serializable configuration for a camera.
#[typetag::serde(tag = "type")]
pub trait CameraFactory {
    fn build_camera(&self) -> io::Result<Box<dyn CameraImpl>>;
}

#[derive(Serialize, Deserialize)]
pub struct CameraConfig {
    pub fov: Option<Fov>,
    pub resize: Option<FrameSize>,
    pub max_fps: Option<f64>,
    #[serde(flatten)]
    pub factory: Box<dyn CameraFactory>,
}
impl CameraConfig {
    /// Get the metadata for this configuration.
    pub fn metadata(&self, name: String) -> CameraMetadata {
        CameraMetadata {
            name,
            min_frame: self
                .max_fps
                .map_or(Duration::ZERO, |f| Duration::from_secs_f64(f.recip())),
            fov: self.fov,
            resize: self.resize,
        }
    }
    /// Build a [`Camera`].
    pub fn build_camera(&self, name: String) -> io::Result<Camera> {
        let _guard = tracing::error_span!("build_camera", name).entered();
        let inner = self.factory.build_camera()?;
        Ok(Camera {
            meta: self.metadata(name),
            querier: CameraQuerier::new(inner),
        })
    }
}

/// The part of a [`Camera`] that actually handles reading from the implementation.
#[derive(Debug)]
pub struct CameraQuerier {
    inner: Box<dyn CameraImpl>,
    resized: Option<Buffer<'static>>,
    fail_count: usize,
    backoff: usize,
    last_frame: Instant,
}
impl CameraQuerier {
    /// Create a new querier.
    pub fn new(inner: Box<dyn CameraImpl>) -> Self {
        Self {
            inner,
            fail_count: 0,
            backoff: 1,
            resized: None,
            last_frame: Instant::now(),
        }
    }
    /// Get a reference to the implementation.
    pub fn inner(&self) -> &dyn CameraImpl {
        &*self.inner
    }
    /// Get a mutable reference to the implementation.
    pub fn inner_mut(&mut self) -> &mut dyn CameraImpl {
        &mut *self.inner
    }
    /// Attempt to downcast the implementation to a concrete type.
    pub fn downcast_ref<T: CameraImpl>(&self) -> Option<&T> {
        let any = self.inner() as &dyn Any;
        any.downcast_ref()
    }
    /// Attempt to mutably downcast the implementation to a concrete type.
    pub fn downcast_mut<T: CameraImpl>(&mut self) -> Option<&mut T> {
        let any = self.inner_mut() as &mut dyn Any;
        any.downcast_mut()
    }
    /// Read a frame, reloading the camera if necessary.
    ///
    /// The name is needed for logging.
    pub fn load_frame(&mut self, meta: &CameraMetadata) -> io::Result<()> {
        let _guard = info_span!("reading frame", name = meta.name);
        let now = Instant::now();
        if let Some(to_sleep) = meta.min_frame.checked_sub(now - self.last_frame) {
            debug!(?to_sleep, "sleeping to throttle framerate");
            std::thread::sleep(to_sleep);
            self.last_frame = now;
        }
        let res = self.inner.load_frame();
        if let Err(err) = &res {
            self.fail_count += 1;
            error!(%err, fail_count = self.fail_count, "failed to read frame");
            if self.fail_count == self.backoff {
                info!("reloading camera");
                let _guard = info_span!("reloading");
                let retry = self.inner.reload();
                if retry {
                    self.backoff *= 2;
                } else {
                    self.backoff = 0;
                }
            }
        } else if let Some(size) = meta.resize
            && size != self.inner.frame_size()
        {
            crate::vision::resize(
                self.inner.get_frame(),
                self.resized.get_or_insert_default(),
                size.width,
                size.height,
            );
        }
        res
    }
    /// Get the last frame read.
    ///
    /// There should've been a call to [`Self::load_frame`] first.
    pub fn get_frame(&self) -> Buffer<'_> {
        self.resized
            .as_ref()
            .map_or_else(|| self.inner.get_frame(), Buffer::borrow)
    }
    /// Fetch a frame, and if it was successful, return it.
    ///
    /// The name is needed for logging.
    pub fn read(&mut self, meta: &CameraMetadata) -> io::Result<Buffer<'_>> {
        self.load_frame(meta).map(|_| self.get_frame())
    }
    /// Get the pipeline ID from the implementation
    pub fn id(&self) -> PipelineId {
        PipelineId::from_ptr(&*self.inner)
    }
}

/// Metadata for the camera.
#[derive(Debug, Clone)]
pub struct CameraMetadata {
    /// The name to be used for logging.
    pub name: String,
    pub min_frame: Duration,
    pub fov: Option<Fov>,
    pub resize: Option<FrameSize>,
}
impl CameraMetadata {
    pub fn new(name: String) -> Self {
        Self {
            name,
            min_frame: Duration::ZERO,
            fov: None,
            resize: None,
        }
    }
}
impl<'r> Provider<'r> for CameraMetadata {
    type Lifetimes = l!['r];
    fn provide(&'r self, want: &mut dyn Want<Self::Lifetimes>) {
        want.provide_value(PipelineName(&self.name));
        if let Some(fov) = self.fov {
            want.provide_value(fov);
        }
    }
}

/// A reference to the [`CameraMetadata`], along with metadata that's derived from the querier.
#[derive(Debug, Clone)]
pub struct FullCameraMetadata {
    /// The original camera metadata.
    pub meta: CameraMetadata,
    /// The pipeline ID.
    pub id: PipelineId,
    /// The size of the frame.
    pub size: FrameSize,
}
impl Deref for FullCameraMetadata {
    type Target = CameraMetadata;
    fn deref(&self) -> &Self::Target {
        &self.meta
    }
}
impl DerefMut for FullCameraMetadata {
    fn deref_mut(&mut self) -> &mut Self::Target {
        &mut self.meta
    }
}
impl<'r> Provider<'r> for FullCameraMetadata {
    type Lifetimes = l!['r];
    fn provide(&'r self, want: &mut dyn Want<Self::Lifetimes>) {
        want.provide_value(self.id).provide_value(self.size);
        self.meta.provide(want);
    }
}

/// A source of frames.
///
/// A [`Camera`] wraps a [`CameraImpl`] and handles framerate throttling and reloading with exponential backoff.
#[derive(Debug)]
pub struct Camera {
    pub meta: CameraMetadata,
    pub querier: CameraQuerier,
}
impl Camera {
    /// Create a new camera from a name and an implementation.
    pub fn new(name: String, inner: Box<dyn CameraImpl>) -> Self {
        Self {
            meta: CameraMetadata::new(name),
            querier: CameraQuerier::new(inner),
        }
    }
    /// Get the name of the camera.
    pub fn name(&self) -> &str {
        &self.meta.name
    }
    /// Get a reference to the implementation.
    pub fn inner(&self) -> &dyn CameraImpl {
        self.querier.inner()
    }
    /// Get a mutable reference to the implementation.
    pub fn inner_mut(&mut self) -> &mut dyn CameraImpl {
        self.querier.inner_mut()
    }
    /// Attempt to downcast the implementation to a concrete type.
    pub fn downcast_ref<T: CameraImpl>(&self) -> Option<&T> {
        self.querier.downcast_ref()
    }
    /// Attempt to mutably downcast the implementation to a concrete type.
    pub fn downcast_mut<T: CameraImpl>(&mut self) -> Option<&mut T> {
        self.querier.downcast_mut()
    }
    /// Read a frame, reloading the camera if necessary.
    pub fn load_frame(&mut self) -> io::Result<()> {
        self.querier.load_frame(&self.meta)
    }
    /// Get the last frame read.
    ///
    /// There should've been a call to [`Self::load_frame`] first.
    pub fn get_frame(&self) -> Buffer<'_> {
        self.querier.get_frame()
    }
    /// Fetch a frame, and if it was successful, return it.
    pub fn read(&mut self) -> io::Result<Buffer<'_>> {
        self.load_frame().map(|_| self.get_frame())
    }
    /// Split this camera into full metadata and the querier.
    pub fn split(self) -> (FullCameraMetadata, CameraQuerier) {
        let full = FullCameraMetadata {
            size: self
                .meta
                .resize
                .unwrap_or_else(|| self.inner().frame_size()),
            id: self.querier.id(),
            meta: self.meta,
        };
        (full, self.querier)
    }
    /// Clone the metadata and fill it in with metadata supplied from the querier.
    pub fn clone_full_metadata(&self) -> FullCameraMetadata {
        FullCameraMetadata {
            size: self
                .meta
                .resize
                .unwrap_or_else(|| self.inner().frame_size()),
            id: self.querier.id(),
            meta: self.meta.clone(),
        }
    }
}

impl<'r> Provider<'r> for Camera {
    type Lifetimes = l!['r];

    fn provide(&'r self, want: &mut dyn Want<Self::Lifetimes>) {
        want.provide_value(self.querier.id());
        self.meta.provide(want);
    }
}
