use super::CameraImpl;
use serde::{Deserialize, Serialize};
use std::io;
use std::time::Duration;

/// Serializable configuration for a camera.
#[typetag::serde(tag = "type")]
pub trait CameraConfig {
    fn width(&self) -> u32;
    fn height(&self) -> u32;
    fn build_camera(&self) -> io::Result<Box<dyn CameraImpl>>;

    fn min_frame(&self) -> Duration {
        Duration::ZERO
    }
    fn fov(&self) -> Option<f32> {
        None
    }
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct BasicConfig {
    pub width: u32,
    pub height: u32,
    pub fov: Option<f32>,
    pub max_fps: Option<f32>,
}

/// Delegate `Config` implementation to `BasicConfig` or similar
#[macro_export]
macro_rules! delegate_camera_config {
    ($this:expr) => {
        $crate::delegate_camera_config!(@inner $this => width height fov max_fps);
    };
    ($this:expr => $($args:ident),*) => {
        $crate::delegate_camera_config!(@inner $this => $($args)*);
    };
    ($this:expr => $($args:ident),*) => {
        $crate::delegate_camera_config!(@inner $this => $($args)*);
    };
    (@inner $this:expr =>) => {};
    (@inner $this:expr => width $($rest:ident)*) => {
        #[inline(always)]
        fn width(&self) -> u32 {
            $this(self).width
        }
        $crate::delegate_camera_config!(@inner $this => $($rest)*);
    };
    (@inner $this:expr => height $($rest:ident)*) => {
        #[inline(always)]
        fn height(&self) -> u32 {
            $this(self).height
        }
        $crate::delegate_camera_config!(@inner $this => $($rest)*);
    };
    (@inner $this:expr => fov $($rest:ident)*) => {
        #[inline(always)]
        fn fov(&self) -> ::std::option::Option<f32> {
            $this(self).fov
        }
        $crate::delegate_camera_config!(@inner $this => $($rest)*);
    };
    (@inner $this:expr => max_fps $($rest:ident)*) => {
        #[inline(always)]
        fn min_frame(&self) -> ::std::time::Duration {
            $this(self).max_fps.map_or(::std::time::Duration::ZERO, |fps| ::std::time::Duration::from_secs(1).div_f32(fps))
        }
        $crate::delegate_camera_config!(@inner $this => $($rest)*);
    };
}
