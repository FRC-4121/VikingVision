use crate::pipeline::graph::GraphComponentId;
use smol_str::SmolStr;

pub mod apriltag;
pub mod collect;
pub mod draw;
pub mod ffmpeg;
pub mod group;
pub mod ntable;
pub mod utils;
pub mod vision;

/// An identifier for a component.
///
/// Loading from a name is likely more useful for serialization, but these components should be easily
/// usable from code, so they can be configured using component IDs, too.
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentIdentifier {
    Name(SmolStr),
    Id(GraphComponentId),
}
impl From<String> for ComponentIdentifier {
    fn from(value: String) -> Self {
        Self::Name(value.into())
    }
}
impl From<SmolStr> for ComponentIdentifier {
    fn from(value: SmolStr) -> Self {
        Self::Name(value)
    }
}
impl From<GraphComponentId> for ComponentIdentifier {
    fn from(value: GraphComponentId) -> Self {
        Self::Id(value)
    }
}

pub mod prelude {
    #[cfg(feature = "apriltag")]
    pub use super::apriltag::{AprilTagComponent, DetectPoseComponent};
    pub use super::collect::{CollectVecComponent, SelectLastComponent};
    pub use super::draw::DrawComponent;
    pub use super::ffmpeg::FfmpegComponent;
    pub use super::group::GroupComponent;
    #[cfg(feature = "ntable")]
    pub use super::ntable::NtPrimitiveComponent;
    pub use super::utils::{
        ChannelComponent, CloneComponent, DebugComponent, FpsComponent, WrapMutexComponent,
    };
    pub use super::vision::{
        BlobComponent, ColorFilterComponent, ColorSpaceComponent, GaussianBlurComponent,
        PercentileFilterComponent,
    };
}
