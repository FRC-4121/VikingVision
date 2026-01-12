use super::PixelFormat;
#[cfg(feature = "serde")]
use serde::{Deserialize, Serialize};
use std::fmt::{self, Display, Formatter};
use std::ops::{Deref, DerefMut, RangeInclusive};

#[derive(Clone, Copy)]
pub enum ColorBytes {
    One([u8; 1]),
    Two([u8; 2]),
    Three([u8; 3]),
    Four([u8; 4]),
}
impl Deref for ColorBytes {
    type Target = [u8];

    fn deref(&self) -> &Self::Target {
        match self {
            Self::One(s) => s,
            Self::Two(s) => s,
            Self::Three(s) => s,
            Self::Four(s) => s,
        }
    }
}
impl DerefMut for ColorBytes {
    fn deref_mut(&mut self) -> &mut Self::Target {
        match self {
            Self::One(s) => s,
            Self::Two(s) => s,
            Self::Three(s) => s,
            Self::Four(s) => s,
        }
    }
}

/// Create a [`ColorBytes`] with the given values.
#[macro_export]
macro_rules! color_bytes {
    [$b0:expr $(,)?] => {
        $crate::vision::ColorBytes::One([$b0])
    };
    [$b0:expr, $b1:expr  $(,)?] => {
        $crate::vision::ColorBytes::Two([$b0, $b1])
    };
    [$b0:expr, $b1:expr, $b2:expr  $(,)?] => {
        $crate::vision::ColorBytes::Three([$b0, $b1, $b2])
    };
    [$b0:expr, $b1:expr, $b2:expr, $b3:expr  $(,)?] => {
        $crate::vision::ColorBytes::Four([$b0, $b1, $b2, $b3])
    };
}

/// A filter, along with a color space to filter in.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(
    feature = "serde",
    serde(
        rename_all = "lowercase",
        rename_all_fields = "kebab-case",
        tag = "space"
    )
)]
pub enum ColorFilter {
    Luma {
        min_l: u8,
        max_l: u8,
    },
    Rgb {
        min_r: u8,
        min_g: u8,
        min_b: u8,
        max_r: u8,
        max_g: u8,
        max_b: u8,
    },
    Hsv {
        min_h: u8,
        max_h: u8,
        min_s: u8,
        max_s: u8,
        min_v: u8,
        max_v: u8,
    },
    Yuyv {
        min_y: u8,
        max_y: u8,
        min_u: u8,
        max_u: u8,
        min_v: u8,
        max_v: u8,
    },
    #[cfg_attr(feature = "serde", serde(rename = "ycc"))]
    YCbCr {
        min_y: u8,
        max_y: u8,
        min_b: u8,
        max_b: u8,
        min_r: u8,
        max_r: u8,
    },
}
impl ColorFilter {
    pub fn pixel_format(&self) -> PixelFormat {
        match self {
            Self::Luma { .. } => PixelFormat::LUMA,
            Self::Rgb { .. } => PixelFormat::RGB,
            Self::Hsv { .. } => PixelFormat::HSV,
            Self::YCbCr { .. } => PixelFormat::YCC,
            Self::Yuyv { .. } => PixelFormat::YUYV,
        }
    }
    pub fn to_range(self) -> RangeInclusive<Color> {
        match self {
            Self::Luma { min_l, max_l } => Color::Luma { l: min_l }..=Color::Luma { l: max_l },

            Self::Rgb {
                min_r,
                min_g,
                min_b,
                max_r,
                max_g,
                max_b,
            } => {
                Color::Rgb {
                    r: min_r,
                    g: min_g,
                    b: min_b,
                }..=Color::Rgb {
                    r: max_r,
                    g: max_g,
                    b: max_b,
                }
            }

            Self::Hsv {
                min_h,
                max_h,
                min_s,
                max_s,
                min_v,
                max_v,
            } => {
                Color::Hsv {
                    h: min_h,
                    s: min_s,
                    v: min_v,
                }..=Color::Hsv {
                    h: max_h,
                    s: max_s,
                    v: max_v,
                }
            }

            Self::Yuyv {
                min_y,
                max_y,
                min_u,
                max_u,
                min_v,
                max_v,
            } => {
                Color::Yuyv {
                    y: min_y,
                    u: min_u,
                    v: min_v,
                }..=Color::Yuyv {
                    y: max_y,
                    u: max_u,
                    v: max_v,
                }
            }

            Self::YCbCr {
                min_y,
                max_y,
                min_b,
                max_b,
                min_r,
                max_r,
            } => {
                Color::YCbCr {
                    y: min_y,
                    b: min_b,
                    r: min_r,
                }..=Color::YCbCr {
                    y: max_y,
                    b: max_b,
                    r: max_r,
                }
            }
        }
    }
}
impl Display for ColorFilter {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        let (min, max) = self.to_range().into_inner();
        write!(f, "{min}..={max}")
    }
}

/// A color, along with its color space.
#[derive(Debug, Clone, Copy, PartialEq)]
#[cfg_attr(feature = "serde", derive(Serialize, Deserialize))]
#[cfg_attr(feature = "serde", serde(rename_all = "lowercase", tag = "space"))]
pub enum Color {
    Luma {
        l: u8,
    },
    Rgb {
        r: u8,
        g: u8,
        b: u8,
    },
    Hsv {
        h: u8,
        s: u8,
        v: u8,
    },
    Yuyv {
        y: u8,
        u: u8,
        v: u8,
    },
    #[cfg_attr(feature = "serde", serde(rename = "ycc"))]
    YCbCr {
        y: u8,
        b: u8,
        r: u8,
    },
}
impl Color {
    pub fn pixel_format(&self) -> PixelFormat {
        match self {
            Self::Luma { .. } => PixelFormat::LUMA,
            Self::Rgb { .. } => PixelFormat::RGB,
            Self::Hsv { .. } => PixelFormat::HSV,
            Self::YCbCr { .. } => PixelFormat::YCC,
            Self::Yuyv { .. } => PixelFormat::YUYV,
        }
    }
    pub fn bytes(&self) -> ColorBytes {
        match *self {
            Self::Luma { l } => color_bytes![l],
            Self::Rgb { r, g, b } => color_bytes![r, g, b],
            Self::Hsv { h, s, v } => color_bytes![h, s, v],
            Self::Yuyv { y, u, v } => color_bytes![y, u, v],
            Self::YCbCr { y, b, r } => color_bytes![y, b, r],
        }
    }
}
impl Display for Color {
    fn fmt(&self, f: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::Luma { l } => write!(f, "luma({l})"),
            Self::Rgb { r, g, b } => write!(f, "rgb({r}, {g}, {b})"),
            Self::Hsv { h, s, v } => write!(f, "hsv({h}, {s}, {v})"),
            Self::Yuyv { y, u, v } => write!(f, "yuv({y}, {u}, {v})"),
            Self::YCbCr { y, b, r } => write!(f, "ycc({y}, {b}, {r})"),
        }
    }
}
