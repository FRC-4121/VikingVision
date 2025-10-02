use crate::pipeline::graph::GraphComponentId;

pub mod apriltag;
pub mod draw;
pub mod ffmpeg;
pub mod group;
pub mod utils;
pub mod vision;

/// An identifier for a component.
///
/// Loading from a name is likely more useful for serialization, but these components should be easily
/// usable from code, so they can be configured using component IDs, too.
#[derive(Debug, Clone, PartialEq)]
pub enum ComponentIdentifier {
    Name(String),
    Id(GraphComponentId),
}
impl From<String> for ComponentIdentifier {
    fn from(value: String) -> Self {
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
    pub use super::draw::DrawComponent;
    pub use super::ffmpeg::FfmpegComponent;
    pub use super::group::GroupComponent;
    pub use super::utils::{
        ChannelComponent, CloneComponent, DebugComponent, FpsComponent, WrapMutexComponent,
    };
    pub use super::vision::{
        BlobComponent, ColorFilterComponent, ColorSpaceComponent, PercentileFilterComponent,
    };
}
