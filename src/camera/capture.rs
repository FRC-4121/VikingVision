use super::CameraImpl;
use super::config::{BasicConfig, Config};
use crate::buffer::{Buffer, PixelFormat};
use crate::delegate_camera_config;
use serde::{Deserialize, Serialize};
use std::fmt::{self, Debug, Formatter};
use std::io;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{error, info};
use v4l::buffer::Type;
use v4l::io::traits::{CaptureStream, Stream};
use v4l::prelude::MmapStream;
use v4l::{Device, FourCC};

mod fourcc_serde {
    use serde::de::{Deserializer, Error, Unexpected, Visitor};
    use serde::ser::{Error as _, Serializer};
    use v4l::FourCC;

    pub fn serialize<S: Serializer>(fourcc: &FourCC, serializer: S) -> Result<S::Ok, S::Error> {
        serializer.serialize_str(fourcc.str().map_err(S::Error::custom)?)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(deserializer: D) -> Result<FourCC, D::Error> {
        struct FourCCVisitor;
        impl Visitor<'_> for FourCCVisitor {
            type Value = FourCC;

            fn expecting(&self, formatter: &mut std::fmt::Formatter) -> std::fmt::Result {
                formatter.write_str("a fourcc string")
            }
            fn visit_bytes<E>(self, v: &[u8]) -> Result<Self::Value, E>
            where
                E: Error,
            {
                v.try_into()
                    .map(FourCC::new)
                    .map_err(|_| E::invalid_value(Unexpected::Bytes(v), &self))
            }
            fn visit_str<E>(self, v: &str) -> Result<Self::Value, E>
            where
                E: Error,
            {
                v.as_bytes()
                    .try_into()
                    .map(FourCC::new)
                    .map_err(|_| E::invalid_value(Unexpected::Str(v), &self))
            }
            fn visit_u32<E>(self, v: u32) -> Result<Self::Value, E>
            where
                E: Error,
            {
                Ok(FourCC::from(v))
            }
            fn visit_u64<E>(self, v: u64) -> Result<Self::Value, E>
            where
                E: Error,
            {
                u32::try_from(v)
                    .map(FourCC::from)
                    .map_err(|_| E::invalid_value(Unexpected::Unsigned(v as _), &self))
            }
            fn visit_i64<E>(self, v: i64) -> Result<Self::Value, E>
            where
                E: Error,
            {
                u32::try_from(v)
                    .map(FourCC::from)
                    .map_err(|_| E::invalid_value(Unexpected::Signed(v as _), &self))
            }
        }
        deserializer.deserialize_bytes(FourCCVisitor)
    }
}

#[typetag::serde]
pub trait CameraSource: Debug + Send + Sync {
    fn resolve(&self) -> io::Result<Device>;
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct V4lPath(pub PathBuf);
#[typetag::serde(name = "path")]
impl CameraSource for V4lPath {
    fn resolve(&self) -> io::Result<Device> {
        info!(path = %self.0.display(), "loading device");
        Device::with_path(&self.0)
    }
}
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct V4lIndex(pub usize);
#[typetag::serde(name = "index")]
impl CameraSource for V4lIndex {
    fn resolve(&self) -> io::Result<Device> {
        info!(index = self.0, "loading device");
        Device::new(self.0)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureCameraConfig {
    #[serde(flatten)]
    basic: BasicConfig,
    #[serde(with = "fourcc_serde")]
    fourcc: FourCC,
    #[serde(flatten)]
    source: Arc<dyn CameraSource>,
}
impl CaptureCameraConfig {
    #[inline(always)]
    fn basic(&self) -> &BasicConfig {
        &self.basic
    }
}
#[typetag::serde(name = "v4l")]
impl Config for CaptureCameraConfig {
    delegate_camera_config!(Self::basic);
    fn build_camera(&self) -> io::Result<Box<dyn CameraImpl>> {
        let device = self.source.resolve()?;
        let mut cam = CaptureCamera {
            config: self.clone(),
            stream: MmapStream::new(&device, Type::VideoCapture)?,
            device,
        };
        cam.config_device()?;
        Ok(Box::new(cam))
    }
}

pub struct CaptureCamera {
    pub config: CaptureCameraConfig,
    pub device: Device,
    pub stream: MmapStream<'static>,
}
impl CaptureCamera {
    /// Configure the device based on the configuration.
    pub fn config_device(&mut self) -> io::Result<()> {
        Ok(())
    }
}
impl Debug for CaptureCamera {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("CaptureCamera")
            .field("config", &self.config)
            .field("device_fd", &self.device.handle().fd())
            .field("stream_fd", &self.stream.handle().fd())
            .finish()
    }
}
impl CameraImpl for CaptureCamera {
    fn config(&self) -> &dyn Config {
        &self.config
    }
    fn read_frame(&mut self) -> io::Result<Buffer<'_>> {
        let (frame, _meta) = self.stream.next()?;
        Ok(Buffer {
            width: 640,
            height: 480,
            format: PixelFormat::Yuyv,
            data: frame.into(),
        })
    }
    fn reload(&mut self) -> bool {
        match self.config.source.resolve() {
            Ok(device) => self.device = device,
            Err(err) => {
                error!(%err, "failed to resolve v4l device");
                return true;
            }
        }
        if let Err(err) = self.config_device() {
            error!(%err, "failed to configure the device");
            return true;
        }
        if let Err(err) = self.stream.stop() {
            error!(%err, "failed to close camera stream");
            return true;
        }
        match MmapStream::new(&self.device, Type::VideoCapture) {
            Ok(stream) => self.stream = stream,
            Err(err) => error!(%err, "failed to resolve v4l device"),
        }
        true
    }
}
