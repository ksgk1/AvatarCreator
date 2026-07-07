use std::path::{Path, PathBuf};

use eframe::egui::{self, CentralPanel, Color32, CornerRadius, Image, Popup, Pos2, Rect, RichText, Sense, Slider, Stroke, StrokeKind, Ui, Vec2};
use image::imageops::resize;
use image::{ImageBuffer, ImageReader, Rgba, RgbaImage};
use rfd::FileDialog;
use thiserror::Error;
use tracing::{debug, error, info};

use crate::SUPPORTED_IMAGE_FORMATS;

/// The height of all UI elements that are not the image.
///
/// Includes all labels, buttons and colour picker
const TOTAL_UI_ELEMENT_HEIGHT: f32 = 250.0;
/// Default border size (in percent)
const BORDER_DEFAULT_PERCENT: f32 = 15.0;
const MAX_BORDER_PERCENT: f32 = 100.0;

#[derive(Error, Debug)]
enum Error {
    #[error("Missing selection")]
    MissingSelection,
}

pub struct AvatarCreatorApp<'a> {
    filepath:                 Option<PathBuf>,
    drag_start:               Option<Pos2>,
    drag_stop:                Option<Pos2>,
    border_percent:           f32,
    border_colour:            Color32,
    original_image_dimension: Option<Vec2>,
    scaled_image_dimension:   Option<Vec2>,
    image:                    Option<Image<'a>>,
    resize:                   bool,
    resize_size:              u16,
    preview_texture:          Option<egui::TextureHandle>,
    preview_image:            Option<ImageBuffer<Rgba<u8>, Vec<u8>>>,
    shape:                    Shape,
    color_kind:               ColorKind,
    gradient_angle_deg:       f32,
    gradient_first_colour:    Color32,
    gradient_last_colour:     Color32,
    gradient_change:          GradientChange,
    error_popup_open:         bool,
    error_message:            Option<String>,
}

#[derive(PartialEq)]
enum Shape {
    Square,
    Circle,
}

#[derive(PartialEq)]
enum ColorKind {
    SingleColour,
    Gradient,
}

#[derive(PartialEq)]
enum GradientChange {
    First,
    Last,
}

impl Default for AvatarCreatorApp<'_> {
    fn default() -> Self {
        Self {
            filepath:                 None,
            drag_start:               None,
            drag_stop:                None,
            border_percent:           BORDER_DEFAULT_PERCENT,
            border_colour:            Color32::WHITE,
            original_image_dimension: None,
            scaled_image_dimension:   None,
            image:                    None,
            resize:                   true,
            resize_size:              400,
            preview_texture:          None,
            preview_image:            None,
            shape:                    Shape::Circle,
            color_kind:               ColorKind::SingleColour,
            gradient_angle_deg:       0.0,
            gradient_first_colour:    Color32::WHITE,
            gradient_last_colour:     Color32::WHITE,
            gradient_change:          GradientChange::First,
            error_popup_open:         false,
            error_message:            None,
        }
    }
}

impl eframe::App for AvatarCreatorApp<'_> {
    #[allow(clippy::cognitive_complexity)]
    fn ui(&mut self, ui: &mut Ui, _: &mut eframe::Frame) {
        CentralPanel::default().show(ui, |ui| {
            ui.horizontal(|ui| {
                self.file_opening(ui);
                self.selection_reset(ui);
            });

            if self.filepath.is_some() {
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        self.main_display_panel(ui);
                        self.utility_elements(ui);
                    });
                    self.preview_panel(ui);
                });
            }
        });
    }
}

impl AvatarCreatorApp<'_> {
    fn generate_avatar(&self) -> Result<(), Box<dyn std::error::Error>> {
        let Some(image) = &self.preview_image else {
            error!("No preview available!");
            return Err(Box::new(Error::MissingSelection));
        };
        info!("Generating avatar...");

        // resize if required
        let final_image = if self.resize { resize(image, self.resize_size.into(), self.resize_size.into(), image::imageops::FilterType::Lanczos3) } else { image.clone() };
        let resulting_file_name = self.generate_final_file_name();
        match final_image.save(&resulting_file_name) {
            Ok(()) => {
                let path = Path::new(&resulting_file_name);
                let file_name = path.file_name().unwrap().display();
                info!("Generation successful! File name: {}", file_name);
                Ok(())
            }
            Err(e) => {
                error!("Failed! Error while saving avatar. Error: {e}");
                Err(Box::new(e))
            }
        }
    }

    fn generate_final_file_name(&self) -> String {
        let filename = self.filepath.as_ref().expect("File path is set").display();
        let resize_text = if self.resize { format!("_resized_{}", self.resize_size) } else { String::new() };
        format!("{filename}{resize_text}_cropped.png")
    }

    // creates the square border around the selection made
    fn create_square_border(
        &self,
        cropped: &RgbaImage,
        inner_size: f32,
        content_half: f32, // inner_size - border_size
    ) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        let final_size = (2.0 * inner_size).ceil() as u32;
        let mut masked = ImageBuffer::new(final_size, final_size);
        let final_center = Pos2::new(final_size as f32 / 2.0, final_size as f32 / 2.0);

        if content_half <= 0.0 {
            let colour = match self.color_kind {
                ColorKind::SingleColour => Rgba::from(self.border_colour.to_srgba_unmultiplied()),
                ColorKind::Gradient => Rgba::from(self.gradient_first_colour.to_srgba_unmultiplied()),
            };
            for pixel in masked.pixels_mut() {
                *pixel = colour;
                pixel[3] = 255;
            }
            return masked;
        }

        let content_pixel_size = (2.0 * content_half).ceil() as u32;
        let scaled_content = resize(cropped, content_pixel_size, content_pixel_size, image::imageops::FilterType::Lanczos3);

        let offset_x = (final_size as f32 - content_pixel_size as f32) / 2.0;
        let offset_y = (final_size as f32 - content_pixel_size as f32) / 2.0;

        let angle_rad = (self.gradient_angle_deg % 360.0) * std::f32::consts::PI / 180.0;
        let dir_x = angle_rad.cos();
        let dir_y = angle_rad.sin();
        let corners = [Pos2::new(0.0, 0.0), Pos2::new(final_size as f32, 0.0), Pos2::new(final_size as f32, final_size as f32), Pos2::new(0.0, final_size as f32)];
        let (min_proj, max_proj) = Self::calc_point_gradient_direction(final_center, dir_x, dir_y, &corners);
        let proj_range = max_proj - min_proj;

        for y in 0..final_size {
            for x in 0..final_size {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let dx = px - final_center.x;
                let dy = py - final_center.y;
                let pixel = masked.get_pixel_mut(x, y);

                let is_inside_content = dx.abs() <= content_half && dy.abs() <= content_half;
                let is_inside_inner = dx.abs() <= inner_size && dy.abs() <= inner_size;

                if is_inside_content {
                    let src_x = (px - offset_x).clamp(0.0, content_pixel_size as f32 - 1.0) as u32;
                    let src_y = (py - offset_y).clamp(0.0, content_pixel_size as f32 - 1.0) as u32;
                    if let Some(src_pixel) = scaled_content.get_pixel_checked(src_x, src_y) {
                        *pixel = *src_pixel;
                        pixel[3] = 255;
                    } else {
                        pixel[3] = 0;
                    }
                } else if is_inside_inner {
                    let proj = dx * dir_x + dy * dir_y;
                    let gradient_pos = if proj_range > 0.0 { (proj - min_proj) / proj_range } else { 0.5 };
                    let color = match self.color_kind {
                        ColorKind::SingleColour => Rgba::from(self.border_colour.to_srgba_unmultiplied()),
                        ColorKind::Gradient => {
                            let first = Rgba::from(self.gradient_first_colour.to_srgba_unmultiplied());
                            let last = Rgba::from(self.gradient_last_colour.to_srgba_unmultiplied());
                            lerp(first, last, gradient_pos)
                        }
                    };
                    pixel.clone_from(&Rgba([color[0], color[1], color[2], 255]));
                } else {
                    pixel[3] = 0;
                }
            }
        }
        masked
    }

    /// creates the circle border around the selection made
    fn create_circle_border(
        &self,
        cropped: &RgbaImage, // selection crop (size = 2*inner_radius)
        inner_radius: f32,
        content_radius: f32, // inner_radius - border_size
    ) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        let final_size = (2.0 * inner_radius).ceil() as u32;
        let mut masked = ImageBuffer::new(final_size, final_size);
        let final_center = Pos2::new(final_size as f32 / 2.0, final_size as f32 / 2.0);

        if content_radius <= 0.0 {
            let colour = match self.color_kind {
                ColorKind::SingleColour => Rgba::from(self.border_colour.to_srgba_unmultiplied()),
                ColorKind::Gradient => Rgba::from(self.gradient_first_colour.to_srgba_unmultiplied()),
            };
            for pixel in masked.pixels_mut() {
                *pixel = colour;
                pixel[3] = 255;
            }
            return masked;
        }

        // scale the selection crop to the content area
        let content_pixel_size = (2.0 * content_radius).ceil() as u32;
        let scaled_content = resize(cropped, content_pixel_size, content_pixel_size, image::imageops::FilterType::Lanczos3);

        let offset_x = (final_size as f32 - content_pixel_size as f32) / 2.0;
        let offset_y = (final_size as f32 - content_pixel_size as f32) / 2.0;

        // precompute gradient direction
        let angle_rad = (self.gradient_angle_deg % 360.0) * std::f32::consts::PI / 180.0;
        let dir_x = angle_rad.cos();
        let dir_y = angle_rad.sin();
        let corners = [Pos2::new(0.0, 0.0), Pos2::new(final_size as f32, 0.0), Pos2::new(final_size as f32, final_size as f32), Pos2::new(0.0, final_size as f32)];
        let (min_proj, max_proj) = Self::calc_point_gradient_direction(final_center, dir_x, dir_y, &corners);
        let proj_range = max_proj - min_proj;

        for y in 0..final_size {
            for x in 0..final_size {
                let px = x as f32 + 0.5;
                let py = y as f32 + 0.5;
                let dx = px - final_center.x;
                let dy = py - final_center.y;
                let dist_sq = dx * dx + dy * dy;
                let pixel = masked.get_pixel_mut(x, y);

                if dist_sq <= content_radius.powi(2) {
                    // Inside content circle: sample from scaled_content
                    let src_x = (px - offset_x).clamp(0.0, content_pixel_size as f32 - 1.0) as u32;
                    let src_y = (py - offset_y).clamp(0.0, content_pixel_size as f32 - 1.0) as u32;
                    if let Some(src_pixel) = scaled_content.get_pixel_checked(src_x, src_y) {
                        *pixel = *src_pixel;
                        pixel[3] = 255;
                    } else {
                        pixel[3] = 0;
                    }
                } else if dist_sq <= inner_radius.powi(2) {
                    // Border ring
                    let proj = dx * dir_x + dy * dir_y;
                    let gradient_pos = if proj_range > 0.0 { (proj - min_proj) / proj_range } else { 0.5 };
                    let color = match self.color_kind {
                        ColorKind::SingleColour => Rgba::from(self.border_colour.to_srgba_unmultiplied()),
                        ColorKind::Gradient => {
                            let first = Rgba::from(self.gradient_first_colour.to_srgba_unmultiplied());
                            let last = Rgba::from(self.gradient_last_colour.to_srgba_unmultiplied());
                            lerp(first, last, gradient_pos)
                        }
                    };
                    pixel.clone_from(&Rgba([color[0], color[1], color[2], 255]));
                } else {
                    pixel[3] = 0;
                }
            }
        }
        masked
    }

    fn calc_point_gradient_direction(center: Pos2, dir_x: f32, dir_y: f32, corners: &[Pos2; 4]) -> (f32, f32) {
        let mut min_proj = f32::MAX;
        let mut max_proj = f32::MIN;
        for corner in corners {
            // project the point onto the gradient direction
            let proj = (corner.x - center.x) * dir_x + (corner.y - center.y) * dir_y;
            min_proj = min_proj.min(proj);
            max_proj = max_proj.max(proj);
        }
        (min_proj, max_proj)
    }

    /// Tries to get the selected start and stop positions
    fn try_get_points(&self) -> Result<(Pos2, Pos2), Error> {
        if let (Some(start), Some(stop)) = (self.drag_start, self.drag_stop) {
            Ok((start, stop))
        } else {
            error!("No selection made, can not generate avatar!");
            Err(Error::MissingSelection)
        }
    }

    #[allow(clippy::type_complexity)]
    /// Generates the preview used in the active window to be display, as a
    /// texture.
    ///
    /// It also provides the resulting image as an `ImageBuffer`, ready to be
    /// saved.
    fn generate_preview(&self, ui: &Ui) -> Result<(ImageBuffer<Rgba<u8>, Vec<u8>>, egui::TextureHandle), Box<dyn std::error::Error>> {
        let src_image = ImageReader::open(self.filepath.as_ref().expect("File path is set"))?.decode()?;
        let scale_factor = self.original_image_dimension.expect("Dimensions have been set").x / self.scaled_image_dimension.expect("Dimensions have been set").x;
        let (start, stop) = self.try_get_points()?;
        let (scaled_start, scaled_stop) = (start * scale_factor, stop * scale_factor);

        let inner_radius = scaled_start.distance(scaled_stop);
        let border_size = (self.border_percent / 100.0) * inner_radius;
        let content_radius = inner_radius - border_size;

        // bounding box for cropping: only the selected shape
        let selection_radius = match self.shape {
            Shape::Circle | Shape::Square => inner_radius, // half‑side
        };
        let bounding_box = Rect::from_center_size(scaled_start, Vec2::splat(2.0 * selection_radius));

        let crop_x = bounding_box.left_top().x.max(0.0).min(src_image.width() as f32) as u32;
        let crop_y = bounding_box.left_top().y.max(0.0).min(src_image.height() as f32) as u32;
        let crop_width = bounding_box.width().min(src_image.width() as f32) as u32;
        let crop_height = bounding_box.height().min(src_image.height() as f32) as u32;
        let cropped_image = src_image.crop_imm(crop_x, crop_y, crop_width, crop_height).into_rgba8();

        let cropped_masked_image = match self.shape {
            Shape::Circle => self.create_circle_border(&cropped_image, inner_radius, content_radius),
            Shape::Square => self.create_square_border(&cropped_image, inner_radius, content_radius),
        };

        let window_dimensions = ui.input(eframe::egui::InputState::content_rect).size();
        let preview_size =
            if self.resize { u32::from(((0.5 * (window_dimensions.y - TOTAL_UI_ELEMENT_HEIGHT)) as u16).max((0.5 * window_dimensions.x) as u16).min(self.resize_size)) } else { crop_width };
        let final_image =
            if preview_size == cropped_masked_image.width() { cropped_masked_image.clone() } else { resize(&cropped_masked_image, preview_size, preview_size, image::imageops::FilterType::Lanczos3) };

        let pixels = final_image.as_raw();
        let texture = ui.ctx().load_texture("preview", egui::ColorImage::from_rgba_unmultiplied([preview_size as usize, preview_size as usize], pixels), egui::TextureOptions::default());
        Ok((final_image, texture))
    }

    /// UI element that displays the preview avatar if available
    fn preview_panel(&mut self, ui: &mut Ui) {
        if let Some(texture) = &self.preview_texture {
            ui.vertical(|ui| {
                ui.label("Preview:");
                ui.image(texture);
                let dims = texture.size();
                ui.label(format!("Size: {} x {}", dims[0], dims[1]));
            });
        }
    }

    /// UI element for the main panel with the selected image
    fn main_display_panel(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            let window_dimensions = ui.input(eframe::egui::InputState::content_rect).size();
            let image = self
                .image
                .clone()
                .expect("Image is readable")
                .sense(Sense::click_and_drag())
                .fit_to_original_size(1.0)
                .max_width(window_dimensions.x)
                .max_height(window_dimensions.y - TOTAL_UI_ELEMENT_HEIGHT);

            ui.label("Draw from the center out to create a preview.\nRight click to move selection.");
            let response = ui.add(image);
            self.scaled_image_dimension = Some(response.rect.size());

            let mut started_in_last_selection = false;
            // generate start point when new selection is created by left-clicking.
            // when clicked on the previous selection, the old starting point is kept.
            if response.drag_started_by(egui::PointerButton::Primary) {
                let point = ui.input(|i| i.pointer.interact_pos().unwrap_or_default()) - response.rect.left_top().to_vec2();
                if let (Some(start), Some(stop)) = (self.drag_start, self.drag_stop) {
                    // Check if started within selection
                    if (point.distance(start)) <= start.distance(stop) {
                        started_in_last_selection = true;
                    }
                }
                if !started_in_last_selection {
                    self.drag_start = Some(point);
                    self.drag_stop = None;
                }
            }

            if response.dragged_by(egui::PointerButton::Primary) {
                self.drag_stop = Some(ui.input(|i| i.pointer.interact_pos().unwrap_or_default()) - response.rect.left_top().to_vec2());
            }

            // moving selection when using the right button
            if response.dragged_by(egui::PointerButton::Secondary)
                && let (Some(start), Some(stop)) = (self.drag_start.as_mut(), self.drag_stop.as_mut())
            {
                *start += response.drag_delta();
                *stop += response.drag_delta();
            }

            match (self.drag_start, self.drag_stop) {
                (Some(start), Some(stop)) => {
                    let painter = ui.painter_at(response.rect);
                    let circle_center = response.rect.left_top() + Vec2 { x: start.x, y: start.y };
                    let selection_radius = start.distance(stop);
                    painter.circle(circle_center, 1.0, Color32::TRANSPARENT, Stroke::new(1.0, Color32::RED));

                    match self.shape {
                        Shape::Circle => {
                            painter.circle(circle_center, selection_radius, Color32::TRANSPARENT, Stroke::new(1.0, Color32::GREEN));
                        }
                        Shape::Square => {
                            let half = selection_radius;
                            let rect = Rect::from_center_size(circle_center, Vec2::splat(2.0 * half));
                            painter.rect(rect, CornerRadius::ZERO, Color32::TRANSPARENT, Stroke::new(1.0, Color32::GREEN), StrokeKind::Outside);
                        }
                    }
                }
                (Some(start), None) => {
                    let painter = ui.painter_at(response.rect);

                    // offset the circle's position by the second image's top-left corner
                    let circle_center = response.rect.left_top() + Vec2 { x: start.x, y: start.y };
                    let radius = 1.0;
                    painter.circle(circle_center, radius, Color32::TRANSPARENT, Stroke::new(1.0, Color32::RED));
                }
                (None, _) => {
                    // nothing to do when no start point is set
                }
            }

            // after handling the drag and selection, generate a preview
            if let (Some(_start), Some(_stop)) = (self.drag_start, self.drag_stop)
                && let Ok((preview_image, preview_texture)) = self.generate_preview(ui)
            {
                self.preview_texture = Some(preview_texture);
                self.preview_image = Some(preview_image);
            }
        });
    }

    /// UI elements for utilities that are displayed beneath the original image
    fn utility_elements(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                self.shape_selection(ui);
                ui.label("Border width percentage:");
                ui.add(Slider::new(&mut self.border_percent, 0.0..=MAX_BORDER_PERCENT).drag_value_speed(1.0));
                ui.checkbox(&mut self.resize, RichText::new(format!("Resize to {:>4} x {:>4} pixels", self.resize_size, self.resize_size)).monospace());
                ui.add_enabled(self.resize, Slider::new(&mut self.resize_size, 100..=1000));
                self.border_coloring_selection(ui);
                ui.horizontal(|ui| {
                    ui.label("Gradient angle:");
                    ui.add_enabled(self.color_kind == ColorKind::Gradient, Slider::new(&mut self.gradient_angle_deg, 0.0..=360.0).drag_value_speed(0.1));
                });
                if !(self.drag_start.is_some() && self.drag_stop.is_some()) {
                    // disables the elements that follow this point on until the "vertical" group
                    // ends
                    ui.disable();
                }
                let generate_response = ui.button("Generate avatar");
                if generate_response.clicked() {
                    match self.generate_avatar() {
                        Err(err) => {
                            let err_str = err.to_string();
                            debug!("Generation failed: {}", err_str);
                            self.error_message = Some(err_str);
                            self.error_popup_open = true;
                        }
                        Ok(()) => {
                            debug!("Generation successful.");
                        }
                    }
                }

                Popup::from_response(&generate_response).open(self.error_popup_open).show(|ui| {
                    ui.set_min_width(200.0);
                    ui.label("Failed to generate avatar. Reason:");
                    if let Some(msg) = &self.error_message {
                        ui.label(msg);
                    }
                });
            });
            ui.vertical(|ui| {
                ui.label("Pre-defined colours:");
                if ui.button("Crimson").clicked() {
                    self.border_colour = Color32::from_rgb(158, 27, 50);
                    debug!("Changed border colour to: {:?}", self.border_colour);
                }
                if ui.button("Amber").clicked() {
                    self.border_colour = Color32::from_rgb(255, 192, 0);
                    debug!("Changed border colour to: {:?}", self.border_colour);
                }
                if ui.button("Malachite").clicked() {
                    self.border_colour = Color32::from_rgb(11, 218, 81);
                    debug!("Changed border colour to: {:?}", self.border_colour);
                }
                self.gradient_colour_selector(ui);
            });
            self.border_colour_element(ui);
        });
    }

    fn border_colour_element(&mut self, ui: &mut Ui) {
        ui.vertical(|ui| {
            ui.label("Border colour:");
            ui.scope(|ui| {
                ui.spacing_mut().slider_width = 100.0;
                egui::widgets::color_picker::color_picker_color32(ui, &mut self.border_colour, egui::color_picker::Alpha::Opaque);
                // write colours into fields if necessary
                if self.color_kind == ColorKind::Gradient {
                    match self.gradient_change {
                        GradientChange::First => {
                            self.gradient_first_colour = self.border_colour;
                        }
                        GradientChange::Last => {
                            self.gradient_last_colour = self.border_colour;
                        }
                    }
                }
            })
        });
    }

    fn gradient_colour_selector(&mut self, ui: &mut Ui) {
        if self.color_kind == ColorKind::Gradient {
            ui.vertical(|ui| {
                if ui.radio_value(&mut self.gradient_change, GradientChange::First, "First colour").clicked() {
                    self.border_colour = self.gradient_first_colour;
                }
                if ui.radio_value(&mut self.gradient_change, GradientChange::Last, "Second colour").clicked() {
                    self.border_colour = self.gradient_last_colour;
                }
            });
        }
    }

    /// ensures that the image data is loaded when an image is selected, so it
    /// can be displayed
    pub(crate) fn load_image_to_state(&mut self, image_path: &Path) {
        self.filepath = Some(image_path.to_path_buf());
        let image_path = image_path.to_str().expect("Image path is UTF-8 string");
        debug!("Loading image: {image_path}");
        let image_dimensions = ImageReader::open(image_path).expect("Image is readable").into_dimensions().expect("Image dimensions exist");
        debug!("Image has dimensions: [{}, {}]", image_dimensions.0, image_dimensions.1);
        self.original_image_dimension = Some(Vec2::new(image_dimensions.0 as f32, image_dimensions.1 as f32));
        self.image = Some(Image::new(format!("file://{image_path}")));
    }

    /// UI element to open the file and load the data
    fn file_opening(&mut self, ui: &mut Ui) {
        if ui.button("Load Image").clicked()
            && let Some(image_path) = FileDialog::new().add_filter("Supported image formats", SUPPORTED_IMAGE_FORMATS).pick_file()
        {
            self.load_image_to_state(&image_path);
            // TODO: Windows file handling
        }
    }

    /// UI element to reset the selection
    fn selection_reset(&mut self, ui: &mut Ui) {
        if ui.add_enabled(self.drag_start.is_some() && self.drag_stop.is_some(), egui::Button::new("Reset selection")).clicked() {
            self.drag_start = None;
            self.drag_stop = None;
            self.preview_texture = None;
            debug!("Reset selection");
        }
    }

    /// UI element that allows selecting between different shapes for the avatar
    fn shape_selection(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Avatar shape:");
            ui.radio_value(&mut self.shape, Shape::Square, "Square");
            ui.radio_value(&mut self.shape, Shape::Circle, "Circle");
        });
    }

    /// UI element to select the border type
    fn border_coloring_selection(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.label("Border colour type:");
            ui.radio_value(&mut self.color_kind, ColorKind::SingleColour, "Single colour");
            ui.radio_value(&mut self.color_kind, ColorKind::Gradient, "Gradient");
        });
    }
}

#[inline]
/// Linear interpolation between two RGBA values
fn lerp(first: Rgba<u8>, second: Rgba<u8>, pos: f32) -> Rgba<u8> {
    if first == second {
        return first;
    }
    let pos = pos.clamp(0.0, 1.0);
    let red = (f32::from(first[0]) * pos) + (f32::from(second[0]) * (1.0 - pos));
    let green = (f32::from(first[1]) * pos) + (f32::from(second[1]) * (1.0 - pos));
    let blue = (f32::from(first[2]) * pos) + (f32::from(second[2]) * (1.0 - pos));
    let alpha = (f32::from(first[3]) * pos) + (f32::from(second[3]) * (1.0 - pos));
    Rgba([red as u8, green as u8, blue as u8, alpha as u8])
}
