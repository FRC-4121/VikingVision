use crate::range_slider::RangeSlider;
use eframe::egui;
use serde::{Deserialize, Serialize};
use std::num::NonZero;
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(feature = "apriltag")]
use viking_vision::apriltag;
use viking_vision::broadcast::par_broadcast2;
use viking_vision::buffer::*;
use viking_vision::draw::*;
use viking_vision::vision::*;

static UNIQUE_COUNTER: AtomicUsize = AtomicUsize::new(0);

#[derive(Serialize, Deserialize)]
enum FilterKind {
    Space(ColorFilter),
    Anon { min: Vec<u8>, max: Vec<u8> },
}

#[derive(Serialize, Deserialize)]
enum Transform {
    ColorSpace(PixelFormat),
    ColorFilter(FilterKind),
    Swizzle(Vec<u8>),
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
            Transform::ColorSpace(space) => {
                self.frame.format = space;
                from.convert_into(&mut self.frame);
            }
            Transform::ColorFilter(FilterKind::Space(filter)) => {
                color_filter(from, &mut self.frame, filter);
            }
            Transform::ColorFilter(FilterKind::Anon { ref min, ref max }) => {
                self.frame.width = from.width;
                self.frame.height = from.height;
                self.frame.format = PixelFormat::LUMA;
                par_broadcast2(FilterPixel::new(min, max), &from, self.frame.resize_data());
            }
            Transform::Swizzle(ref s) => {
                swizzle(from, &mut self.frame, s);
            }
            Transform::BoxBlur {
                format,
                width,
                height,
            } => {
                from.convert_inplace(format);
                box_blur(from, &mut self.frame, width * 2 + 1, height * 2 + 1);
            }
            Transform::PercentileFilter {
                format,
                width,
                height,
                pixel,
            } => {
                from.convert_inplace(format);
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
                original.convert_into(&mut self.frame);
                let sz = from.format.pixel_size();
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
                    blob.draw(&[255, 0, 0], &mut self.frame);
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
                original.convert_into(&mut self.frame);
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
                    detection.draw(&[255, 0, 0], &mut self.frame);
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
            Transform::ColorSpace(space) => {
                let _ = write!(self.title, "Color Space: {space}");
            }
            Transform::ColorFilter(FilterKind::Space(filter)) => {
                let _ = write!(self.title, "Color Filter: {filter}");
            }
            Transform::ColorFilter(FilterKind::Anon { ref min, ref max }) => {
                self.title.push_str("Color Filter: (");
                if let Some((last, rest)) = min.split_last() {
                    for elem in rest {
                        let _ = write!(self.title, "{elem}, ");
                    }
                    let _ = write!(self.title, "{last}");
                }
                self.title.push_str(")..=(");
                if let Some((last, rest)) = max.split_last() {
                    for elem in rest {
                        let _ = write!(self.title, "{elem}, ");
                    }
                    let _ = write!(self.title, "{last})");
                }
            }
            Transform::Swizzle(ref s) => {
                let _ = write!(self.title, "Swizzle: {s:?}");
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
        if ui.button("Color Space").clicked() {
            next.push(
                DerivedFrame::new(Transform::ColorSpace(PixelFormat::LUMA), id)
                    .with_updated_title(title),
            );
        }
        if ui.button("Color Filter").clicked() {
            next.push(
                DerivedFrame::new(
                    Transform::ColorFilter(FilterKind::Space(ColorFilter::Luma {
                        min_l: 0,
                        max_l: 255,
                    })),
                    id,
                )
                .with_updated_title(title),
            );
        }
        if ui.button("Swizzle").clicked() {
            next.push(DerivedFrame::new(Transform::Swizzle(vec![0]), id).with_updated_title(title));
        }
        if ui.button("Box Blur").clicked() {
            next.push(
                DerivedFrame::new(
                    Transform::BoxBlur {
                        format: PixelFormat::RGB,
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
                        format: PixelFormat::RGB,
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
                    Transform::ColorSpace(space) => {
                        changed = color_space_dropdown(ui, 3, space, true);
                    }
                    Transform::ColorFilter(filter) => {
                        let mut space = match filter {
                            FilterKind::Space(space) => space.pixel_format(),
                            FilterKind::Anon { min, .. } => {
                                PixelFormat::anon(min.len() as _).unwrap()
                            }
                        };
                        changed = color_space_dropdown(ui, 2, &mut space, true);
                        if changed {
                            match space {
                                PixelFormat::LUMA => {
                                    *filter = FilterKind::Space(ColorFilter::Luma {
                                        min_l: 0,
                                        max_l: 255,
                                    })
                                }
                                PixelFormat::RGB => {
                                    *filter = FilterKind::Space(ColorFilter::Rgb {
                                        min_r: 0,
                                        min_g: 0,
                                        min_b: 0,
                                        max_r: 255,
                                        max_g: 255,
                                        max_b: 255,
                                    })
                                }
                                PixelFormat::HSV => {
                                    *filter = FilterKind::Space(ColorFilter::Hsv {
                                        min_h: 0,
                                        max_h: 255,
                                        min_s: 0,
                                        max_s: 255,
                                        min_v: 0,
                                        max_v: 255,
                                    })
                                }
                                PixelFormat::YUYV => {
                                    *filter = FilterKind::Space(ColorFilter::Yuyv {
                                        min_y: 0,
                                        max_y: 255,
                                        min_u: 0,
                                        max_u: 255,
                                        min_v: 0,
                                        max_v: 255,
                                    })
                                }
                                PixelFormat::YCC => {
                                    *filter = FilterKind::Space(ColorFilter::YCbCr {
                                        min_y: 0,
                                        max_y: 255,
                                        min_b: 0,
                                        max_b: 255,
                                        min_r: 0,
                                        max_r: 255,
                                    })
                                }
                                PixelFormat(v) => {
                                    let len = v.get() as usize;
                                    match filter {
                                        FilterKind::Anon { min, max } => {
                                            min.resize(len, 0);
                                            max.resize(len, 255);
                                        }
                                        FilterKind::Space(_) => {
                                            let min = vec![0; len];
                                            let max = vec![255; len];
                                            *filter = FilterKind::Anon { min, max };
                                        }
                                    }
                                }
                            };
                        }
                        match filter {
                            FilterKind::Space(filter) => match filter {
                                ColorFilter::Luma { min_l, max_l } => {
                                    changed |=
                                        ui.add(RangeSlider::new("L", min_l, max_l)).changed();
                                }
                                ColorFilter::Rgb {
                                    min_r,
                                    min_g,
                                    min_b,
                                    max_r,
                                    max_g,
                                    max_b,
                                } => {
                                    changed |=
                                        ui.add(RangeSlider::new("R", min_r, max_r)).changed();
                                    changed |=
                                        ui.add(RangeSlider::new("G", min_g, max_g)).changed();
                                    changed |=
                                        ui.add(RangeSlider::new("B", min_b, max_b)).changed();
                                }
                                ColorFilter::Hsv {
                                    min_h,
                                    max_h,
                                    min_s,
                                    max_s,
                                    min_v,
                                    max_v,
                                } => {
                                    changed |=
                                        ui.add(RangeSlider::new("H", min_h, max_h)).changed();
                                    changed |=
                                        ui.add(RangeSlider::new("S", min_s, max_s)).changed();
                                    changed |=
                                        ui.add(RangeSlider::new("V", min_v, max_v)).changed();
                                }
                                ColorFilter::Yuyv {
                                    min_y,
                                    max_y,
                                    min_u,
                                    max_u,
                                    min_v,
                                    max_v,
                                } => {
                                    changed |=
                                        ui.add(RangeSlider::new("Y", min_y, max_y)).changed();
                                    changed |=
                                        ui.add(RangeSlider::new("U", min_u, max_u)).changed();
                                    changed |=
                                        ui.add(RangeSlider::new("V", min_v, max_v)).changed();
                                }
                                ColorFilter::YCbCr {
                                    min_y,
                                    max_y,
                                    min_b,
                                    max_b,
                                    min_r,
                                    max_r,
                                } => {
                                    changed |=
                                        ui.add(RangeSlider::new("Y", min_y, max_y)).changed();
                                    changed |=
                                        ui.add(RangeSlider::new("Cb", min_b, max_b)).changed();
                                    changed |=
                                        ui.add(RangeSlider::new("Cr", min_r, max_r)).changed();
                                }
                            },
                            FilterKind::Anon { min, max } => {
                                for (ch, (min, max)) in min.iter_mut().zip(max).enumerate() {
                                    changed |= ui
                                        .add(RangeSlider::new(&ch.to_string(), min, max))
                                        .changed();
                                }
                            }
                        }
                    }
                    Transform::Swizzle(s) => {
                        let mut i = 0;
                        while i < s.len() {
                            ui.horizontal(|ui| {
                                changed |= ui
                                    .add(
                                        egui::Slider::new(&mut s[i], 0..=4)
                                            .clamping(egui::SliderClamping::Never)
                                            .text(format!("{i}")),
                                    )
                                    .changed();
                                if ui.add_enabled(i > 0, egui::Button::new("^")).clicked() {
                                    s.swap(i - 1, i);
                                    changed = true;
                                }
                                if ui
                                    .add_enabled(i + 1 < s.len(), egui::Button::new("v"))
                                    .clicked()
                                {
                                    s.swap(i, i + 1);
                                    changed = true;
                                }
                                if ui
                                    .add_enabled(s.len() > 1, egui::Button::new("X"))
                                    .clicked()
                                {
                                    s.remove(i);
                                    changed = true;
                                } else {
                                    i += 1;
                                }
                            });
                        }
                        if s.len() < 200 && ui.button("New channel").clicked() {
                            s.push(0);
                        }
                    }
                    Transform::BoxBlur {
                        format,
                        width,
                        height,
                    } => {
                        changed = color_space_dropdown(ui, 0, format, false);
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
                        changed = color_space_dropdown(ui, 1, format, false);
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
                        let threads =
                            std::thread::available_parallelism().map_or(1, std::num::NonZero::get);
                        changed |= ui
                            .add(
                                egui::Slider::new(
                                    max_threads,
                                    1..=((threads * 3 / 4).clamp(1, 255) as u8),
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
fn color_space_dropdown(
    ui: &mut egui::Ui,
    id_salt: u64,
    space: &mut PixelFormat,
    allow_yuyv: bool,
) -> bool {
    let mut idx = match *space {
        PixelFormat::LUMA => 0,
        PixelFormat::RGB => 1,
        PixelFormat::HSV => 2,
        PixelFormat::YCC => 3,
        PixelFormat::RGBA => 4,
        PixelFormat::YUYV => 5,
        _ => 5 + allow_yuyv as usize,
    };
    let old = idx;
    egui::ComboBox::new(id_salt, "Color Space")
        .selected_text(space.to_string())
        .show_index(ui, &mut idx, 6 + allow_yuyv as usize, |idx| {
            const WITH: &[&str] = &["Luma", "RGB", "HSV", "YCbCr", "RGBA", "YUYV", "Unknown"];
            const WITHOUT: &[&str] = &["Luma", "RGB", "HSV", "YCbCr", "RGBA", "Unknown"];
            (if allow_yuyv { WITH } else { WITHOUT })[idx]
        });
    let mut changed = idx != old;
    match idx {
        0 => *space = PixelFormat::LUMA,
        1 => *space = PixelFormat::RGB,
        2 => *space = PixelFormat::HSV,
        3 => *space = PixelFormat::YCC,
        4 => *space = PixelFormat::RGBA,
        5 if allow_yuyv => *space = PixelFormat::YUYV,
        _ => {
            const ONE: NonZero<u8> = NonZero::new(1).unwrap();
            const TEN: NonZero<u8> = NonZero::new(10).unwrap();
            match old {
                0 => *space = PixelFormat::ANON_1,
                1..=3 | 5 => *space = PixelFormat::ANON_3,
                4 => *space = PixelFormat::ANON_4,
                _ => {}
            }
            changed |= ui
                .add(
                    egui::Slider::new(&mut space.0, ONE..=TEN)
                        .clamping(egui::SliderClamping::Never)
                        .text("Channels"),
                )
                .changed();
            if space.0 > PixelFormat::MAX_ANON {
                space.0 = PixelFormat::MAX_ANON;
            }
        }
    }
    changed
}
