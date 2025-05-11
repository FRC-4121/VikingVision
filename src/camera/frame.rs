use super::CameraImpl;
use super::config::BasicConfig;
use crate::buffer::{Buffer, PixelFormat};
use crate::delegate_camera_config;
use image::DynamicImage;
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;
use tracing::{error, info_span};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FrameCameraConfig {
    #[serde(flatten)]
    basic: BasicConfig,
    #[serde(flatten)]
    source: ImageSource,
}
impl FrameCameraConfig {
    #[inline(always)]
    fn basic(&self) -> &BasicConfig {
        &self.basic
    }
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub enum ImageSource {
    #[serde(rename = "path")]
    Path(PathBuf),
    #[serde(rename = "color")]
    Color(Color),
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(try_from = "ColorShim")]
pub struct Color {
    pub format: PixelFormat,
    pub bytes: Vec<u8>,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
enum ColorShim {
    Bytes { format: PixelFormat, bytes: Vec<u8> },
    String(String),
}

impl TryFrom<ColorShim> for Color {
    type Error = String;

    fn try_from(value: ColorShim) -> Result<Self, Self::Error> {
        match value {
            ColorShim::Bytes { format, bytes } => {
                let fsize = format.pixel_size() as usize;
                let bsize = bytes.len();
                if fsize != bsize {
                    return Err(format!(
                        "{format} expects {fsize} bytes, but {bsize} were given"
                    ));
                }
                Ok(Color { format, bytes })
            }
            ColorShim::String(_s) => Err("Color string parsing isn't yet supported".to_string()),
        }
    }
}

#[typetag::serde(name = "frame")]
impl super::Config for FrameCameraConfig {
    delegate_camera_config!(FrameCameraConfig::basic);

    fn build_camera(&self) -> io::Result<Box<dyn CameraImpl>> {
        let width = self.width();
        let height = self.height();
        let buffer = match &self.source {
            ImageSource::Color(Color { format, bytes }) => {
                Buffer::monochrome(width, height, *format, bytes)
            }
            ImageSource::Path(path) => {
                let _guard = info_span!("loading image", path = %path.display());
                let image = image::ImageReader::open(path)
                    .inspect_err(|err| error!(%err, "failed to open file"))?
                    .with_guessed_format()
                    .inspect_err(|err| error!(%err, "failed to guess format"))?
                    .decode()
                    .map_err(|err| {
                        error!(%err, "error decoding image");
                        io::Error::new(io::ErrorKind::InvalidData, err)
                    })?;
                let (format, data) = match image {
                    DynamicImage::ImageLuma8(img) => (PixelFormat::Luma, img.into_raw()),
                    DynamicImage::ImageLumaA8(img) => (PixelFormat::LumaA, img.into_raw()),
                    DynamicImage::ImageRgb8(img) => (PixelFormat::Rgb, img.into_raw()),
                    DynamicImage::ImageRgba8(img) => (PixelFormat::Rgba, img.into_raw()),
                    DynamicImage::ImageLuma16(img) => (
                        PixelFormat::Luma,
                        img.into_raw().into_iter().map(|p| (p >> 8) as u8).collect(),
                    ),
                    DynamicImage::ImageLumaA16(img) => (
                        PixelFormat::LumaA,
                        img.into_raw().into_iter().map(|p| (p >> 8) as u8).collect(),
                    ),
                    DynamicImage::ImageRgb16(img) => (
                        PixelFormat::Rgb,
                        img.into_raw().into_iter().map(|p| (p >> 8) as u8).collect(),
                    ),
                    DynamicImage::ImageRgba16(img) => (
                        PixelFormat::Rgba,
                        img.into_raw().into_iter().map(|p| (p >> 8) as u8).collect(),
                    ),
                    DynamicImage::ImageRgb32F(img) => (
                        PixelFormat::Luma,
                        img.into_raw()
                            .into_iter()
                            .map(|p| (p * 256.0).min(255.0) as u8)
                            .collect(),
                    ),
                    DynamicImage::ImageRgba32F(img) => (
                        PixelFormat::LumaA,
                        img.into_raw()
                            .into_iter()
                            .map(|p| (p * 256.0).min(255.0) as u8)
                            .collect(),
                    ),
                    _ => {
                        return Err(io::Error::new(
                            io::ErrorKind::InvalidData,
                            "invalid image type",
                        ));
                    }
                };
                Buffer {
                    width,
                    height,
                    format,
                    data: data.into(),
                }
            }
        };
        Ok(Box::new(FrameCamera {
            config: self.clone(),
            buffer,
        }))
    }
}

#[derive(Debug, Clone)]
pub struct FrameCamera {
    pub config: FrameCameraConfig,
    pub buffer: Buffer<'static>,
}
impl CameraImpl for FrameCamera {
    fn config(&self) -> &dyn super::Config {
        &self.config
    }
    fn read_frame(&mut self) -> io::Result<Buffer<'_>> {
        Ok(self.buffer.borrow())
    }
}
