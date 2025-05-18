use {
    crate::SUPPORTED_IMAGE_FORMATS,
    eframe::{
        egui,
        egui::{CentralPanel, Color32, Context, CornerRadius, Image, Pos2, Rect, RichText, Sense, Slider, Stroke, StrokeKind, Ui, Vec2},
    },
    image::{ImageBuffer, ImageReader, Rgba, RgbaImage, imageops::resize},
    rfd::FileDialog,
    std::path::{Path, PathBuf},
    thiserror::Error,
    tracing::{debug, error, info},
};

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
    filepath: Option<PathBuf>,
    drag_start: Option<Pos2>,
    drag_stop: Option<Pos2>,
    border_percent: f32,
    border_colour: Color32,
    original_image_dimension: Option<Vec2>,
    scaled_image_dimension: Option<Vec2>,
    image: Option<Image<'a>>,
    resize: bool,
    resize_size: u16,
    preview_texture: Option<egui::TextureHandle>,
    preview_image: Option<ImageBuffer<Rgba<u8>, Vec<u8>>>,
    shape: Shape,
    color_kind: ColorKind,
    gradient_angle_deg: f32,
    gradient_first_colour: Color32,
    gradient_last_colour: Color32,
    gradient_change: GradientChange,
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
            filepath: None,
            drag_start: None,
            drag_stop: None,
            border_percent: BORDER_DEFAULT_PERCENT,
            border_colour: Color32::WHITE,
            original_image_dimension: None,
            scaled_image_dimension: None,
            image: None,
            resize: true,
            resize_size: 400,
            preview_texture: None,
            preview_image: None,
            shape: Shape::Circle,
            color_kind: ColorKind::SingleColour,
            gradient_angle_deg: 0.0,
            gradient_first_colour: Color32::WHITE,
            gradient_last_colour: Color32::WHITE,
            gradient_change: GradientChange::First,
        }
    }
}

impl eframe::App for AvatarCreatorApp<'_> {
    #[allow(clippy::cognitive_complexity)]
    fn update(&mut self, ctx: &Context, _: &mut eframe::Frame) {
        CentralPanel::default().show(ctx, |ui| {
            let window_dimensions = ctx.input(|i: &egui::InputState| i.screen_rect()).size();
            ui.horizontal(|ui| {
                self.file_opening(ui);
                self.selection_reset(ui);
            });

            if self.filepath.is_some() {
                ui.horizontal(|ui| {
                    // treat image and settings as one element
                    ui.vertical(|ui| {
                        // main image
                        self.main_display_panel(ctx, window_dimensions, ui);
                        // utils
                        self.utility_elements(ui);
                    });
                    // preview panel
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

    /// creates the circle border around the selection made
    fn create_circle_border(&self, cropped: &RgbaImage, inner_radius: f32) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        let (width, height) = cropped.dimensions();
        let inner_radius_squared = inner_radius.powi(2);
        let border_size = (self.border_percent / 100.0) * inner_radius;
        let outer_radius_squared = (inner_radius + border_size).powi(2);
        let center = Pos2::new((width / 2) as f32, (height / 2) as f32);
        let mut masked = cropped.clone();

        // normalize angle to 0-360 degree and convert to radians
        let angle_rad = (self.gradient_angle_deg % 360.0) * std::f32::consts::PI / 180.0;
        let dir_x = angle_rad.cos();
        let dir_y = angle_rad.sin();

        // compute bounding box of the circle
        let radius_plus_border = inner_radius + border_size;
        let circle_min_x = center.x - radius_plus_border;
        let circle_max_x = center.x + radius_plus_border;
        let circle_min_y = center.y - radius_plus_border;
        let circle_max_y = center.y + radius_plus_border;

        // calculate projection extremes for normalization
        let corners = [Pos2::new(circle_min_x, circle_min_y), Pos2::new(circle_max_x, circle_min_y), Pos2::new(circle_max_x, circle_max_y), Pos2::new(circle_min_x, circle_max_y)];

        // find min and max values to normalise gradient positions
        let mut min_proj = f32::MAX;
        let mut max_proj = f32::MIN;

        for corner in &corners {
            // project the point onto the gradient direction
            let proj = (corner.x - center.x) * dir_x + (corner.y - center.y) * dir_y;
            min_proj = min_proj.min(proj);
            max_proj = max_proj.max(proj);
        }

        let proj_range = max_proj - min_proj;

        for y in 0..height {
            for x in 0..width {
                let pixel_pos = Pos2::new(x as f32, y as f32);
                let distance_squared = (pixel_pos.x - center.x).powi(2) + (pixel_pos.y - center.y).powi(2);

                let pixel = masked.get_pixel_mut(x, y);
                if distance_squared <= inner_radius_squared {
                    // inside the inner circle - keep the original pixel values
                    pixel[3] = 255;
                } else if distance_squared <= outer_radius_squared {
                    // inside the border area - apply gradient

                    // project the current point onto the gradient direction
                    let proj = (pixel_pos.x - center.x) * dir_x + (pixel_pos.y - center.y) * dir_y;

                    // normalize to 0.0-1.0 range
                    let gradient_pos = if proj_range > 0.0 {
                        (proj - min_proj) / proj_range
                    } else {
                        0.5 // fallback
                    };

                    // apply gradient between start and end colours
                    let (first_colour, last_colour) = match self.color_kind {
                        ColorKind::SingleColour => {
                            (self.border_colour, self.border_colour)
                        }
                        ColorKind::Gradient => {
                            (self.gradient_first_colour, self.gradient_last_colour)
                        }
                    };
                    
                    let first_colour_rgba = Rgba::from(first_colour.to_srgba_unmultiplied());
                    let last_colour_rgba = Rgba::from(last_colour.to_srgba_unmultiplied());
                    let color = lerp(first_colour_rgba, last_colour_rgba, gradient_pos);
                    // our colour selection only allows RGB values, so we can use 255 here.
                    pixel.clone_from(&Rgba([color[0], color[1], color[2], 255]));
                } else {
                    // outside the outer border - transparent
                    pixel[3] = 0;
                }
            }
        }
        masked
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
    /// Generates the preview used in the active window to be display, as a texture.
    ///
    /// It also provides the resulting image as an `ImageBuffer`, ready to be saved.
    fn generate_preview(&self, ctx: &Context) -> Result<(ImageBuffer<Rgba<u8>, Vec<u8>>, egui::TextureHandle), Box<dyn std::error::Error>> {
        let src_image = ImageReader::open(self.filepath.as_ref().expect("File path is set"))?.decode()?;

        // selection will be square; therefore, we only care about one part for the scaling
        let scale_factor = self.original_image_dimension.expect("Dimensions have been set").x / self.scaled_image_dimension.expect("Dimensions have been set").x;
        let (start, stop) = self.try_get_points()?;
        let (scaled_start, scaled_stop) = (start * scale_factor, stop * scale_factor);

        // calculate bounding box from points and border
        let inner_radius = scaled_start.distance(scaled_stop);
        let border_size = (self.border_percent / 100.0) * inner_radius;
        let bounding_box = Rect::from_center_size(scaled_start, Vec2 { x: (inner_radius + border_size) * 2.0, y: (inner_radius + border_size) * 2.0 });

        // crop out image depending on the bounding box
        let crop_x = bounding_box.left_top().x.max(0.0).min(src_image.width() as f32) as u32;
        let crop_y = bounding_box.left_top().y.max(0.0).min(src_image.height() as f32) as u32;
        let crop_width = bounding_box.width().min(src_image.width() as f32) as u32;
        let crop_height = bounding_box.height().min(src_image.height() as f32) as u32;
        let cropped_image = src_image.crop_imm(crop_x, crop_y, crop_width, crop_height).into_rgba8();

        let cropped_masked_image = match self.shape {
            Shape::Circle => self.create_circle_border(&cropped_image, inner_radius),
            Shape::Square => cropped_image,
        };

        let window_dimensions = ctx.input(|i: &egui::InputState| i.screen_rect()).size();
        let preview_size =
            if self.resize { u32::from(((0.5 * (window_dimensions.y - TOTAL_UI_ELEMENT_HEIGHT)) as u16).max((0.5 * window_dimensions.x) as u16).min(self.resize_size)) } else { crop_width };

        let final_image: ImageBuffer<Rgba<u8>, Vec<u8>> = resize(&cropped_masked_image, preview_size, preview_size, image::imageops::FilterType::Lanczos3);

        let pixels = final_image.as_raw();
        let texture = ctx.load_texture("preview", egui::ColorImage::from_rgba_unmultiplied([preview_size as usize, preview_size as usize], pixels), egui::TextureOptions::default());

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
    fn main_display_panel(&mut self, ctx: &Context, window_dimensions: Vec2, ui: &mut Ui) {
        ui.vertical(|ui| {
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
            if response.dragged_by(egui::PointerButton::Secondary) {
                if let (Some(start), Some(stop)) = (self.drag_start.as_mut(), self.drag_stop.as_mut())
                {
                    *start += response.drag_delta();
                    *stop += response.drag_delta();
                }
            }

            match (self.drag_start, self.drag_stop) {
                (Some(start), Some(stop)) => {
                    let painter = ui.painter_at(response.rect);

                    // centre marker
                    let radius = 1.0;
                    let circle_center = response.rect.left_top() + Vec2 { x: start.x, y: start.y };
                    painter.circle(circle_center, radius, Color32::TRANSPARENT, Stroke::new(1.0, Color32::RED));

                    // offset the circle's position by the second image's top-left corner
                    let center_to_inside_border = start.distance(stop);
                    let bounding_box =
                        Rect::from_center_size(circle_center, Vec2 { x: (center_to_inside_border + self.border_percent) * 2.0, y: (center_to_inside_border + self.border_percent) * 2.0 });
                    painter.rect(bounding_box, CornerRadius::ZERO, Color32::TRANSPARENT, Stroke::new(1.0, Color32::GREEN), StrokeKind::Outside);
                    if self.shape == Shape::Circle {
                        painter.circle(circle_center, center_to_inside_border, Color32::TRANSPARENT, Stroke::new(self.border_percent, self.border_colour));
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
            if let (Some(_start), Some(_stop)) = (self.drag_start, self.drag_stop) {
                if let Ok((preview_image, preview_texture)) = self.generate_preview(ctx)
                {
                    self.preview_texture = Some(preview_texture);
                    self.preview_image = Some(preview_image);
                }
            }
              
        });
    }

    /// UI elements for utilities that are displayed beneath the original image
    fn utility_elements(&mut self, ui: &mut Ui) {
        ui.horizontal(|ui| {
            ui.vertical(|ui| {
                self.shape_selection(ui);
                ui.label("Border width percentage:");
                ui.add_enabled(self.shape == Shape::Circle, Slider::new(&mut self.border_percent, 0.0..=MAX_BORDER_PERCENT).drag_value_speed(1.0));
                ui.checkbox(&mut self.resize, RichText::new(format!("Resize to {:>4} x {:>4} pixels", self.resize_size, self.resize_size)).monospace());
                ui.add_enabled(self.resize, Slider::new(&mut self.resize_size, 100..=1000));
                self.border_coloring_selection(ui);
                ui.horizontal(|ui| {
                    ui.label("Gradient angle:");
                    ui.add_enabled(self.color_kind == ColorKind::Gradient, Slider::new(&mut self.gradient_angle_deg, 0.0..=360.0).drag_value_speed(0.1));
                });
                if !(self.drag_start.is_some() && self.drag_stop.is_some()) {
                    // disables the elements that follow this point on until the "vertical" group ends
                    ui.disable();
                }
                let generate_response = ui.button("Generate avatar");
                if generate_response.clicked() {
                    match self.generate_avatar() {
                        Err(err) => {
                            debug!("Generation failed: {err}");
                            let generation_failed_popup = ui.make_persistent_id("failed_popup_id");
                            ui.memory_mut(|mem| mem.toggle_popup(generation_failed_popup));
                            egui::popup::popup_above_or_below_widget(
                                ui,
                                generation_failed_popup,
                                &generate_response,
                                egui::AboveOrBelow::Below,
                                egui::popup::PopupCloseBehavior::CloseOnClickOutside,
                                |ui| {
                                    ui.set_min_width(200.0); // if you want to control the size
                                    ui.label("Failed to generate avatar. Reason:");
                                    ui.label(format!("{err}"));
                                },
                            );
                        }
                        Ok(()) => {
                            debug!("Generation successful.");
                        }
                    }
                }
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

    /// ensures that the image data is loaded when an image is selected, so it can be displayed
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
        if ui.button("Load Image").clicked() {
            if let Some(image_path) = FileDialog::new().add_filter("Supported image formats", SUPPORTED_IMAGE_FORMATS).pick_file()
            {
                self.load_image_to_state(&image_path);
                // TODO: Windows file handling
            }
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
            if ui.radio_value(&mut self.shape, Shape::Square, "Square").clicked() {
                self.border_percent = 0.0;
            }
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
