#![cfg(feature = "vision")]

use crate::pipeline::prelude::*;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::marker::PhantomData;
use vv_utils::mutex::Mutex;
use vv_vision::buffer::Buffer;
use vv_vision::draw::*;
#[cfg(feature = "serde")]
use vv_vision::vision::Blob;
use vv_vision::vision::Color;

pub struct DrawComponent<T> {
    pub color: Color,
    _marker: PhantomData<fn(T)>,
}
impl<T: Drawable + Data> DrawComponent<T> {
    pub const fn new(color: Color) -> Self {
        Self {
            color,
            _marker: PhantomData,
        }
    }
    pub fn new_boxed(color: Color) -> Box<dyn Component> {
        Box::new(Self::new(color))
    }
}
impl<T: Drawable + Data> Component for DrawComponent<T> {
    fn inputs(&self) -> Inputs {
        Inputs::named(["canvas", "elem"])
    }
    fn output_kind(&self, name: &str) -> OutputKind {
        if name == "echo" {
            OutputKind::Single
        } else {
            OutputKind::None
        }
    }
    fn run<'s, 'r: 's>(&self, context: ComponentContext<'_, 's, 'r>) {
        let Ok(canvas) = context.get_as::<Mutex<Buffer>>("canvas").and_log_err() else {
            return;
        };
        let Ok(elem) = context.get_as::<T>("elem").and_log_err() else {
            return;
        };
        {
            let Ok(mut lock) = canvas.lock() else {
                tracing::error!("attempted to lock poisoned mutex");
                return;
            };
            let fmt = self.color.pixel_format();
            lock.convert_inplace(fmt);
            elem.draw(&self.color.bytes(), &mut lock);
        }
        context.submit("echo", canvas);
    }
}

#[derive(Debug, Clone)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(try_from = "DrawShim"))]
pub struct DrawFactory {
    /// The type of things to draw.
    ///
    /// Currently supported types are:
    /// - [`Blob`] as `blob`
    /// - [`Line`] as `line`
    /// - [`apriltag::Detection`](crate::apriltag::Detection) as `apriltag`
    /// - a [`Vec`] of any of the previous types, as the previous wrapped in brackets e.g. `[blob]` for `Vec<Blob>`
    pub draw: String,
    /// The color to draw in.
    ///
    /// The image will be converted to the specified colorspace first.
    #[cfg_attr(feature = "serde", serde(flatten))]
    pub color: Color,
    /// The actual construction function.
    ///
    /// This is skipped in de/serialization, and looked up based on the type name
    #[cfg_attr(feature = "serde", serde(skip))]
    pub factory: fn(Color) -> Box<dyn Component>,
}
#[cfg_attr(feature = "serde", typetag::serde(name = "draw"))]
impl ComponentFactory for DrawFactory {
    fn build(&self) -> Box<dyn Component> {
        (self.factory)(self.color)
    }
}

#[cfg(feature = "serde")]
#[derive(Deserialize)]
struct DrawShim {
    draw: String,
    #[serde(flatten)]
    color: Color,
}
#[cfg(feature = "serde")]
impl TryFrom<DrawShim> for DrawFactory {
    type Error = String;

    fn try_from(value: DrawShim) -> Result<Self, Self::Error> {
        let factory = match &*value.draw {
            "blob" => DrawComponent::<Blob>::new_boxed,
            "line" => DrawComponent::<Line>::new_boxed,
            #[cfg(feature = "apriltag")]
            "apriltag" => DrawComponent::<vv_apriltag::Detection>::new_boxed,
            "[blob]" => DrawComponent::<Vec<Blob>>::new_boxed,
            "[line]" => DrawComponent::<Vec<Line>>::new_boxed,
            #[cfg(feature = "apriltag")]
            "[apriltag]" => DrawComponent::<Vec<vv_apriltag::Detection>>::new_boxed,
            name => return Err(format!("Unrecognized type {name:?}")),
        };
        Ok(DrawFactory {
            draw: value.draw,
            color: value.color,
            factory,
        })
    }
}
