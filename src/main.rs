use {
    eframe::egui::{self, CentralPanel, Context, CornerRadius, Image, Pos2, Rect, RichText, Sense, Slider, Vec2},
    image::{ImageReader, imageops::resize, ImageBuffer, Rgba, RgbaImage},
    rfd::FileDialog,
    std::path::PathBuf,
    thiserror::Error,
    tracing::{error, info},
    tracing_subscriber::{EnvFilter, fmt, layer::SubscriberExt, util::SubscriberInitExt},
};

const APPLICATION_NAME: &str = "Avatar creator";
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Default window width
const UI_WINDOW_WIDTH: f32 = 1280.0;
/// Default window height
const UI_WINDOW_HEIGHT: f32 = 900.0;
/// The height of all UI elements that are not the image.
///
/// Includes all labels, buttons and colour picker
const TOTAL_UI_ELEMENT_HEIGHT: f32 = 250.0;

#[derive(Error, Debug)]
enum Error {
    #[error("Missing selection")]
    MissingSelection,
}

pub struct ImageManipulatorApp<'a> {
    filepath: Option<PathBuf>,
    drag_start: Option<Pos2>,
    drag_stop: Option<Pos2>,
    border_size: f32,
    border_colour: egui::Color32,
    original_image_dimension: Option<Vec2>,
    scaled_image_dimension: Option<Vec2>,
    image: Option<Image<'a>>,
    resize: bool,
    resize_size: u16,
    preview_texture: Option<egui::TextureHandle>,
    preview_image: Option<ImageBuffer<Rgba<u8>, Vec<u8>>>,
    feather_border: bool
}

impl Default for ImageManipulatorApp<'_> {
    fn default() -> Self {
        Self {
            filepath: None,
            drag_start: None,
            drag_stop: None,
            border_size: 0.0,
            border_colour: egui::Color32::WHITE,
            original_image_dimension: None,
            scaled_image_dimension: None,
            image: None,
            resize: false,
            resize_size: 400,
            preview_texture: None,
            preview_image: None,
            feather_border: true
        }
    }
}

impl eframe::App for ImageManipulatorApp<'_> {
    #[allow(clippy::cognitive_complexity, clippy::too_many_lines)]
    fn update(&mut self, ctx: &Context, _: &mut eframe::Frame) {
        CentralPanel::default().show(ctx, |ui| {
            let window_dimensions = ctx.input(|i: &egui::InputState| i.screen_rect()).size();
            ui.horizontal(|ui| {
                if ui.button("Load Image").clicked() {
                    if let Some(image_path) = FileDialog::new().add_filter("Image", &["png", "jpg", "jpeg", "bmp"]).pick_file() {
                        self.filepath = Some(image_path.clone());
                        let image_path = image_path.to_str().expect("Image path is UTF-8 string");
                        let image_dimensions = ImageReader::open(image_path).expect("Image is readable").into_dimensions().expect("Image dimensions exist");
                        self.original_image_dimension = Some(Vec2::new(image_dimensions.0 as f32, image_dimensions.1 as f32));
                        self.image = Some(Image::new(format!("file://{image_path}")));
                    }
                }
                if ui.add_enabled(self.drag_start.is_some() && self.drag_stop.is_some(), egui::Button::new("Reset selection")).clicked() {
                    self.drag_start = None;
                    self.drag_stop = None;
                }
            });

            if self.filepath.is_some() {
                ui.horizontal(|ui| {
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
                        // Generate start point when new selection is created by left-clicking.
                        // If clicked on the previous selection, the old starting point is kept.
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

                        // Moving selection when using the right button
                        if response.dragged_by(egui::PointerButton::Secondary) {
                            if let (Some(start), Some(stop)) = (self.drag_start.as_mut(), self.drag_stop.as_mut()) {
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
                                painter.circle(circle_center, radius, egui::Color32::TRANSPARENT, egui::Stroke::new(1.0, egui::Color32::RED));

                                // offset the circle's position by the second image's top-left corner
                                let center_to_outside_border = start.distance(stop);
                                let diameter = center_to_outside_border * 2.0;
                                let bounding_box = Rect::from_center_size(circle_center, Vec2 { x: diameter + 2.0 * self.border_size, y: diameter + 2.0 * self.border_size });
                                painter.rect(bounding_box, CornerRadius::ZERO, egui::Color32::TRANSPARENT, egui::Stroke::new(1.0, egui::Color32::GREEN), egui::StrokeKind::Outside);
                                painter.circle(circle_center, center_to_outside_border, egui::Color32::TRANSPARENT, egui::Stroke::new(self.border_size, self.border_colour));
                            }
                            (Some(start), None) => {
                                let painter = ui.painter_at(response.rect);

                                // Offset the circle's position by the second image's top-left corner
                                let circle_center = response.rect.left_top() + Vec2 { x: start.x, y: start.y };
                                let radius = 1.0;

                                painter.circle(circle_center, radius, egui::Color32::TRANSPARENT, egui::Stroke::new(1.0, egui::Color32::RED));
                            }
                            (None, _) => {}
                        }

                        // After handling the drag and selection, generate a preview
                        if let (Some(_start), Some(_stop)) = (self.drag_start, self.drag_stop) {
                            // Generate preview image
                            if let Ok((preview_image, preview_texture)) = self.generate_preview(ctx) {
                                self.preview_texture = Some(preview_texture);
                                self.preview_image = Some(preview_image);
                            }
                        }
                    });

                    // Preview panel
                    if let Some(texture) = &self.preview_texture {
                        ui.vertical(|ui| {
                            ui.label("Preview:");
                            ui.image(texture);
                        });
                    }

                });
                ui.horizontal(|ui| {
                    ui.vertical(|ui| {
                        ui.label("Border width:");
                        ui.add(Slider::new(&mut self.border_size, 0.0..=40.0).drag_value_speed(1.0));
                        ui.checkbox(&mut self.feather_border, "Feather boarder");
                        ui.checkbox(&mut self.resize, RichText::new(format!("Resize to {:>4} x {:>4} pixels", self.resize_size, self.resize_size)).monospace());
                        ui.add_enabled(self.resize, Slider::new(&mut self.resize_size, 100..=1000));
                        if ui.add_enabled(self.drag_start.is_some() && self.drag_stop.is_some(), egui::Button::new("Generate avatar")).clicked() {
                            let _ = self.generate_avatar();
                        }
                    });
                    ui.vertical(|ui| {
                        ui.label("Border colour:");
                        ui.scope(|ui| {
                            ui.spacing_mut().slider_width = 100.0;
                            egui::widgets::color_picker::color_picker_color32(ui, &mut self.border_colour, egui::color_picker::Alpha::Opaque);
                        })
                    });
                });
            }
        });
    }
}

impl ImageManipulatorApp<'_> {
    fn generate_avatar(&self) -> Result<(), Box<dyn std::error::Error>> {
        info!("Generating avatar...");

        let Some(image) = &self.preview_image else {
                error!("No preview available!");
                return Err(Box::new(Error::MissingSelection));
            };

        // resize if needed
        let final_image = if self.resize {
            resize(image,
                   self.resize_size.into(),
                   self.resize_size.into(),
                   image::imageops::FilterType::Lanczos3)
        } else {
            image.clone()
        };

        match final_image.save(self.generate_final_file_name()) {
            Ok(()) => {
                info!("Generation successful!");
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
        let resize_text = if self.resize {
            format!("_resized_{}", self.resize_size)
        } else {
            String::new()
        };

        format!("{filename}{resize_text}.cropped.png")
    }

    fn create_border(&self, cropped: &RgbaImage, inner_radius: f32, width: u32, height: u32, center: Pos2) -> ImageBuffer<Rgba<u8>, Vec<u8>> {
        let radius_squared = inner_radius.powi(2);
        let outer_radius_squared = (inner_radius + self.border_size).powi(2);
        let mut masked = cropped.clone();
        for y in 0..height {
            for x in 0..width {
                let pixel_pos = Pos2::new(x as f32, y as f32);
                let distance_squared = (pixel_pos.x - center.x).powi(2) + (pixel_pos.y - center.y).powi(2);

                let pixel = masked.get_pixel_mut(x, y);
                if distance_squared <= radius_squared {
                    pixel[3] = 255;
                } else if distance_squared <= outer_radius_squared {
                    let rgba = self.border_colour.to_array();
                    pixel[0] = rgba[0];
                    pixel[1] = rgba[1];
                    pixel[2] = rgba[2];
                    // inside the outer boundary, apply antialiasing
                    if self.feather_border {
                        let distance = distance_squared.sqrt();
                        let normalized_distance = (distance - inner_radius) / (inner_radius + self.border_size - inner_radius);
                        let alpha = (1.0 - normalized_distance).clamp(0.0, 1.0) * 255.0;
                        // set border colour with alpha blending
                        pixel[3] = alpha as u8;
                    } else {
                        pixel[3] = 255;
                    }
                } else {
                    pixel[3] = 0;
                }
            }
        }
        masked
    }

    fn get_points(&self) -> Result<(Pos2, Pos2), Error> {
        if let (Some(start), Some(stop)) = (self.drag_start, self.drag_stop) { Ok((start, stop)) } else {
            error!("No selection made, can not generate avatar!");
            Err(Error::MissingSelection)
        }
    }

    #[allow(clippy::type_complexity)]
    fn generate_preview(&self, ctx: &Context) -> Result<(ImageBuffer<Rgba<u8>, Vec<u8>>,
                                                         egui::TextureHandle), Box<dyn std::error::Error>> {
        let mut tmp_image = ImageReader::open(self.filepath.as_ref().expect("File path is readable"))?.decode()?;

        let scale_factor = self.original_image_dimension.expect("Dimensions have been set").x
            / self.scaled_image_dimension.expect("Dimensions have been set").x;
        let (start, stop) = self.get_points()?;
        let (start, stop) = (start * scale_factor, stop * scale_factor);

        // calculate bounding box from points and border
        let inner_radius = start.distance(stop);
        let inner_diameter = inner_radius * 2.0;
        let bounding_box = Rect::from_center_size(start, Vec2 { x: inner_diameter + 2.0 * self.border_size, y: inner_diameter + 2.0 * self.border_size});

        // crop out image depending on the bounding box
        let crop_x = bounding_box.left_top().x.max(0.0).min(tmp_image.width() as f32) as u32;
        let crop_y = bounding_box.left_top().y.max(0.0).min(tmp_image.height() as f32) as u32;
        let crop_width = bounding_box.width().min(tmp_image.width() as f32) as u32;
        let crop_height = bounding_box.height().min(tmp_image.height() as f32) as u32;
        let cropped = tmp_image.crop(crop_x, crop_y, crop_width, crop_height).into_rgba8();

        let (new_width, new_height) = cropped.dimensions();
        let center = Pos2::new((new_width / 2) as f32, (new_height / 2) as f32);

        let masked = self.create_border(&cropped, inner_radius, new_width, new_height, center);

        let window_dimensions = ctx.input(|i: &egui::InputState| i.screen_rect()).size();
        let preview_size = (0.5 * (window_dimensions.y - TOTAL_UI_ELEMENT_HEIGHT))  as u32; // match the size used in the UI
        let final_image: ImageBuffer<Rgba<u8>, Vec<u8>> = resize(&masked, preview_size, preview_size,
                                 image::imageops::FilterType::Lanczos3);

        let pixels = final_image.as_raw();
        let texture = ctx.load_texture(
            "preview",
            egui::ColorImage::from_rgba_unmultiplied(
                [preview_size as usize, preview_size as usize],
                pixels,
            ),
            egui::TextureOptions::default(),
        );

        Ok((final_image, texture))
    }

}

fn main() -> eframe::Result<()> {
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let custom_format = fmt::format().with_target(false).with_ansi(true).with_file(true).with_level(true).with_line_number(true).compact();
    let fmt_layer = fmt::layer().event_format(custom_format);
    tracing_subscriber::registry().with(env_filter).with(fmt_layer).init();

    let options =
        eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([UI_WINDOW_WIDTH, UI_WINDOW_HEIGHT]).with_resizable(true).with_maximize_button(true), ..Default::default() };

    info!("Starting application [Version: {VERSION}]");
    eframe::run_native(
        APPLICATION_NAME,
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(Box::<ImageManipulatorApp>::default())
        }),
    )

}
