use crate::camera::config::CameraConfig;
use crate::pipeline::serialized::{ComponentChannel, SerializedGraph};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

fn default_running() -> usize {
    rayon::current_num_threads().div_ceil(2)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunConfig {
    #[serde(default = "default_running")]
    pub max_running: usize,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            max_running: default_running(),
        }
    }
}

#[derive(Serialize, Deserialize)]
pub struct CameraWithOutputs {
    #[serde(flatten)]
    pub camera: Box<dyn CameraConfig>,
    pub output: Option<ComponentChannel>,
    #[serde(default)]
    pub outputs: Vec<ComponentChannel>,
}

#[derive(Serialize, Deserialize)]
pub struct ConfigFile {
    #[serde(default)]
    pub config: RunConfig,
    #[serde(alias = "camera")]
    pub cameras: HashMap<String, CameraWithOutputs>,
    #[serde(alias = "component")]
    pub components: SerializedGraph,
}
