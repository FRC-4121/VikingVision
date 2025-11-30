use crate::camera::config::CameraConfig;
use crate::pipeline::serialized::{ComponentChannel, SerializedGraph};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::time::Duration;

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

#[cfg(feature = "ntable")]
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum NtHost {
    Host(String),
    Team(ntable::team::TeamNumber),
}
#[cfg(feature = "ntable")]
impl std::fmt::Display for NtHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Host(host) => f.write_str(host),
            Self::Team(team) => std::fmt::Display::fmt(&team.to_ipv4(), f),
        }
    }
}

fn default_port() -> u16 {
    5810
}
fn default_duration() -> Duration {
    Duration::from_millis(500)
}

/// Configuration for the network table client.
///
/// If the feature is disabled, this is still available, but as a stub.
#[cfg(feature = "ntable")]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NtConfig {
    pub identity: String,
    #[serde(flatten)]
    pub host: NtHost,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_duration", with = "humantime_serde")]
    pub keepalive: Duration,
}

/// Configuration for the network table client.
///
/// If the feature is disabled, this is still available, but as a stub.
#[cfg(not(feature = "ntable"))]
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NtConfig;

impl NtConfig {
    /// Initialize a client and store it in the global handle.
    ///
    /// If the feature is disabled, this always returns [`NtInitResult::SpawnFailed`].
    #[cfg(feature = "ntable")]
    pub fn init(self) -> NtInitResult {
        let _guard = tracing::error_span!("init").entered();
        let mut client = ntable::NtClient::new(self.identity);
        client.port = self.port;
        client.keepalive = self.keepalive;
        let mut set = false;
        ntable::GLOBAL_HANDLE.get_or_init(|| {
            set = true;
            client.handle().clone()
        });
        if !set {
            tracing::warn!("attempted to globally initialize a handle after one was already set");
            return NtInitResult::AlreadySet;
        }
        if let Err(err) = client.try_spawn(self.host) {
            tracing::error!(%err, "failed to spawn client thread");
            NtInitResult::SpawnFailed
        } else {
            NtInitResult::Good
        }
    }
    /// Initialize a client and store it in the global handle.
    ///
    /// If the feature is disabled, this always returns [`NtInitResult::SpawnFailed`].
    #[cfg(not(feature = "ntable"))]
    pub fn init(self) -> NtInitResult {
        tracing::error!(parent: tracing::error_span!("init"), "attempted to spawn a network table client, but that feature is not enabled");
        NtInitResult::SpawnFailed
    }
}

/// Result of [`NtConfig::init`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum NtInitResult {
    /// We created a client but it didn't spawn correctly.
    SpawnFailed,
    /// A global client handle was already set.
    AlreadySet,
    /// Everything went well!
    Good,
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
    #[cfg_attr(not(feature = "ntable"), serde(skip))]
    pub ntable: Option<NtConfig>,
    #[serde(alias = "camera")]
    pub cameras: HashMap<String, CameraWithOutputs>,
    #[serde(alias = "component")]
    pub components: SerializedGraph,
}
