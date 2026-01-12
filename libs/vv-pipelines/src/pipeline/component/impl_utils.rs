use super::*;
use vv_utils::geom::*;

impl Data for Vec3 {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(*self)
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["x", "y", "z"]
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        match field {
            "x" => Some(Cow::Borrowed(&self.0[0])),
            "y" => Some(Cow::Borrowed(&self.0[1])),
            "z" => Some(Cow::Borrowed(&self.0[2])),
            _ => None,
        }
    }
}
impl Data for Mat3 {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(*self)
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["quat", "euler"]
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        match field {
            "quat" => Some(Cow::Owned(Arc::new(self.to_quat()) as _)),
            "euler" => Some(Cow::Owned(Arc::new(self.to_euler()) as _)),
            _ => None,
        }
    }
}
impl Data for Quat {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(*self)
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["x", "y", "z", "w"]
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        match field {
            "x" => Some(Cow::Borrowed(&self.0[0])),
            "y" => Some(Cow::Borrowed(&self.0[1])),
            "z" => Some(Cow::Borrowed(&self.0[2])),
            "w" => Some(Cow::Borrowed(&self.0[3])),
            _ => None,
        }
    }
}
impl Data for EulerXYZ {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(self, f)
    }
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(*self)
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["x", "y", "z"]
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        match field {
            "x" => Some(Cow::Borrowed(&self.0[0])),
            "y" => Some(Cow::Borrowed(&self.0[1])),
            "z" => Some(Cow::Borrowed(&self.0[2])),
            _ => None,
        }
    }
}
