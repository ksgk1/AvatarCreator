use image::imageops::resize;
use rfd::FileDialog;
use tracing::{debug, error};
use {
    crate::app::RESIZE_FILTER,
    eframe::egui::{
        self, CentralPanel, Color32, CornerRadius, Image, Panel, Pos2, Rect, RichText, ScrollArea,
        Sense, Slider, Stroke, StrokeKind, Ui, Vec2,
    },
};

use super::{AvatarCreatorApp, BorderColours, GradientChange, MAX_BORDER_PERCENT, Shape};
use crate::SUPPORTED_IMAGE_FORMATS;

impl eframe::App for AvatarCreatorApp {
    fn ui(&mut self, ui: &mut Ui, _: &mut eframe::Frame) {
        CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                self.file_opening(ui);
                self.selection_reset(ui);
                if let Some(message) = &self.error_message {
                    ui.label(RichText::new(message).color(Color32::RED));
                }
            });

            if self.image.is_some() {
                let inputs_before = self.preview_inputs();
                Panel::left("utility_panel")
                    .default_size(280.0)
                    .show(ui, |ui| {
                        ScrollArea::vertical()
                            .auto_shrink([false, true])
                            .show(ui, |ui| self.utility_elements(ui));
                    });
                Panel::right("preview_panel").show(ui, |ui| self.preview_panel(ui));
                self.main_display_panel(ui);
                if self.preview_outdated(&inputs_before) {
                    self.regenerate_preview(ui.ctx());
                }
            }
        });
    }
}

impl AvatarCreatorApp {
    fn regenerate_preview(&mut self, ctx: &egui::Context) {
        let Some(masked) = self.generate_masked_avatar() else {
            self.preview_image = None;
            self.preview_texture = None;
            return;
        };
        let final_size = if self.resize {
            u32::from(self.resize_size)
        } else {
            masked.width()
        };
        let final_image = if masked.width() == final_size {
            masked
        } else {
            resize(&masked, final_size, final_size, RESIZE_FILTER)
        };
        let size = [final_image.width() as usize, final_image.height() as usize];
        let texture = ctx.load_texture(
            "preview",
            egui::ColorImage::from_rgba_unmultiplied(size, final_image.as_raw()),
            egui::TextureOptions::default(),
        );
        self.preview_image = Some(final_image);
        self.preview_texture = Some(texture);
    }

    fn preview_panel(&mut self, ui: &mut Ui) {
        // Panel persist their size from their content, so the content must
        // fill the panel's width, otherwise the panel would shrink to the
        // content's width and snap back after the divider is dragged.
        ui.set_min_width(ui.available_width());
        let Some(texture) = &self.preview_texture else {
            return;
        };
        ui.vertical(|ui| {
            ui.label("Preview:");
            let available = ui.available_size();
            ui.add(
                Image::from_texture(texture)
                    .max_width(available.x)
                    .max_height(available.y),
            );
            let size = texture.size_vec2();
            ui.label(
                RichText::new(format!(
                    "Size: {width:.0} x {height:.0}",
                    width = size.x,
                    height = size.y
                ))
                .monospace(),
            );
        });
    }

    fn main_display_panel(&mut self, ui: &mut Ui) {
        let (Some(image), Some(dimension)) = (self.image.clone(), self.original_image_dimension)
        else {
            return;
        };
        ui.vertical(|ui| {
            ui.label(
                "Draw from the center out to create a preview.\nRight click to move selection.",
            );
            let available = ui.available_size();
            let image = image
                .sense(Sense::click_and_drag())
                .maintain_aspect_ratio(true)
                .max_width(available.x)
                .max_height(available.y);
            let response = ui.add(image);

            // factor between displayed pixels and original image pixels
            let displayed = response.rect.size();
            let scale = Vec2::new(displayed.x / dimension.x, displayed.y / dimension.y);
            if !(scale.x.is_finite() && scale.x > 0.0) {
                return;
            }
            let origin = response.rect.left_top();

            if response.drag_started_by(egui::PointerButton::Primary)
                && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
            {
                let point = pointer_to_image_position(pointer, origin, scale, dimension);
                let inside_last_selection = match (self.drag_start, self.drag_stop) {
                    (Some(start), Some(stop)) => point.distance(start) <= start.distance(stop),
                    _ => false,
                };
                if !inside_last_selection {
                    self.drag_start = Some(point);
                    self.drag_stop = None;
                }
            }

            if response.dragged_by(egui::PointerButton::Primary)
                && let Some(pointer) = ui.input(|i| i.pointer.interact_pos())
            {
                self.drag_stop = Some(pointer_to_image_position(pointer, origin, scale, dimension));
            }

            if response.dragged_by(egui::PointerButton::Secondary)
                && let (Some(start), Some(stop)) =
                    (self.drag_start.as_mut(), self.drag_stop.as_mut())
            {
                let delta = response.drag_delta();
                let delta = Vec2::new(delta.x / scale.x, delta.y / scale.y);
                let radius = start.distance(*stop);
                let max_x = (dimension.x - radius).max(radius);
                let max_y = (dimension.y - radius).max(radius);
                let shift = Vec2::new(
                    (start.x + delta.x).clamp(radius, max_x) - start.x,
                    (start.y + delta.y).clamp(radius, max_y) - start.y,
                );
                *start += shift;
                *stop += shift;
            }

            self.paint_selection(ui, response.rect, origin, scale);
        });
    }

    /// Paints the outline of the current selection on top of the image.
    fn paint_selection(&mut self, ui: &mut Ui, image_rect: Rect, origin: Pos2, scale: Vec2) {
        match (self.drag_start, self.drag_stop) {
            (Some(start), Some(stop)) => {
                let painter = ui.painter_at(image_rect);
                let center = image_to_screen_position(start, origin, scale);
                let radius = start.distance(stop) * scale.x;
                painter.circle(
                    center,
                    1.0,
                    Color32::TRANSPARENT,
                    Stroke::new(1.0, Color32::RED),
                );
                match self.shape {
                    Shape::Circle => {
                        painter.circle(
                            center,
                            radius,
                            Color32::TRANSPARENT,
                            Stroke::new(1.0, Color32::GREEN),
                        );
                    }
                    Shape::Square => {
                        let rect = Rect::from_center_size(center, Vec2::splat(2.0 * radius));
                        painter.rect(
                            rect,
                            CornerRadius::ZERO,
                            Color32::TRANSPARENT,
                            Stroke::new(1.0, Color32::GREEN),
                            StrokeKind::Outside,
                        );
                    }
                }
            }
            (Some(start), None) => {
                let painter = ui.painter_at(image_rect);
                let center = image_to_screen_position(start, origin, scale);
                painter.circle(
                    center,
                    1.0,
                    Color32::TRANSPARENT,
                    Stroke::new(1.0, Color32::RED),
                );
            }
            (None, _) => {
                // nothing to paint when no start point is set
            }
        }
    }

    fn utility_elements(&mut self, ui: &mut Ui) {
        self.shape_selection(ui);
        ui.label("Border width percentage:");
        ui.add(Slider::new(&mut self.border_percent, 0..=MAX_BORDER_PERCENT).drag_value_speed(1.0));
        self.selection_info(ui);
        ui.checkbox(
            &mut self.resize,
            RichText::new(format!(
                "Resize to {size:>4} x {size:>4} pixels",
                size = self.resize_size
            ))
            .monospace(),
        );
        ui.add_enabled(self.resize, Slider::new(&mut self.resize_size, 100..=1000));
        ui.separator();
        if ui
            .add_enabled(
                self.selection_complete(),
                egui::Button::new("Generate avatar"),
            )
            .clicked()
        {
            let outcome = self.generate_avatar();
            self.error_message = outcome.err().map(|e| e.to_string());
        }
        ui.separator();
        self.border_colour_selection(ui);
        ui.horizontal(|ui| {
            ui.label("Gradient angle:");
            let mut angle_deg = self.colours.angle_deg();
            ui.add_enabled(
                !self.colours.is_single(),
                Slider::new(&mut angle_deg, 0.0..=360.0).drag_value_speed(0.1),
            );
            self.colours.set_angle_deg(angle_deg);
        });
        // the pre-defined colours do not apply to the rainbow
        if !self.colours.is_rainbow() {
            ui.label("Pre-defined colours:");
            // wrapped, so the buttons move onto a new line on narrow panels
            ui.horizontal_wrapped(|ui| {
                self.predefined_colour_button(ui, "Crimson", Color32::from_rgb(158, 27, 50));
                self.predefined_colour_button(ui, "Amber", Color32::from_rgb(255, 192, 0));
                self.predefined_colour_button(ui, "Malachite", Color32::from_rgb(11, 218, 81));
            });
        }
        self.gradient_colour_selector(ui);
        self.border_colour_element(ui);
    }

    fn selection_info(&mut self, ui: &mut Ui) {
        let text = match (self.drag_start, self.drag_stop) {
            (Some(start), Some(stop)) => format!(
                "Position: ({x:.0}, {y:.0})   Radius: {radius:.0}",
                x = start.x,
                y = start.y,
                radius = start.distance(stop)
            ),
            _ => "Position: -   Radius: -".to_owned(),
        };
        ui.label(RichText::new(text).monospace());
    }

    fn predefined_colour_button(&mut self, ui: &mut Ui, name: &str, colour: Color32) {
        if ui.button(name).clicked() {
            self.colours.set_picked(colour, self.gradient_change);
            debug!("Changed border colour to: {colour:?}");
        }
    }

    fn file_opening(&mut self, ui: &mut Ui) {
        if ui.button("Load Image").clicked()
            && let Some(image_path) = FileDialog::new()
                .add_filter("Supported image formats", SUPPORTED_IMAGE_FORMATS)
                .pick_file()
        {
            match self.load_image_to_state(&image_path) {
                Ok(()) => self.error_message = None,
                Err(e) => {
                    self.reset_image_state();
                    error!("Failed to load image: {e}");
                    self.error_message = Some(e.to_string());
                }
            }
        }
    }

    fn selection_reset(&mut self, ui: &mut Ui) {
        if ui
            .add_enabled(
                self.selection_complete(),
                egui::Button::new("Reset selection"),
            )
            .clicked()
        {
            self.drag_start = None;
            self.drag_stop = None;
            self.preview_image = None;
            self.preview_texture = None;
            debug!("Reset selection");
        }
    }

    fn shape_selection(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Avatar shape:");
            ui.radio_value(&mut self.shape, Shape::Square, "Square");
            ui.radio_value(&mut self.shape, Shape::Circle, "Circle");
        });
    }

    fn border_colour_selection(&mut self, ui: &mut Ui) {
        ui.label("Border colour type:");
        // wrapped, so the options move onto a new line on narrow panels
        ui.horizontal_wrapped(|ui| {
            if ui
                .radio(self.colours.is_single(), "Single colour")
                .clicked()
            {
                self.colours =
                    BorderColours::Single(self.colours.picked_colour(self.gradient_change));
            }
            if ui.radio(self.colours.is_gradient(), "Gradient").clicked() {
                self.colours = BorderColours::Gradient {
                    angle_deg: self.colours.angle_deg(),
                    first: self.colours.picked_colour(GradientChange::First),
                    last: self.colours.picked_colour(GradientChange::Last),
                };
            }
            if ui.radio(self.colours.is_rainbow(), "Rainbow").clicked() {
                self.colours = BorderColours::Rainbow {
                    angle_deg: self.colours.angle_deg(),
                };
            }
        });
    }

    fn border_colour_element(&mut self, ui: &mut Ui) {
        if self.colours.is_rainbow() {
            ui.label("The rainbow border uses the full colour spectrum.");
            return;
        }
        ui.vertical(|ui| {
            ui.label("Border colour:");
            ui.scope(|ui| {
                ui.spacing_mut().slider_width = 100.0;
                let mut colour = self.colours.picked_colour(self.gradient_change);
                egui::widgets::color_picker::color_picker_color32(
                    ui,
                    &mut colour,
                    egui::color_picker::Alpha::Opaque,
                );
                self.colours.set_picked(colour, self.gradient_change);
            })
        });
    }

    fn gradient_colour_selector(&mut self, ui: &mut Ui) {
        if self.colours.is_gradient() {
            ui.vertical(|ui| {
                ui.radio_value(
                    &mut self.gradient_change,
                    GradientChange::First,
                    "First colour",
                );
                ui.radio_value(
                    &mut self.gradient_change,
                    GradientChange::Last,
                    "Second colour",
                );
            });
        }
    }
}

fn pointer_to_image_position(pointer: Pos2, origin: Pos2, scale: Vec2, dimension: Vec2) -> Pos2 {
    Pos2::new(
        ((pointer.x - origin.x) / scale.x).clamp(0.0, dimension.x),
        ((pointer.y - origin.y) / scale.y).clamp(0.0, dimension.y),
    )
}

fn image_to_screen_position(point: Pos2, origin: Pos2, scale: Vec2) -> Pos2 {
    Pos2::new(origin.x + point.x * scale.x, origin.y + point.y * scale.y)
}
