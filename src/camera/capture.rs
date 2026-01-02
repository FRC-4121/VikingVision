#![cfg(feature = "v4l")]

use super::CameraImpl;
use super::config::{BasicConfig, CameraConfig};
use crate::buffer::{Buffer, PixelFormat};
use crate::delegate_camera_config;
use polonius_the_crab::{ForLt, Placeholder, PoloniusResult, polonius};
use serde::{Deserialize, Serialize};
use std::fmt::{self, Debug, Formatter};
use std::io;
use std::path::PathBuf;
use std::sync::{Arc, LazyLock};
use tracing::{error, info};
use v4l::buffer::Type;
use v4l::control::{Control, Value};
use v4l::io::traits::CaptureStream;
use v4l::prelude::MmapStream;
use v4l::video::Capture;
use v4l::video::capture::Parameters;
use v4l::{Device, FourCC, Fraction};
use zune_jpeg::{JpegDecoder, zune_core::options::DecoderOptions};

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
mod interval_serde {
    use serde::{Deserialize, Deserializer, Serialize, Serializer};
    use v4l::Fraction;

    #[derive(Serialize, Deserialize)]
    struct FrameInterval {
        top: u32,
        bottom: u32,
    }

    #[derive(Serialize, Deserialize)]
    enum FrameIntervalShim {
        #[serde(rename = "fps")]
        Fps(u32),
        #[serde(rename = "interval")]
        Interval(FrameInterval),
    }

    pub fn serialize<S: Serializer>(
        fraction: &Option<Fraction>,
        serializer: S,
    ) -> Result<S::Ok, S::Error> {
        fraction
            .map(|f| {
                FrameIntervalShim::Interval(FrameInterval {
                    top: f.numerator,
                    bottom: f.denominator,
                })
            })
            .serialize(serializer)
    }
    pub fn deserialize<'de, D: Deserializer<'de>>(
        deserializer: D,
    ) -> Result<Option<Fraction>, D::Error> {
        Option::<FrameIntervalShim>::deserialize(deserializer).map(|o| {
            o.map(|i| match i {
                FrameIntervalShim::Fps(denominator) => Fraction {
                    numerator: 1,
                    denominator,
                },
                FrameIntervalShim::Interval(i) => Fraction {
                    numerator: i.top,
                    denominator: i.bottom,
                },
            })
        })
    }
}

pub mod control_ids {
    pub const BRIGHTNESS: u32 = 0x00980900;
    pub const CONTRAST: u32 = 0x00980901;
    pub const SATURATION: u32 = 0x00980902;
    pub const WHITE_BALANCE_AUTOMATIC: u32 = 0x0098090c;
    pub const GAIN: u32 = 0x00980913;
    pub const POWER_LINE_FREQUENCY: u32 = 0x00980918;
    pub const WHITE_BALANCE_TEMPERATURE: u32 = 0x0098091a;
    pub const SHARPNESS: u32 = 0x0098091b;
    pub const BACKLIGHT_COMPENSATION: u32 = 0x0098091c;
    pub const AUTO_EXPOSURE: u32 = 0x009a0901;
    pub const EXPOSURE_TIME_ABSOLUTE: u32 = 0x009a0902;
    pub const EXPOSURE_DYNAMIC_FRAMERATE: u32 = 0x009a0903;
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
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
pub struct NoSource {}
#[typetag::serde(name = "unknown")]
impl CameraSource for NoSource {
    fn resolve(&self) -> io::Result<Device> {
        error!("unknown source");
        Err(io::Error::other("unknown source type"))
    }
}

static NO_SOURCE: LazyLock<Arc<NoSource>> = LazyLock::new(|| Arc::new(NoSource {}));

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CaptureCameraConfig {
    #[serde(flatten)]
    pub basic: BasicConfig,
    #[serde(with = "fourcc_serde")]
    pub fourcc: FourCC,
    pub pixel_format: Option<PixelFormat>,
    #[serde(default)]
    pub decode_jpeg: bool,
    pub exposure: Option<i64>,
    #[serde(with = "interval_serde", flatten)]
    pub interval: Option<Fraction>,
    #[serde(flatten)]
    pub source: Arc<dyn CameraSource>,
}
impl CaptureCameraConfig {
    #[inline(always)]
    fn basic(&self) -> &BasicConfig {
        &self.basic
    }
}
#[typetag::serde(name = "v4l")]
impl CameraConfig for CaptureCameraConfig {
    delegate_camera_config!(Self::basic);
    fn build_camera(&self) -> io::Result<Box<dyn CameraImpl>> {
        let device = self.source.resolve()?;
        let mut cam = CaptureCamera {
            config: self.clone(),
            stream: None,
            device,
            jpeg_buf: None,
        };
        cam.config_device()?;
        Ok(Box::new(cam))
    }
}

pub struct CaptureCamera {
    pub config: CaptureCameraConfig,
    pub device: Device,
    pub stream: Option<MmapStream<'static>>,
    pub jpeg_buf: Option<Buffer<'static>>,
}
impl CaptureCamera {
    pub fn from_device(device: Device) -> io::Result<Self> {
        let format = device.format()?;
        let fmt = format
            .fourcc
            .try_into()
            .map_err(|err| io::Error::new(io::ErrorKind::Unsupported, err))?;
        let config = CaptureCameraConfig {
            basic: BasicConfig {
                width: format.width,
                height: format.height,
                fov: None,
                max_fps: None,
            },
            fourcc: format.fourcc,
            pixel_format: Some(fmt),
            decode_jpeg: &format.fourcc.repr == b"MJPG",
            interval: None,
            exposure: None,
            source: NO_SOURCE.clone(),
        };
        Ok(Self {
            config,
            device,
            stream: None,
            jpeg_buf: None,
        })
    }
    /// Configure the device based on the configuration.
    pub fn config_device(&mut self) -> io::Result<()> {
        self.stream = None;
        if let Some(exposure) = self.config.exposure {
            self.device.set_control(Control {
                id: control_ids::AUTO_EXPOSURE,
                value: Value::Integer(1),
            })?;
            self.device.set_control(Control {
                id: control_ids::EXPOSURE_DYNAMIC_FRAMERATE,
                value: Value::Boolean(false),
            })?;
            self.device.set_control(Control {
                id: control_ids::EXPOSURE_TIME_ABSOLUTE,
                value: Value::Integer(exposure),
            })?;
        } else {
            self.device.set_control(Control {
                id: control_ids::AUTO_EXPOSURE,
                value: Value::Integer(3),
            })?;
            self.device.set_control(Control {
                id: control_ids::EXPOSURE_DYNAMIC_FRAMERATE,
                value: Value::Boolean(true),
            })?;
        }
        self.device.set_format(&v4l::Format::new(
            self.config.width(),
            self.config.height(),
            self.config.fourcc,
        ))?;
        let interval = self.interval_mut()?;
        self.device.set_params(&Parameters::new(interval))?;
        let fmt = self
            .config
            .fourcc
            .try_into()
            .map_err(|err| io::Error::new(io::ErrorKind::Unsupported, err))?;
        self.config.pixel_format = Some(fmt);
        self.config.decode_jpeg = &self.config.fourcc.repr == b"MJPG";
        Ok(())
    }
    /// Initialize a stream from a device if it's not available.
    pub fn make_stream<'a>(
        stream: &'a mut Option<MmapStream<'static>>,
        device: &Device,
    ) -> io::Result<&'a mut MmapStream<'static>> {
        let res = polonius::<_, _, ForLt!(&'_ mut MmapStream<'static>)>(stream, |stream| {
            if let Some(stream) = stream {
                PoloniusResult::Borrowing(stream)
            } else {
                PoloniusResult::Owned {
                    value: (),
                    input_borrow: Placeholder,
                }
            }
        });
        match res {
            PoloniusResult::Borrowing(stream) => Ok(stream),
            PoloniusResult::Owned {
                value: _,
                input_borrow,
            } => {
                *input_borrow = Some(MmapStream::with_buffers(device, Type::VideoCapture, 4)?);
                Ok(input_borrow.as_mut().unwrap())
            }
        }
    }
    /// Get the video stream, creating it if it's not available.
    pub fn stream(&mut self) -> io::Result<&mut MmapStream<'static>> {
        Self::make_stream(&mut self.stream, &self.device)
    }
    /// Set the width and height. Changes won't take effect until `config_device` is called.
    pub const fn set_resolution(&mut self, width: u32, height: u32) {
        self.config.basic.width = width;
        self.config.basic.height = height;
    }
    /// Set the FourCC. Changes won't take effect until `config_device` is called.
    pub const fn set_fourcc(&mut self, fourcc: FourCC) {
        self.config.fourcc = fourcc;
    }
    /// Set the frame interval. Changes won't take effect until `config_device` is called.
    pub const fn set_interval(&mut self, interval: Fraction) {
        self.config.interval = Some(interval);
    }
    /// Get the configured width.
    pub const fn width(&self) -> u32 {
        self.config.basic.width
    }
    /// Get the configured height.
    pub const fn height(&self) -> u32 {
        self.config.basic.height
    }
    /// Get the configured FourCC.
    pub const fn fourcc(&self) -> FourCC {
        self.config.fourcc
    }
    /// Get the configured frame interval.
    pub const fn interval(&self) -> Option<Fraction> {
        self.config.interval
    }
    /// Get the frame interval, or figure it out if not configured.
    pub fn interval_mut(&mut self) -> io::Result<Fraction> {
        if let Some(int) = self.config.interval {
            return Ok(int);
        }
        let int = self.device.params()?.interval;
        self.config.interval = Some(int);
        Ok(int)
    }
}
impl Debug for CaptureCamera {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        f.debug_struct("CaptureCamera")
            .field("config", &self.config)
            .field("device_fd", &self.device.handle().fd())
            .field("stream_fd", &self.stream.as_ref().map(|s| s.handle().fd()))
            .finish()
    }
}
impl CameraImpl for CaptureCamera {
    fn config(&self) -> &dyn CameraConfig {
        &self.config
    }
    fn read_frame(&mut self) -> io::Result<Buffer<'_>> {
        let width = self.width();
        let height = self.height();
        let (frame, _meta) = Self::make_stream(&mut self.stream, &self.device)?
            .next()
            .inspect_err(|err| error!(%err, "failed to read from stream"))?;
        if self.config.decode_jpeg {
            let px_buf = self.jpeg_buf.get_or_insert_default();
            let mut decoder = JpegDecoder::new_with_options(frame, DecoderOptions::new_fast());
            px_buf.width = width;
            px_buf.height = height;
            px_buf.format = PixelFormat::RGB;
            let px_data = px_buf.resize_data();
            if let Err(err) = decoder.decode_into(&mut *px_data) {
                error!(%err, "failed to decode JPEG data");
            }
            return Ok(px_buf.borrow());
        }
        Ok(Buffer {
            width,
            height,
            format: self.config.pixel_format.unwrap(),
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
        true
    }
}
