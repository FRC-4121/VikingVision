#![cfg(feature = "apriltag")]

use crate::apriltag;
use crate::buffer::Buffer;
use crate::camera::Camera;
use crate::pipeline::prelude::*;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use supply::ProviderExt;

#[derive(Debug)]
pub struct AprilTagComponent {
    pub detector: Mutex<apriltag::Detector>,
}
impl Component for AprilTagComponent {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, name: Option<&str>) -> OutputKind {
        match name {
            None => OutputKind::Multiple,
            Some("vec") => OutputKind::Single,
            Some("found") => OutputKind::Single,
            _ => OutputKind::None,
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(img) = context.get_as::<Buffer>(None).and_log_err() else {
            return;
        };
        let it = {
            let Ok(mut lock) = self.detector.lock() else {
                tracing::warn!("poisoned mutex for detector");
                return;
            };
            lock.detect(img.borrow())
        };
        let listening_vec = context.listening("vec");
        let listening_elem = context.listening(None);
        if context.listening("found") {
            context.submit("found", it.len());
        }
        let mut vec = Vec::new();
        if listening_vec {
            vec.reserve(it.len());
        }
        for elem in it {
            match [listening_elem, listening_vec] {
                [true, false] => context.submit(None, elem),
                [false, true] => vec.push(elem),
                [true, true] => {
                    vec.push(elem.clone());
                    context.submit(None, elem);
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
    pub config: apriltag::DetectorConfig,
}
#[typetag::serde(name = "apriltag")]
impl ComponentFactory for AprilTagFactory {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(AprilTagComponent {
            detector: Mutex::new(apriltag::Detector::from_config(&self.config)),
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(tag = "spec", rename_all = "lowercase")]
pub enum DetectPoseComponent {
    Fixed(apriltag::PoseParams),
    Infer {
        #[serde(deserialize_with = "apriltag::tag_size::deserialize")]
        tag_size: f64,
    },
}
impl Component for DetectPoseComponent {
    fn inputs(&self) -> Inputs {
        Inputs::Primary
    }
    fn output_kind(&self, name: Option<&str>) -> OutputKind {
        if name.is_none() {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let params = match *self {
            Self::Fixed(p) => p,
            Self::Infer { tag_size } => {
                if let Some(cam) = context.context.request::<Camera>() {
                    let cfg = cam.config();
                    if let Some(fov) = cfg.fov() {
                        apriltag::PoseParams {
                            tag_size,
                            ..apriltag::PoseParams::from_dimensions(
                                cfg.width(),
                                cfg.height(),
                                fov as _,
                            )
                        }
                    } else {
                        tracing::error!(
                            "attempted to infer parameters for a camera without an FOV"
                        );
                        return;
                    }
                } else {
                    tracing::error!("a camera wasn't passed as context!");
                    return;
                }
            }
        };
        let Ok(detection) = context.get_as::<apriltag::Detection>(None).and_log_err() else {
            return;
        };
        context.submit(None, detection.estimate_pose(params));
    }
}
#[typetag::serde(name = "april-pose")]
impl ComponentFactory for DetectPoseComponent {
    fn build(&self, _: &mut dyn ProviderDyn) -> Box<dyn Component> {
        Box::new(self.clone())
    }
}
