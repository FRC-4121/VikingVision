use eframe::egui::{self, *};
use std::ops::RangeInclusive;

/// A slider widget for selecting a range with two handles (min and max).
///
/// ```
/// # egui::__run_test_ui(|ui| {
/// let mut min: u8 = 50;
/// let mut max: u8 = 200;
/// ui.add(RangeSlider::new("Range", &mut min, &mut max));
/// # });
/// ```
#[must_use = "You should put this widget in a ui with `ui.add(widget);`"]
pub struct RangeSlider<'a> {
    label: &'static str,
    min_value: &'a mut u8,
    max_value: &'a mut u8,
    range: RangeInclusive<u8>,
}

impl<'a> RangeSlider<'a> {
    /// Creates a new range slider.
    ///
    /// Both values will be clamped to ensure min <= max.
    pub fn new(label: &'static str, min_value: &'a mut u8, max_value: &'a mut u8) -> Self {
        Self {
            label,
            min_value,
            max_value,
            range: 0..=255,
        }
    }

    fn handle_radius(&self, rect: &Rect) -> f32 {
        rect.height() / 2.5
    }

    fn position_range(&self, rect: &Rect) -> Rangef {
        let handle_radius = self.handle_radius(rect);
        rect.x_range().shrink(handle_radius)
    }

    fn rail_rect(&self, rect: &Rect, radius: f32) -> Rect {
        Rect::from_min_max(
            pos2(rect.left(), rect.center().y - radius),
            pos2(rect.right(), rect.center().y + radius),
        )
    }

    fn value_from_position(&self, position: f32, position_range: Rangef) -> u8 {
        let normalized = remap_clamp(position, position_range, 0.0..=1.0);
        let value = lerp(
            (*self.range.start() as f32)..=(*self.range.end() as f32),
            normalized,
        );
        value
            .round()
            .clamp(*self.range.start() as f32, *self.range.end() as f32) as u8
    }

    fn position_from_value(&self, value: u8, position_range: Rangef) -> f32 {
        let normalized = remap_clamp(
            value as f32,
            (*self.range.start() as f32)..=(*self.range.end() as f32),
            0.0..=1.0,
        );
        lerp(position_range, normalized)
    }
}

impl Widget for RangeSlider<'_> {
    fn ui(self, ui: &mut Ui) -> Response {
        let mut resp = ui.horizontal(|ui| {
            // Ensure min <= max
            if *self.min_value > *self.max_value {
                std::mem::swap(self.min_value, self.max_value);
            }

            let desired_size = vec2(
                ui.spacing().slider_width,
                ui.text_style_height(&TextStyle::Body)
                    .at_least(ui.spacing().interact_size.y),
            );
            let (rect, mut response) = ui.allocate_exact_size(desired_size, Sense::drag());

            let handle_shape = ui.style().visuals.handle_shape;
            let position_range = self.position_range(&rect);

            let min_pos = self.position_from_value(*self.min_value, position_range);
            let max_pos = self.position_from_value(*self.max_value, position_range);

            if let Some(pointer_pos) = response.interact_pointer_pos() {
                let pointer_x = pointer_pos.x;

                // Calculate distances to both handles
                let dist_to_min = (pointer_x - min_pos).abs();
                let dist_to_max = (pointer_x - max_pos).abs();

                // Choose the closer handle
                if dist_to_min < dist_to_max {
                    let new_value = self.value_from_position(pointer_x, position_range);
                    *self.min_value = new_value.min(*self.max_value);
                } else {
                    let new_value = self.value_from_position(pointer_x, position_range);
                    *self.max_value = new_value.max(*self.min_value);
                }
                response.mark_changed();
            }

            // Paint the slider
            if ui.is_rect_visible(rect) {
                let visuals = ui.style().interact(&response);
                let widget_visuals = &ui.visuals().widgets;
                let spacing = &ui.style().spacing;

                let rail_radius = (spacing.slider_rail_height / 2.0).at_least(0.0);
                let rail_rect = self.rail_rect(&rect, rail_radius);
                let corner_radius = widget_visuals.inactive.corner_radius;

                // Draw background rail
                ui.painter()
                    .rect_filled(rail_rect, corner_radius, widget_visuals.inactive.bg_fill);

                // Draw filled range between handles
                let min_x = self.position_from_value(*self.min_value, position_range);
                let max_x = self.position_from_value(*self.max_value, position_range);
                let filled_rect =
                    Rect::from_min_max(pos2(min_x, rail_rect.min.y), pos2(max_x, rail_rect.max.y));
                ui.painter().rect_filled(
                    filled_rect,
                    corner_radius,
                    ui.visuals().selection.bg_fill,
                );

                let radius = self.handle_radius(&rect);

                // Draw min handle
                let min_center = pos2(min_x, rail_rect.center().y);
                match handle_shape {
                    egui::style::HandleShape::Circle => {
                        ui.painter().add(epaint::CircleShape {
                            center: min_center,
                            radius: radius + visuals.expansion,
                            fill: visuals.bg_fill,
                            stroke: visuals.fg_stroke,
                        });
                    }
                    egui::style::HandleShape::Rect { aspect_ratio } => {
                        let v = Vec2::new(radius * aspect_ratio, radius)
                            + Vec2::splat(visuals.expansion);
                        let rect = Rect::from_center_size(min_center, 2.0 * v);
                        ui.painter().rect(
                            rect,
                            visuals.corner_radius,
                            visuals.bg_fill,
                            visuals.fg_stroke,
                            epaint::StrokeKind::Inside,
                        );
                    }
                }

                // Draw max handle
                let max_center = pos2(max_x, rail_rect.center().y);
                match handle_shape {
                    egui::style::HandleShape::Circle => {
                        ui.painter().add(epaint::CircleShape {
                            center: max_center,
                            radius: radius + visuals.expansion,
                            fill: visuals.bg_fill,
                            stroke: visuals.fg_stroke,
                        });
                    }
                    egui::style::HandleShape::Rect { aspect_ratio } => {
                        let v = Vec2::new(radius * aspect_ratio, radius)
                            + Vec2::splat(visuals.expansion);
                        let rect = Rect::from_center_size(max_center, 2.0 * v);
                        ui.painter().rect(
                            rect,
                            visuals.corner_radius,
                            visuals.bg_fill,
                            visuals.fg_stroke,
                            epaint::StrokeKind::Inside,
                        );
                    }
                }
            }

            if ui
                .add(egui::DragValue::new(self.min_value).range(0..=*self.max_value))
                .changed()
            {
                response.mark_changed();
            }
            if ui
                .add(egui::DragValue::new(self.max_value).range(*self.min_value..=255))
                .changed()
            {
                response.mark_changed();
            }

            // Add widget info for accessibility
            response.widget_info(|| {
                WidgetInfo::slider(ui.is_enabled(), *self.min_value as f64, self.label)
            });

            if !self.label.is_empty() {
                ui.label(self.label);
            }

            response
        });
        if resp.inner.changed() {
            resp.response.mark_changed();
        }
        resp.response
    }
}
