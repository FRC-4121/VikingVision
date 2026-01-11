#![cfg(feature = "apriltag")]

use crate::buffer::Buffer;
use crate::camera::{Fov, FrameSize};
use crate::mutex::Mutex;
use crate::pipeline::prelude::*;
use serde::{Deserialize, Serialize};
use supply::ProviderExt;

#[derive(Debug)]
pub struct AprilTagComponent {
    pub detector: Mutex<vv_apriltag::Detector>,
}
impl Component for AprilTagComponent {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, name: &str) -> OutputKind {
        match name {
            "" => OutputKind::Multiple,
            "vec" => OutputKind::Single,
            "found" => OutputKind::Single,
            _ => OutputKind::None,
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(img) = context.get_as::<Buffer>(None).and_log_err() else {
            return;
        };
        let grayscale = img.convert_cow(crate::buffer::PixelFormat::LUMA);
        let it = {
            let Ok(mut lock) = self.detector.lock() else {
                tracing::warn!("poisoned mutex for detector");
                return;
            };
            lock.detect(grayscale)
        };
        let listening_vec = context.listening("vec");
        let listening_elem = context.listening("");
        if context.listening("found") {
            context.submit("found", it.len());
        }
        let mut vec = Vec::new();
        if listening_vec {
            vec.reserve(it.len());
        }
        for elem in it {
            match [listening_elem, listening_vec] {
                [true, false] => context.submit("", elem),
                [false, true] => vec.push(elem),
                [true, true] => {
                    vec.push(elem.clone());
                    context.submit("", elem);
                }
                [false, false] => {}
            }
        }
        if listening_vec {
            context.submit("vec", vec);
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AprilTagFactory {
    #[serde(flatten)]
    pub config: vv_apriltag::DetectorConfig,
}
#[typetag::serde(name = "vv_apriltag")]
impl ComponentFactory for AprilTagFactory {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(AprilTagComponent {
            detector: Mutex::new(vv_apriltag::Detector::from_config(&self.config)),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "spec", rename_all = "lowercase")]
pub enum DetectPoseComponent {
    Fixed(vv_apriltag::PoseParams),
    Infer {
        #[serde(deserialize_with = "vv_apriltag::tag_size::deserialize")]
        tag_size: f64,
    },
}
impl Component for DetectPoseComponent {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn can_take(&self, input: &str) -> bool {
        input == "frame"
    }
    fn output_kind(&self, name: &str) -> OutputKind {
        match name {
            "" | "pose" | "error" => OutputKind::Single,
            _ => OutputKind::None,
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let params = match *self {
            Self::Fixed(p) => p,
            Self::Infer { tag_size } => {
                let Some(Fov(fov)) = context.context.request::<Fov>() else {
                    tracing::error!("attempted to infer parameters for a camera without an FOV");
                    return;
                };
                let Some(FrameSize { width, height }) = context.context.request::<FrameSize>()
                else {
                    tracing::error!(
                        "attempted to infer parameters for a camera without a frame size"
                    );
                    return;
                };
                vv_apriltag::PoseParams {
                    tag_size,
                    ..vv_apriltag::PoseParams::from_dimensions(width, height, fov)
                }
            }
        };
        let Ok(detection) = context.get_as::<vv_apriltag::Detection>(None).and_log_err() else {
            return;
        };
        let pose = detection.estimate_pose(params);
        context.submit_if_listening("", || pose);
        context.submit_if_listening("pose", || pose.pose);
        context.submit_if_listening("error", || pose.error);
    }
}
#[typetag::serde(name = "april-pose")]
impl ComponentFactory for DetectPoseComponent {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(self.clone())
    }
}
