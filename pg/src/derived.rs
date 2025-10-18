use crate::range_slider::RangeSlider;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "apriltag")]
use viking_vision::apriltag;
use viking_vision::buffer::*;
use viking_vision::draw::*;
use viking_vision::vision::*;

static UNIQUE_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Serialize, Deserialize)]
pub enum Transform {
    ColorFilter(ColorFilter),
    BoxBlur {
        format: PixelFormat,
        width: usize,
        height: usize,
    },
    PercentileFilter {
        format: PixelFormat,
        width: usize,
        height: usize,
        pixel: usize,
    },
    Blobs {
        min_width: u32,
        min_height: u32,
        min_area: u64,
        min_fill: f64,
    },
    #[cfg(feature = "apriltag")]
    Apriltag {
        families: Vec<apriltag::TagFamilyWithBits>,
        max_threads: u8,
        sigma: f32,
        decimate: f32,
        changed: bool,
    },
}

#[derive(Serialize, Deserialize)]
pub struct DerivedFrame {
    transform: Transform,
    derived: Vec<DerivedFrame>,
    title: String,
    id: egui::Id,
    #[serde(skip)]
    frame: Buffer<'static>,
    #[serde(skip)]
    rgb: Buffer<'static>,
    #[cfg(feature = "apriltag")]
    #[serde(skip)]
    detector: Option<apriltag::Detector>,
    #[cfg(feature = "apriltag")]
    #[serde(skip)]
    last_families: Vec<apriltag::TagFamily>,
}
impl DerivedFrame {
    fn new(transform: Transform, id: egui::Id) -> Self {
        Self {
            transform,
            derived: Vec::new(),
            title: String::new(),
            id: id.with(UNIQUE_COUNTER.fetch_add(1, Ordering::Relaxed)),
            frame: Buffer::empty_rgb(),
            rgb: Buffer::empty_rgb(),
            #[cfg(feature = "apriltag")]
            detector: None,
            #[cfg(feature = "apriltag")]
            last_families: Vec::new(),
        }
    }
    pub fn update_frame(&mut self, mut from: Buffer<'_>, original: Buffer<'_>) {
        match self.transform {
            Transform::ColorFilter(filter) => filter_into(from, &mut self.frame, filter),
            Transform::BoxBlur {
                format,
                width,
                height,
            } => {
                tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::new(), || {
                    from.convert_inplace(format);
                });
                box_blur(from, &mut self.frame, width * 2 + 1, height * 2 + 1);
            }
            Transform::PercentileFilter {
                format,
                width,
                height,
                pixel,
            } => {
                tracing::subscriber::with_default(tracing::subscriber::NoSubscriber::new(), || {
                    from.convert_inplace(format);
                });
                let w = width * 2 + 1;
                let h = height * 2 + 1;
                percentile_filter(from, &mut self.frame, w, h, pixel.min(w * h - 1));
            }
            Transform::Blobs {
                min_width,
                min_height,
                min_area,
                min_fill,
            } => {
                if original.format == PixelFormat::Yuyv {
                    original.convert_into(&mut self.frame);
                } else {
                    self.frame.copy_from(original.borrow());
                }
                let sz = from.format.pixel_size() as usize;
                let it = BlobsIterator::new(
                    from.data
                        .chunks(sz * from.width as usize)
                        .map(|row| row.chunks(sz).map(|px| px.iter().any(|v| *v != 0))),
                );
                for blob in it {
                    if blob.width() < min_width
                        || blob.height() < min_height
                        || blob.area() < min_area
                        || blob.filled() < min_fill
                    {
                        continue;
                    }
                    blob.draw(self.frame.format.bright_color(), &mut self.frame);
                }
            }
            #[cfg(feature = "apriltag")]
            Transform::Apriltag {
                ref families,
                max_threads,
                sigma,
                decimate,
                ref mut changed,
            } => {
                if original.format == PixelFormat::Yuyv {
                    original.convert_into(&mut self.frame);
                } else {
                    self.frame.copy_from(original.borrow());
                }
                let detector = self.detector.get_or_insert_with(|| {
                    *changed = true;
                    apriltag::Detector::new()
                });
                if *changed {
                    *changed = false;
                    detector.set_max_threads(max_threads);
                    detector.set_sigma(sigma);
                    detector.set_decimate(decimate);
                    for family in self.last_families.drain(..) {
                        detector.remove_family(family);
                    }
                    for &family in families {
                        detector.add_family(family);
                    }
                    self.last_families.extend(families.iter().map(|f| f.family));
                }
                for detection in detector.detect(from) {
                    detection.draw(self.frame.format.bright_color(), &mut self.frame);
                }
            }
        }
        self.frame.convert_into(&mut self.rgb);
        for next in &mut self.derived {
            next.update_frame(self.frame.borrow(), original.borrow());
        }
    }
    fn update_title(&mut self, prev: &str) {
        use std::fmt::Write;
        self.title.clear();
        self.title.push_str(prev);
        self.title.push_str(" > ");
        match self.transform {
            Transform::ColorFilter(filter) => {
                let _ = write!(self.title, "Color Filter: {filter}");
            }
            Transform::BoxBlur {
                format,
                width,
                height,
            } => {
                let w = width * 2 + 1;
                let h = height * 2 + 1;
                let _ = write!(self.title, "Box Blur: {format}, {w}x{h}");
            }
            Transform::PercentileFilter {
                format,
                width,
                height,
                pixel,
            } => {
                let w = width * 2 + 1;
                let h = height * 2 + 1;
                let p = pixel * 100 / w / h;
                let _ = write!(self.title, "Percentile Filter: {format}, {w}x{h}, {p}%");
            }
            Transform::Blobs { .. } => {
                self.title.push_str("Blobs");
            }
            #[cfg(feature = "apriltag")]
            Transform::Apriltag {
                ref families,
                sigma,
                decimate,
                ..
            } => {
                let _ = write!(
                    self.title,
                    "Apriltag: sigma: {sigma}, decimate: {decimate}, "
                );
                let mut families = families.as_slice();
                if let Some(first) = families.split_off_first() {
                    let _ = write!(
                        self.title,
                        "families: {} ({} bits)",
                        first.family, first.bits
                    );
                    for family in families {
                        let _ = write!(self.title, ", {} ({} bits)", family.family, family.bits);
                    }
                } else {
                    self.title.push_str("no families");
                }
            }
        }
        for next in &mut self.derived {
            next.update_title("...");
        }
    }
    fn with_updated_title(mut self, prev: &str) -> Self {
        self.update_title(prev);
        self
    }
}

pub fn add_button(ui: &mut egui::Ui, title: &str, id: egui::Id, next: &mut Vec<DerivedFrame>) {
    ui.menu_button("Add derived", |ui| {
        if ui.button("Color Filter").clicked() {
            next.push(
                DerivedFrame::new(
                    Transform::ColorFilter(ColorFilter::Luma {
                        min_l: 0,
                        max_l: 255,
                    }),
                    id,
                )
                .with_updated_title(title),
            );
        }
        if ui.button("Box Blur").clicked() {
            next.push(
                DerivedFrame::new(
                    Transform::BoxBlur {
                        format: PixelFormat::Rgb,
                        width: 0,
                        height: 0,
                    },
                    id,
                )
                .with_updated_title(title),
            );
        }
        if ui.button("Percentile Filter").clicked() {
            next.push(
                DerivedFrame::new(
                    Transform::PercentileFilter {
                        format: PixelFormat::Rgb,
                        width: 0,
                        height: 0,
                        pixel: 0,
                    },
                    id,
                )
                .with_updated_title(title),
            );
        }
        if ui.button("Blobs").clicked() {
            next.push(
                DerivedFrame::new(
                    Transform::Blobs {
                        min_width: 50,
                        min_height: 50,
                        min_area: 0,
                        min_fill: 0.0,
                    },
                    id,
                )
                .with_updated_title(title),
            );
        }
        #[cfg(feature = "apriltag")]
        if ui.button("Apriltag").clicked() {
            next.push(
                DerivedFrame::new(
                    Transform::Apriltag {
                        families: Vec::new(),
                        max_threads: 1,
                        sigma: 2.0,
                        decimate: 0.0,
                        changed: false,
                    },
                    id,
                )
                .with_updated_title(title),
            );
        }
    });
}
pub fn render_frame(ctx: &egui::Context, prev: &str) -> impl Fn(&mut DerivedFrame) -> bool {
    move |frame| {
        let mut show = true;
        egui::Window::new(&frame.title)
            .id(frame.id)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    add_button(ui, "...", frame.id, &mut frame.derived);
                    if ui.button("Delete").clicked() {
                        show = false;
                    }
                });
                let mut changed = false;
                match &mut frame.transform {
                    #[allow(unused_variables)]
                    Transform::ColorFilter(filter) => {
                        let mut space = filter.pixel_format();
                        changed = color_space_dropdown(ui, 2, &mut space);
                        if changed {
                            *filter = match space {
                                PixelFormat::Luma => ColorFilter::Luma {
                                    min_l: 0,
                                    max_l: 255,
                                },
                                PixelFormat::LumaA => ColorFilter::LumaA {
                                    min_l: 0,
                                    max_l: 255,
                                    min_a: 0,
                                    max_a: 255,
                                },
                                PixelFormat::Gray => ColorFilter::Gray {
                                    min_v: 0,
                                    max_v: 255,
                                },
                                PixelFormat::GrayA => ColorFilter::GrayA {
                                    min_v: 0,
                                    max_v: 255,
                                    min_a: 0,
                                    max_a: 255,
                                },
                                PixelFormat::Rgb => ColorFilter::Rgb {
                                    min_r: 0,
                                    min_g: 0,
                                    min_b: 0,
                                    max_r: 255,
                                    max_g: 255,
                                    max_b: 255,
                                },
                                PixelFormat::Rgba => ColorFilter::Rgba {
                                    min_r: 0,
                                    min_g: 0,
                                    min_b: 0,
                                    max_r: 255,
                                    max_g: 255,
                                    max_b: 255,
                                    min_a: 0,
                                    max_a: 255,
                                },
                                PixelFormat::Hsv => ColorFilter::Hsv {
                                    min_h: 0,
                                    max_h: 255,
                                    min_s: 0,
                                    max_s: 255,
                                    min_v: 0,
                                    max_v: 255,
                                },
                                PixelFormat::Hsva => ColorFilter::Hsva {
                                    min_h: 0,
                                    max_h: 255,
                                    min_s: 0,
                                    max_s: 255,
                                    min_v: 0,
                                    max_v: 255,
                                    min_a: 0,
                                    max_a: 255,
                                },
                                PixelFormat::Yuyv => ColorFilter::Yuyv {
                                    min_y: 0,
                                    max_y: 255,
                                    min_u: 0,
                                    max_u: 255,
                                    min_v: 0,
                                    max_v: 255,
                                },
                                PixelFormat::YCbCr => ColorFilter::YCbCr {
                                    min_y: 0,
                                    max_y: 255,
                                    min_b: 0,
                                    max_b: 255,
                                    min_r: 0,
                                    max_r: 255,
                                },
                                PixelFormat::YCbCrA => ColorFilter::YCbCrA {
                                    min_y: 0,
                                    max_y: 255,
                                    min_b: 0,
                                    max_b: 255,
                                    min_r: 0,
                                    max_r: 255,
                                    min_a: 0,
                                    max_a: 255,
                                },
                            };
                        }
                        match filter {
                            ColorFilter::Luma { min_l, max_l } => {
                                changed |= ui.add(RangeSlider::new("L", min_l, max_l)).changed();
                            }
                            ColorFilter::LumaA {
                                min_l,
                                max_l,
                                min_a,
                                max_a,
                            } => {
                                changed |= ui.add(RangeSlider::new("L", min_l, max_l)).changed();
                                changed |= ui.add(RangeSlider::new("A", min_a, max_a)).changed();
                            }
                            ColorFilter::Gray { min_v, max_v } => {
                                changed |= ui.add(RangeSlider::new("V", min_v, max_v)).changed();
                            }
                            ColorFilter::GrayA {
                                min_v,
                                max_v,
                                min_a,
                                max_a,
                            } => {
                                changed |= ui.add(RangeSlider::new("V", min_v, max_v)).changed();
                                changed |= ui.add(RangeSlider::new("A", min_a, max_a)).changed();
                            }
                            ColorFilter::Rgb {
                                min_r,
                                min_g,
                                min_b,
                                max_r,
                                max_g,
                                max_b,
                            } => {
                                changed |= ui.add(RangeSlider::new("R", min_r, max_r)).changed();
                                changed |= ui.add(RangeSlider::new("G", min_g, max_g)).changed();
                                changed |= ui.add(RangeSlider::new("B", min_b, max_b)).changed();
                            }
                            ColorFilter::Rgba {
                                min_r,
                                min_g,
                                min_b,
                                max_r,
                                max_g,
                                max_b,
                                min_a,
                                max_a,
                            } => {
                                changed |= ui.add(RangeSlider::new("R", min_r, max_r)).changed();
                                changed |= ui.add(RangeSlider::new("G", min_g, max_g)).changed();
                                changed |= ui.add(RangeSlider::new("B", min_b, max_b)).changed();
                                changed |= ui.add(RangeSlider::new("A", min_a, max_a)).changed();
                            }
                            ColorFilter::Hsv {
                                min_h,
                                max_h,
                                min_s,
                                max_s,
                                min_v,
                                max_v,
                            } => {
                                changed |= ui.add(RangeSlider::new("H", min_h, max_h)).changed();
                                changed |= ui.add(RangeSlider::new("S", min_s, max_s)).changed();
                                changed |= ui.add(RangeSlider::new("V", min_v, max_v)).changed();
                            }
                            ColorFilter::Hsva {
                                min_h,
                                max_h,
                                min_s,
                                max_s,
                                min_v,
                                max_v,
                                min_a,
                                max_a,
                            } => {
                                changed |= ui.add(RangeSlider::new("H", min_h, max_h)).changed();
                                changed |= ui.add(RangeSlider::new("S", min_s, max_s)).changed();
                                changed |= ui.add(RangeSlider::new("V", min_v, max_v)).changed();
                                changed |= ui.add(RangeSlider::new("A", min_a, max_a)).changed();
                            }
                            ColorFilter::Yuyv {
                                min_y,
                                max_y,
                                min_u,
                                max_u,
                                min_v,
                                max_v,
                            } => {
                                changed |= ui.add(RangeSlider::new("Y", min_y, max_y)).changed();
                                changed |= ui.add(RangeSlider::new("U", min_u, max_u)).changed();
                                changed |= ui.add(RangeSlider::new("V", min_v, max_v)).changed();
                            }
                            ColorFilter::YCbCr {
                                min_y,
                                max_y,
                                min_b,
                                max_b,
                                min_r,
                                max_r,
                            } => {
                                changed |= ui.add(RangeSlider::new("Y", min_y, max_y)).changed();
                                changed |= ui.add(RangeSlider::new("Cb", min_b, max_b)).changed();
                                changed |= ui.add(RangeSlider::new("Cr", min_r, max_r)).changed();
                            }
                            ColorFilter::YCbCrA {
                                min_y,
                                max_y,
                                min_b,
                                max_b,
                                min_r,
                                max_r,
                                min_a,
                                max_a,
                            } => {
                                changed |= ui.add(RangeSlider::new("Y", min_y, max_y)).changed();
                                changed |= ui.add(RangeSlider::new("Cb", min_b, max_b)).changed();
                                changed |= ui.add(RangeSlider::new("Cr", min_r, max_r)).changed();
                                changed |= ui.add(RangeSlider::new("A", min_a, max_a)).changed();
                            }
                        }
                    }
                    Transform::BoxBlur {
                        format,
                        width,
                        height,
                    } => {
                        changed = color_space_dropdown(ui, 0, format);
                        changed |= ui
                            .add(
                                egui::Slider::from_get_set(1.0..=13.0, |old| {
                                    if let Some(v) = old {
                                        *width = (v * 0.5).max(0.0) as _;
                                    }
                                    (*width * 2 + 1) as _
                                })
                                .integer()
                                .text("Width"),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::Slider::from_get_set(1.0..=13.0, |old| {
                                    if let Some(v) = old {
                                        *height = (v * 0.5).max(0.0) as _;
                                    }
                                    (*height * 2 + 1) as _
                                })
                                .integer()
                                .text("Height"),
                            )
                            .changed();
                    }
                    Transform::PercentileFilter {
                        format,
                        width,
                        height,
                        pixel,
                    } => {
                        changed = color_space_dropdown(ui, 1, format);
                        changed |= ui
                            .add(
                                egui::Slider::from_get_set(1.0..=13.0, |old| {
                                    if let Some(v) = old {
                                        *width = (v * 0.5).max(0.0) as _;
                                    }
                                    (*width * 2 + 1) as _
                                })
                                .integer()
                                .text("Width"),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::Slider::from_get_set(1.0..=13.0, |old| {
                                    if let Some(v) = old {
                                        *height = (v * 0.5).max(0.0) as _;
                                    }
                                    (*height * 2 + 1) as _
                                })
                                .integer()
                                .text("Height"),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::Slider::new(
                                    pixel,
                                    0..=((*width * 2 + 1) * (*height * 2 + 1) - 1),
                                )
                                .clamping(egui::SliderClamping::Always)
                                .text("Selected"),
                            )
                            .changed();
                    }
                    Transform::Blobs {
                        min_width,
                        min_height,
                        min_area,
                        min_fill,
                    } => {
                        changed |= ui
                            .add(
                                egui::Slider::new(min_width, 0..=100)
                                    .clamping(egui::SliderClamping::Never)
                                    .text("Min Width"),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::Slider::new(min_height, 0..=100)
                                    .clamping(egui::SliderClamping::Never)
                                    .text("Min Height"),
                            )
                            .changed();
                        changed |= ui
                            .add(
                                egui::Slider::new(min_area, 0..=10000)
                                    .clamping(egui::SliderClamping::Never)
                                    .text("Min Area"),
                            )
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(min_fill, 0.0..=1.0).text("Min Fill"))
                            .changed();
                    }
                    #[cfg(feature = "apriltag")]
                    Transform::Apriltag {
                        #[allow(unused_variables)]
                        families,
                        max_threads,
                        sigma,
                        decimate,
                        changed: at_changed,
                    } => {
                        changed |= ui
                            .add(
                                egui::Slider::new(
                                    max_threads,
                                    1..=(std::thread::available_parallelism()
                                        .map_or(1, std::num::NonZero::get)
                                        as u8
                                        * 3
                                        / 4),
                                )
                                .text("Threads"),
                            )
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(sigma, 0.0..=10.0).text("Sigma"))
                            .changed();
                        changed |= ui
                            .add(egui::Slider::new(decimate, 0.0..=10.0).text("Decimate"))
                            .changed();
                        *at_changed = changed;
                    }
                }
                if changed {
                    frame.update_title(prev);
                }
                let img = egui::ColorImage::from_rgb(
                    [frame.rgb.width as _, frame.rgb.height as _],
                    &frame.rgb.data,
                );
                let texture =
                    ui.ctx()
                        .load_texture(frame.id.short_debug_format(), img, Default::default());
                ui.image(&texture);
            });
        frame.derived.retain_mut(render_frame(ctx, "..."));
        show
    }
}
fn color_space_dropdown(ui: &mut egui::Ui, id_salt: u64, space: &mut PixelFormat) -> bool {
    let mut idx = *space as usize;
    let old = idx;
    egui::ComboBox::new(id_salt, "Color Space")
        .selected_text(space.to_string())
        .show_index(ui, &mut idx, PixelFormat::VARIANTS.len(), |idx| {
            PixelFormat::VARIANTS[idx].to_string()
        });
    *space = PixelFormat::VARIANTS[idx];
    idx != old
}
