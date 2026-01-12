use super::*;
use vv_apriltag::{Detection, Pose, PoseEstimation};

impl Data for Pose {
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(*self)
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        match field {
            "translation" => Some(Cow::Borrowed(&self.translation)),
            "rotation" => Some(Cow::Borrowed(&self.rotation)),
            _ => None,
        }
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["translation", "rotation"]
    }
}

impl Data for PoseEstimation {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        f.debug_struct("PoseEstimation")
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(*self)
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        match field {
            "pose" => Some(Cow::Borrowed(&self.pose)),
            "error" => Some(Cow::Borrowed(&self.error)),
            "translation" => Some(Cow::Borrowed(&self.pose.translation)),
            "rotation" => Some(Cow::Borrowed(&self.pose.rotation)),
            _ => None,
        }
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["pose", "error", "translation", "rotation"]
    }
}
impl Data for Detection {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(self.clone())
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        unsafe {
            match field {
                "id" => Some(Cow::Borrowed(&(*self.as_ptr()).id)),
                "hamming" => Some(Cow::Borrowed(&(*self.as_ptr()).hamming)),
                "corners" => Some(Cow::Owned(
                    Arc::new(self.corners().as_flattened().to_vec()) as _
                )),
                "center" => Some(Cow::Owned(Arc::new(self.center().to_vec()) as _)),
                "cx" => Some(Cow::Borrowed(&(*self.as_ptr()).c[0])),
                "cy" => Some(Cow::Borrowed(&(*self.as_ptr()).c[1])),
                "p0x" => Some(Cow::Borrowed(&(*self.as_ptr()).p[0][0])),
                "p0y" => Some(Cow::Borrowed(&(*self.as_ptr()).p[0][1])),
                "p1x" => Some(Cow::Borrowed(&(*self.as_ptr()).p[1][0])),
                "p1y" => Some(Cow::Borrowed(&(*self.as_ptr()).p[1][1])),
                "p2x" => Some(Cow::Borrowed(&(*self.as_ptr()).p[2][0])),
                "p2y" => Some(Cow::Borrowed(&(*self.as_ptr()).p[2][1])),
                "p3x" => Some(Cow::Borrowed(&(*self.as_ptr()).p[3][0])),
                "p3y" => Some(Cow::Borrowed(&(*self.as_ptr()).p[3][1])),
                "homography" => Some(Cow::Owned(Arc::new(self.homography().to_vec()) as _)),
                _ => None,
            }
        }
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &[
            "id",
            "hamming",
            "corners",
            "center",
            "cx",
            "cy",
            "p0x",
            "p0y",
            "p1x",
            "p1y",
            "p2x",
            "p2y",
            "p3x",
            "p3y",
            "homography",
        ]
    }
}
