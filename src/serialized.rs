#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
#[cfg(feature = "ntable")]
use std::time::Duration;
use vv_camera::CameraConfig;
use vv_pipelines::pipeline::serialized::{ComponentChannel, SerializedGraph};
use vv_vision::vision_debug::DefaultDebug;

fn default_running() -> usize {
    rayon::current_num_threads().div_ceil(2)
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct RunConfig {
    #[cfg_attr(feature = "serde", serde(default = "default_running"))]
    pub max_running: usize,
    pub num_threads: Option<usize>,
}

impl Default for RunConfig {
    fn default() -> Self {
        Self {
            max_running: default_running(),
            num_threads: None,
        }
    }
}

#[cfg(feature = "ntable")]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase"))]
pub enum NtHost {
    Host(String),
    Team(vv_ntable::team::TeamNumber),
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

#[cfg(all(feature = "serde", feature = "ntable"))]
fn default_port() -> u16 {
    5810
}
#[cfg(all(feature = "serde", feature = "ntable"))]
fn default_duration() -> Duration {
    Duration::from_millis(500)
}

/// Configuration for the network table client.
///
/// If the feature is disabled, this is still available, but as a stub.
#[cfg(feature = "ntable")]
#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct NtConfig {
    pub identity: String,
    #[cfg_attr(feature = "serde", serde(flatten))]
    pub host: NtHost,
    #[cfg_attr(feature = "serde", serde(default = "default_port"))]
    pub port: u16,
    #[cfg_attr(
        feature = "serde",
        serde(default = "default_duration", with = "humantime_serde")
    )]
    pub keepalive: Duration,
}

/// Configuration for the network table client.
///
/// If the feature is disabled, this is still available, but as a stub.
#[cfg(not(feature = "ntable"))]
#[derive(Debug, Clone)]
pub struct NtConfig;

#[cfg(all(feature = "serde", not(feature = "ntable")))]
impl Serialize for NtConfig {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: serde::Serializer,
    {
        serializer.serialize_struct("NtConfig", 0)?.end()
    }
}
#[cfg(all(feature = "serde", not(feature = "ntable")))]
impl<'de> Deserialize<'de> for NtConfig {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: serde::Deserializer<'de>,
    {
        serde::de::IgnoredAny::deserialize(deserializer).map(|_| NtConfig)
    }
}

impl NtConfig {
    /// Initialize a client and store it in the global handle.
    ///
    /// If the feature is disabled, this always returns [`NtInitResult::SpawnFailed`].
    #[cfg(feature = "ntable")]
    pub fn init(self) -> NtInitResult {
        let _guard = tracing::error_span!("init").entered();
        let mut client = vv_ntable::NtClient::new(self.identity);
        client.port = self.port;
        client.keepalive = self.keepalive;
        let mut set = false;
        vv_ntable::GLOBAL_HANDLE.get_or_init(|| {
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

#[derive(Debug)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct CameraWithOutputs {
    #[cfg_attr(feature = "serde", serde(flatten))]
    pub camera: CameraConfig,
    pub output: Option<ComponentChannel>,
    #[cfg_attr(feature = "serde", serde(default))]
    pub outputs: Vec<ComponentChannel>,
}

#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
pub struct ConfigFile {
    #[cfg_attr(feature = "serde", serde(default))]
    pub run: RunConfig,
    #[cfg_attr(feature = "serde", serde(default))]
    pub debug: DefaultDebug,
    #[cfg_attr(all(feature = "serde", not(feature = "ntable")), serde(skip))]
    pub ntable: Option<NtConfig>,
    #[cfg_attr(feature = "serde", serde(alias = "camera"))]
    pub cameras: HashMap<String, CameraWithOutputs>,
    #[cfg_attr(feature = "serde", serde(alias = "component"))]
    pub components: SerializedGraph,
}
