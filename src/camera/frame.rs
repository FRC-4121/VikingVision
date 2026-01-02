use super::{CameraFactory, CameraImpl};
use crate::buffer::{Buffer, PixelFormat};
use serde::{Deserialize, Serialize};
use std::io;
use std::path::PathBuf;
use tracing::{error, info_span};

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
                let fsize = format.pixel_size();
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

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
pub enum FrameCameraConfig {
    Path {
        path: PathBuf,
    },
    Color {
        width: u32,
        height: u32,
        color: Color,
    },
}

#[typetag::serde(name = "frame")]
impl CameraFactory for FrameCameraConfig {
    fn build_camera(&self) -> io::Result<Box<dyn CameraImpl>> {
        let buffer = match self {
            Self::Path { path } => {
                let _guard = info_span!("loading image", path = %path.display());
                let buf =
                    std::fs::read(path).inspect_err(|err| error!(%err, "failed to open file"))?;
                Buffer::decode_img_data(&buf)?
            }
            &Self::Color {
                width,
                height,
                color: Color { format, ref bytes },
            } => Buffer::monochrome(width, height, format, bytes),
        };
        Ok(Box::new(FrameCamera { buffer }))
    }
}

#[derive(Debug, Clone)]
pub struct FrameCamera {
    pub buffer: Buffer<'static>,
}
impl CameraImpl for FrameCamera {
    fn frame_size(&self) -> super::FrameSize {
        super::FrameSize {
            width: self.buffer.width,
            height: self.buffer.height,
        }
    }
    fn load_frame(&mut self) -> io::Result<()> {
        Ok(())
    }
    fn get_frame(&self) -> Buffer<'_> {
        self.buffer.borrow()
    }
}
