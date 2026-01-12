use super::*;
use vv_vision::buffer::Buffer;
use vv_vision::draw::Line;
use vv_vision::vision::Blob;

impl Data for Line {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Display::fmt(self, f)
    }
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(*self)
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["x0", "y0", "x1", "y1"]
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        match field {
            "x0" => Some(Cow::Borrowed(&self.x0)),
            "y0" => Some(Cow::Borrowed(&self.y0)),
            "x1" => Some(Cow::Borrowed(&self.x1)),
            "y1" => Some(Cow::Borrowed(&self.y1)),
            _ => None,
        }
    }
}
impl Data for Blob {
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(*self)
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        match field {
            "min_x" => Some(Cow::Borrowed(&self.min_x)),
            "max_x" => Some(Cow::Borrowed(&self.max_x)),
            "min_y" => Some(Cow::Borrowed(&self.min_y)),
            "max_y" => Some(Cow::Borrowed(&self.max_y)),
            "pixels" => Some(Cow::Borrowed(&self.pixels)),
            "width" => Some(Cow::Owned(Arc::new(self.width()) as _)),
            "height" => Some(Cow::Owned(Arc::new(self.height()) as _)),
            "area" => Some(Cow::Owned(Arc::new(self.area()) as _)),
            "filled" => Some(Cow::Owned(Arc::new(self.filled()) as _)),
            _ => None,
        }
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &[
            "min_x", "max_x", "min_y", "max_y", "pixels", "width", "height", "area", "filled",
        ]
    }
}
impl Data for Buffer<'static> {
    fn debug(&self, f: &mut Formatter) -> fmt::Result {
        Debug::fmt(&self, f)
    }
    fn clone_to_arc(&self) -> Arc<dyn Data> {
        Arc::new(self.clone())
    }
    fn field(&self, field: &str) -> Option<Cow<'_, dyn Data>> {
        match field {
            "width" => Some(Cow::Borrowed(&self.width)),
            "height" => Some(Cow::Borrowed(&self.height)),
            "pixels" => Some(Cow::Owned(Arc::new(self.width * self.height) as _)),
            "raw_size" => Some(Cow::Owned(Arc::new(self.data.len()) as _)),
            "format" => Some(Cow::Owned(Arc::new(self.format.to_string()) as _)),
            _ => None,
        }
    }
    fn known_fields(&self) -> &'static [&'static str] {
        &["width", "height", "pixels", "raw_size", "format"]
    }
}
