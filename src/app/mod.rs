use std::path::{Path, PathBuf};

use eframe::egui::{self, Color32, Image, Pos2, Vec2};
use image::imageops::{crop_imm, resize};
use image::{ImageBuffer, ImageError, ImageReader, Rgba, RgbaImage};
use png::text_metadata::ITXtChunk;
use thiserror::Error;
use tracing::{debug, info};

use metadata::{
    PNG_METADATA_KEYWORD, SavedAvatar, lerp_rgba, opaque_colour, rainbow_colour,
    selection_metadata_payload,
};

mod metadata;
mod ui;

/// Default border size (in percent of the selection radius)
const BORDER_DEFAULT_PERCENT: u8 = 10;
const MAX_BORDER_PERCENT: u8 = 50;
const RESIZE_FILTER: image::imageops::FilterType = image::imageops::FilterType::Lanczos3;

#[derive(Debug, Error)]
pub(crate) enum AppError {
    #[error("missing selection")]
    MissingSelection,
    #[error("no image loaded")]
    MissingFilePath,
    #[error("failed to read image `{path}`")]
    ImageRead {
        path: PathBuf,
        #[source]
        source: ImageError,
    },
    #[error("failed to write file `{path}`")]
    Io {
        path: PathBuf,
        #[source]
        source: std::io::Error,
    },
    #[error("failed to encode avatar `{path}`")]
    PngEncode {
        path: PathBuf,
        #[source]
        source: png::EncodingError,
    },
}

/// detect which changes require a preview regeneration.
#[derive(PartialEq)]
struct PreviewInputs {
    drag_start: Option<Pos2>,
    drag_stop: Option<Pos2>,
    border_percent: u8,
    shape: Shape,
    colours: BorderColours,
    resize: bool,
    resize_size: u16,
}

pub struct AvatarCreatorApp {
    filepath: Option<PathBuf>,
    /// Decoded source image, cached so the preview never re-reads from disk.
    source_image: Option<RgbaImage>,
    original_image_dimension: Option<Vec2>,
    image: Option<Image<'static>>,
    /// Selection centre, in original image pixels.
    drag_start: Option<Pos2>,
    /// Selection edge, in original image pixels; its distance to
    /// `drag_start` is the selection radius.
    drag_stop: Option<Pos2>,
    border_percent: u8,
    shape: Shape,
    colours: BorderColours,
    gradient_change: GradientChange,
    resize: bool,
    resize_size: u16,
    preview_texture: Option<egui::TextureHandle>,
    preview_image: Option<RgbaImage>,
    error_message: Option<String>,
}

#[derive(Clone, Copy, PartialEq)]
enum Shape {
    Square,
    Circle,
}

/// Border colour configuration: a solid colour, a two-colour gradient, or a
/// full-spectrum rainbow sweep.
#[derive(Clone, Copy, Debug, PartialEq)]
enum BorderColours {
    Single(Color32),
    Gradient {
        angle_deg: f32,
        first: Color32,
        last: Color32,
    },
    Rainbow {
        angle_deg: f32,
    },
}

impl BorderColours {
    fn is_single(self) -> bool {
        matches!(self, Self::Single(_))
    }

    fn is_gradient(self) -> bool {
        matches!(self, Self::Gradient { .. })
    }

    fn is_rainbow(self) -> bool {
        matches!(self, Self::Rainbow { .. })
    }

    fn angle_deg(self) -> f32 {
        match self {
            Self::Single(_) => 0.0,
            Self::Gradient { angle_deg, .. } | Self::Rainbow { angle_deg } => angle_deg,
        }
    }

    fn set_angle_deg(&mut self, new_angle_deg: f32) {
        match self {
            Self::Single(_) => {}
            Self::Gradient { angle_deg, .. } | Self::Rainbow { angle_deg } => {
                *angle_deg = new_angle_deg;
            }
        }
    }

    fn picked_colour(self, change: GradientChange) -> Color32 {
        match self {
            Self::Single(colour) => colour,
            Self::Gradient { first, last, .. } => match change {
                GradientChange::First => first,
                GradientChange::Last => last,
            },
            Self::Rainbow { .. } => Color32::WHITE,
        }
    }

    fn set_picked(&mut self, colour: Color32, change: GradientChange) {
        match self {
            Self::Single(picked) => *picked = colour,
            Self::Gradient { first, last, .. } => match change {
                GradientChange::First => *first = colour,
                GradientChange::Last => *last = colour,
            },
            Self::Rainbow { .. } => {}
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
enum GradientChange {
    First,
    Last,
}

impl Default for AvatarCreatorApp {
    fn default() -> Self {
        Self {
            filepath: None,
            source_image: None,
            original_image_dimension: None,
            image: None,
            drag_start: None,
            drag_stop: None,
            border_percent: BORDER_DEFAULT_PERCENT,
            shape: Shape::Circle,
            colours: BorderColours::Single(Color32::WHITE),
            gradient_change: GradientChange::First,
            resize: true,
            resize_size: 400,
            preview_texture: None,
            preview_image: None,
            error_message: None,
        }
    }
}
impl AvatarCreatorApp {
    fn preview_inputs(&self) -> PreviewInputs {
        PreviewInputs {
            drag_start: self.drag_start,
            drag_stop: self.drag_stop,
            border_percent: self.border_percent,
            shape: self.shape,
            colours: self.colours,
            resize: self.resize,
            resize_size: self.resize_size,
        }
    }

    fn selection_complete(&self) -> bool {
        self.drag_start.is_some() && self.drag_stop.is_some()
    }

    fn preview_outdated(&self, inputs_before: &PreviewInputs) -> bool {
        self.preview_inputs() != *inputs_before
            || (self.selection_complete() && self.preview_image.is_none())
    }

    fn generate_masked_avatar(&self) -> Option<RgbaImage> {
        let source = self.source_image.as_ref()?;
        let start = self.drag_start?;
        let stop = self.drag_stop?;

        let inner_radius = start.distance(stop);
        if inner_radius < 0.5 {
            return None;
        }
        let border_size = (f32::from(self.border_percent) / 100.0) * inner_radius;
        let content_radius = inner_radius - border_size;

        let (crop_left, crop_right) = clamped_range(
            start.x - inner_radius,
            start.x + inner_radius,
            source.width(),
        );
        let (crop_top, crop_bottom) = clamped_range(
            start.y - inner_radius,
            start.y + inner_radius,
            source.height(),
        );
        let cropped = crop_imm(
            source,
            crop_left,
            crop_top,
            crop_right - crop_left,
            crop_bottom - crop_top,
        )
        .to_image();

        Some(self.render_avatar(&cropped, inner_radius, content_radius))
    }

    fn render_avatar(
        &self,
        cropped: &RgbaImage,
        inner_radius: f32,
        content_radius: f32,
    ) -> RgbaImage {
        let final_size = (2.0 * inner_radius).ceil().max(1.0) as u32;
        let mut masked: RgbaImage = ImageBuffer::new(final_size, final_size);
        let final_center = Pos2::new(final_size as f32 / 2.0, final_size as f32 / 2.0);

        if content_radius <= 0.0 && !self.colours.is_rainbow() {
            let colour = self.border_fill_colour();
            for pixel in masked.pixels_mut() {
                pixel.clone_from(&colour);
            }
            return masked;
        }

        let content_pixel_size = (2.0 * content_radius).ceil().max(1.0) as u32;
        let scaled_content = resize(
            cropped,
            content_pixel_size,
            content_pixel_size,
            RESIZE_FILTER,
        );
        let content_offset = (final_size as f32 - content_pixel_size as f32) / 2.0;

        let angle_rad = (self.colours.angle_deg() % 360.0) * std::f32::consts::PI / 180.0;
        let (angle_sin, angle_cos) = angle_rad.sin_cos();
        let direction = Vec2::new(angle_cos, angle_sin);
        let (min_projection, max_projection) =
            Self::gradient_projection_bounds(self.shape, inner_radius, direction);
        let projection_range = max_projection - min_projection;

        for y in 0..final_size {
            for x in 0..final_size {
                let pixel_x = x as f32 + 0.5;
                let pixel_y = y as f32 + 0.5;
                let dx = pixel_x - final_center.x;
                let dy = pixel_y - final_center.y;
                let (in_content, in_border) = match self.shape {
                    Shape::Square => (
                        dx.abs() <= content_radius && dy.abs() <= content_radius,
                        dx.abs() <= inner_radius && dy.abs() <= inner_radius,
                    ),
                    Shape::Circle => {
                        let distance_sq = dx * dx + dy * dy;
                        (
                            distance_sq <= content_radius * content_radius,
                            distance_sq <= inner_radius * inner_radius,
                        )
                    }
                };
                let pixel = masked.get_pixel_mut(x, y);

                if in_content {
                    let source_x = (pixel_x - content_offset)
                        .clamp(0.0, content_pixel_size as f32 - 1.0)
                        as u32;
                    let source_y = (pixel_y - content_offset)
                        .clamp(0.0, content_pixel_size as f32 - 1.0)
                        as u32;
                    match scaled_content.get_pixel_checked(source_x, source_y) {
                        Some(source_pixel) => {
                            *pixel = *source_pixel;
                            pixel[3] = 255;
                        }
                        None => pixel[3] = 0,
                    }
                } else if in_border {
                    let projection = dx * direction.x + dy * direction.y;
                    let gradient_pos = if projection_range > 0.0 {
                        (projection - min_projection) / projection_range
                    } else {
                        0.5
                    };
                    let colour = self.border_colour_at(gradient_pos);
                    pixel.clone_from(&colour);
                } else {
                    pixel[3] = 0;
                }
            }
        }
        masked
    }

    fn border_fill_colour(&self) -> Rgba<u8> {
        match self.colours {
            BorderColours::Single(colour) => opaque_colour(colour),
            BorderColours::Gradient { first, .. } => opaque_colour(first),
            // unreachable: full rainbow borders are rendered per pixel
            BorderColours::Rainbow { .. } => rainbow_colour(0.0),
        }
    }

    fn border_colour_at(&self, gradient_pos: f32) -> Rgba<u8> {
        match self.colours {
            BorderColours::Single(colour) => opaque_colour(colour),
            BorderColours::Gradient { first, last, .. } => {
                lerp_rgba(opaque_colour(first), opaque_colour(last), gradient_pos)
            }
            BorderColours::Rainbow { .. } => rainbow_colour(gradient_pos),
        }
    }

    fn gradient_projection_bounds(shape: Shape, inner_radius: f32, direction: Vec2) -> (f32, f32) {
        // the square's corners are its farthest points along a direction
        let extent = match shape {
            Shape::Circle => inner_radius,
            Shape::Square => inner_radius * (direction.x.abs() + direction.y.abs()),
        };
        (-extent, extent)
    }

    fn generate_avatar(&self) -> Result<String, AppError> {
        let (Some(preview), Some(start)) = (&self.preview_image, self.drag_start) else {
            return Err(AppError::MissingSelection);
        };
        let radius = match self.drag_stop {
            Some(stop) => start.distance(stop),
            None => return Err(AppError::MissingSelection),
        };
        let filepath = self.filepath.as_deref().ok_or(AppError::MissingFilePath)?;
        let file_name = final_file_name(filepath);
        let saved = SavedAvatar {
            original_file_name: filepath
                .file_name()
                .map_or_else(|| filepath.to_path_buf(), PathBuf::from),
            center: Vec2::new(start.x, start.y),
            radius,
            border_percent: self.border_percent,
            colours: self.colours,
        };
        let payload = selection_metadata_payload(&saved);
        let output_path = filepath.with_file_name(&file_name);
        Self::save_avatar(&output_path, preview, &payload)?;
        info!(
            "Generation successful! File: {path}",
            path = output_path.display()
        );
        Ok(file_name)
    }

    fn save_avatar(path: &Path, image: &RgbaImage, metadata_payload: &str) -> Result<(), AppError> {
        let file = std::fs::File::create(path).map_err(|source| AppError::Io {
            path: path.to_path_buf(),
            source,
        })?;
        let mut encoder =
            png::Encoder::new(std::io::BufWriter::new(file), image.width(), image.height());
        encoder.set_color(png::ColorType::Rgba);
        encoder.set_depth(png::BitDepth::Eight);
        let mut writer = encoder
            .write_header()
            .map_err(|source| AppError::PngEncode {
                path: path.to_path_buf(),
                source,
            })?;
        let chunk = ITXtChunk::new(PNG_METADATA_KEYWORD, metadata_payload);
        writer
            .write_text_chunk(&chunk)
            .map_err(|source| AppError::PngEncode {
                path: path.to_path_buf(),
                source,
            })?;
        writer
            .write_image_data(image.as_raw())
            .map_err(|source| AppError::PngEncode {
                path: path.to_path_buf(),
                source,
            })?;
        Ok(())
    }

    fn read_selection_metadata(path: &Path) -> Option<SavedAvatar> {
        let file = std::fs::File::open(path).ok()?;
        let reader = png::Decoder::new(std::io::BufReader::new(file))
            .read_info()
            .ok()?;
        let chunk = reader
            .info()
            .utf8_text
            .iter()
            .find(|chunk| chunk.keyword == PNG_METADATA_KEYWORD)?;
        chunk.get_text().ok()?.parse().ok()
    }

    pub(crate) fn load_image_to_state(&mut self, path: &Path) -> Result<(), AppError> {
        let saved = Self::read_selection_metadata(path);
        let source_path = match &saved {
            Some(saved) => {
                // the avatar is saved next to the original image, so the
                // stored file name resolves against the avatar's directory
                let original = path.with_file_name(&saved.original_file_name);
                if original.is_file() {
                    original
                } else {
                    path.to_path_buf()
                }
            }
            _ => path.to_path_buf(),
        };
        let decoded = ImageReader::open(&source_path)
            .map_err(|source| AppError::Io {
                path: source_path.clone(),
                source,
            })?
            .decode()
            .map_err(|source| AppError::ImageRead {
                path: source_path.clone(),
                source,
            })?
            .into_rgba8();
        let dimension = Vec2::new(decoded.width() as f32, decoded.height() as f32);

        self.filepath = Some(source_path.clone());
        self.source_image = Some(decoded);
        self.original_image_dimension = Some(dimension);
        self.image = Some(Image::new(file_uri(&source_path)));
        self.drag_start = None;
        self.drag_stop = None;
        if let Some(saved) = saved {
            self.drag_start = Some(Pos2::new(saved.center.x, saved.center.y));
            self.drag_stop = Some(Pos2::new(saved.center.x + saved.radius, saved.center.y));
            self.border_percent = saved.border_percent;
            self.colours = saved.colours;
            self.gradient_change = GradientChange::First;
        }
        self.preview_image = None;
        self.preview_texture = None;
        debug!(
            "Loaded image: {path} (dimensions: {width:.0} x {height:.0})",
            path = source_path.display(),
            width = dimension.x,
            height = dimension.y
        );
        Ok(())
    }

    fn reset_image_state(&mut self) {
        self.filepath = None;
        self.source_image = None;
        self.original_image_dimension = None;
        self.image = None;
        self.drag_start = None;
        self.drag_stop = None;
        self.preview_image = None;
        self.preview_texture = None;
    }
}

/// Clamps a floating-point pixel range to `[0, max]` and returns the
/// inclusive start / exclusive end pixel indices. The result always covers at
/// least one pixel and stays inside the image, so cropping from it cannot
/// panic.
fn clamped_range(low: f32, high: f32, max: u32) -> (u32, u32) {
    let max = max.max(1) as f32;
    let low = low.clamp(0.0, max);
    let high = high.clamp(low, max);
    let start = (low as u32).min(max as u32 - 1);
    let end = ((high as u32).saturating_add(1)).clamp(start + 1, max as u32);
    (start, end)
}

/// `file://` URI accepted by egui's file loader. Windows paths must use
/// forward slashes, and a drive-letter path needs an extra `/` otherwise
/// the loader treats the drive letter as the host of a UNC network path.
fn file_uri(path: &Path) -> String {
    if cfg!(windows) {
        let path = path.to_string_lossy().replace('\\', "/");
        match path.strip_prefix("//") {
            // UNC path: the host belongs in the authority part of the URI
            Some(host_and_share) => format!("file://{host_and_share}"),
            None => format!("file:///{path}"),
        }
    } else {
        format!("file://{path}", path = path.display())
    }
}

/// File name of a generated avatar: `<stem>_avatar.png`, saved next to the
/// original image. The selection and settings live in the PNG metadata, not
/// in the name.
fn final_file_name(original: &Path) -> String {
    match original.file_stem() {
        // stem only: the full path would leak the user's directory structure
        // into the shared file name
        Some(stem) => format!("{stem}_avatar.png", stem = stem.to_string_lossy()),
        // no usable stem (only possible for paths like `/` or `..`)
        None => "avatar.png".to_owned(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn file_uri_matches_loader_expectations() {
        if cfg!(windows) {
            assert_eq!(
                file_uri(Path::new(r"C:\Users\me\a b.png")),
                "file:///C:/Users/me/a b.png"
            );
            assert_eq!(
                file_uri(Path::new(r"\\server\share\a b.png")),
                "file://server/share/a b.png"
            );
        } else {
            assert_eq!(file_uri(Path::new("/tmp/a b.png")), "file:///tmp/a b.png");
        }
    }

    #[test]
    fn gradient_range_covers_the_visible_shape() {
        let axis = Vec2::new(1.0, 0.0);
        assert_eq!(
            AvatarCreatorApp::gradient_projection_bounds(Shape::Circle, 10.0, axis),
            (-10.0, 10.0)
        );
        assert_eq!(
            AvatarCreatorApp::gradient_projection_bounds(Shape::Square, 10.0, axis),
            (-10.0, 10.0)
        );
        // along a diagonal, a circle still reaches only its radius, while a
        // square reaches its corners (extent radius * (|x| + |y|))
        let diagonal = Vec2::new(0.75, 0.75);
        assert_eq!(
            AvatarCreatorApp::gradient_projection_bounds(Shape::Circle, 10.0, diagonal),
            (-10.0, 10.0)
        );
        assert_eq!(
            AvatarCreatorApp::gradient_projection_bounds(Shape::Square, 10.0, diagonal),
            (-15.0, 15.0)
        );
    }

    #[test]
    fn preview_outdated_for_selection_without_preview() {
        let mut app = AvatarCreatorApp {
            drag_start: Some(Pos2::new(2.0, 2.0)),
            drag_stop: Some(Pos2::new(3.0, 2.0)),
            ..AvatarCreatorApp::default()
        };
        let inputs = app.preview_inputs();
        // a selection restored from metadata changes no input during the
        // load frame, so the missing preview must trigger regeneration
        assert!(app.preview_outdated(&inputs));

        // with a preview present and no input change, nothing regenerates
        app.preview_image = Some(RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255])));
        assert!(!app.preview_outdated(&inputs));
    }

    #[test]
    fn clamped_range_stays_inside_and_is_never_empty() {
        assert_eq!(clamped_range(-5.0, 5.0, 100), (0, 6));
        assert_eq!(clamped_range(50.0, 500.0, 100), (50, 100));
        // inverted or degenerate ranges still yield exactly one pixel
        assert_eq!(clamped_range(90.0, 10.0, 100), (90, 91));
        assert_eq!(clamped_range(-5.0, -1.0, 100), (0, 1));
        // a selection outside the image is pulled back inside
        assert_eq!(clamped_range(120.0, 150.0, 100), (99, 100));
    }

    #[test]
    fn final_file_name_is_simple_avatar_name() {
        assert_eq!(
            final_file_name(Path::new("/tmp/photo.png")),
            "photo_avatar.png"
        );
        // the stem keeps inner dots, the extension is replaced
        assert_eq!(
            final_file_name(Path::new("my.photo.jpg")),
            "my.photo_avatar.png"
        );
        // a path without a usable stem still yields a valid name
        assert_eq!(final_file_name(Path::new("/")), "avatar.png");
    }

    #[test]
    fn load_image_resolves_original_from_file_name_metadata() {
        let dir = std::env::temp_dir().join(format!(
            "avatar-creator-test-{pid}",
            pid = std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let original = dir.join("photo.png");
        let image = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]));
        image.save(&original).unwrap();

        let saved = SavedAvatar {
            original_file_name: PathBuf::from("photo.png"),
            center: Vec2::new(10.5, 20.25),
            radius: 55.0,
            border_percent: 12,
            colours: BorderColours::Single(Color32::from_rgb(158, 27, 50)),
        };
        let payload = selection_metadata_payload(&saved);
        // the payload must contain the file name only, never the directory
        assert_eq!(payload.lines().next(), Some("photo.png"));
        let avatar = dir.join(final_file_name(&original));
        AvatarCreatorApp::save_avatar(&avatar, &image, &payload).unwrap();

        let mut app = AvatarCreatorApp::default();
        app.load_image_to_state(&avatar).unwrap();

        assert_eq!(app.filepath.as_deref(), Some(original.as_path()));
        assert_eq!(app.drag_start, Some(Pos2::new(10.5, 20.25)));
        assert_eq!(app.drag_stop, Some(Pos2::new(65.5, 20.25)));
        assert_eq!(
            app.colours,
            BorderColours::Single(Color32::from_rgb(158, 27, 50))
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    #[test]
    fn generate_avatar_saves_simple_name_and_restores_state() {
        let dir = std::env::temp_dir().join(format!(
            "avatar-creator-test-generate-{pid}",
            pid = std::process::id()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let original = dir.join("photo.png");
        let image = RgbaImage::from_pixel(1, 1, Rgba([0, 0, 0, 255]));
        image.save(&original).unwrap();

        let app = AvatarCreatorApp {
            filepath: Some(original.clone()),
            preview_image: Some(image),
            drag_start: Some(Pos2::new(10.5, 20.25)),
            drag_stop: Some(Pos2::new(65.5, 20.25)),
            border_percent: 12,
            colours: BorderColours::Single(Color32::from_rgb(158, 27, 50)),
            resize: false,
            ..AvatarCreatorApp::default()
        };
        let file_name = app.generate_avatar().unwrap();

        // simple file name, saved next to the original image
        assert_eq!(file_name, "photo_avatar.png");
        let output = dir.join(&file_name);
        assert!(output.is_file());

        // the metadata stores the original's file name only, so loading the
        // avatar re-opens the original with the same settings
        let saved = AvatarCreatorApp::read_selection_metadata(&output).unwrap();
        assert_eq!(saved.original_file_name, PathBuf::from("photo.png"));
        let mut app = AvatarCreatorApp::default();
        app.load_image_to_state(&output).unwrap();
        assert_eq!(app.filepath.as_deref(), Some(original.as_path()));
        assert_eq!(app.drag_start, Some(Pos2::new(10.5, 20.25)));
        assert_eq!(app.drag_stop, Some(Pos2::new(65.5, 20.25)));
        assert_eq!(app.border_percent, 12);
        assert_eq!(
            app.colours,
            BorderColours::Single(Color32::from_rgb(158, 27, 50))
        );

        std::fs::remove_dir_all(&dir).unwrap();
    }

    /// Regression test for the divider snap-back: egui panels persist their
    /// size from their content, so panel content that does not fill the panel
    /// width makes the panel shrink back after the divider is dragged. This
    /// mirrors the app's layout, a settings panel whose content fills the
    /// width, and a preview panel whose content is forced to fill the width,
    /// and checks that both dividers stay where they are dragged.
    #[test]
    fn panel_dividers_stay_where_dragged() {
        let ctx = egui::Context::default();
        let screen = egui::Rect::from_min_size(egui::Pos2::ZERO, egui::Vec2::new(1280.0, 900.0));

        // the central area's boundaries reveal where the panels end
        let central = std::cell::Cell::new(egui::Rect::NOTHING);
        let run = |ctx: &egui::Context, events: Vec<egui::Event>| {
            let raw = egui::RawInput {
                screen_rect: Some(screen),
                events,
                ..Default::default()
            };
            let mut output = ctx.run_ui(raw, |ui| {
                egui::CentralPanel::default().show(ui, |ui| {
                    egui::Panel::left("utility_panel")
                        .default_size(280.0)
                        .show(ui, |ui| {
                            egui::ScrollArea::vertical()
                                .auto_shrink([false, true])
                                .show(ui, |ui| {
                                    ui.label("settings");
                                });
                        });
                    egui::Panel::right("preview_panel").show(ui, |ui| {
                        ui.set_min_width(ui.available_width());
                        ui.label("preview");
                    });
                    central.set(ui.available_rect_before_wrap());
                    ui.label("image area");
                });
            });
            // no GPU here; discard the font texture upload
            output.textures_delta.clear();
        };

        let drag = |ctx: &egui::Context, from: egui::Pos2, to: egui::Pos2| {
            let button = |pos: egui::Pos2, pressed: bool| egui::Event::PointerButton {
                pos,
                button: egui::PointerButton::Primary,
                pressed,
                modifiers: egui::Modifiers::default(),
            };
            run(ctx, vec![egui::Event::PointerMoved(from)]);
            run(ctx, vec![button(from, true)]);
            run(ctx, vec![egui::Event::PointerMoved(to)]);
            run(ctx, vec![egui::Event::PointerMoved(to)]);
            run(ctx, vec![button(to, false)]);
        };

        // let the layout settle
        run(&ctx, vec![]);
        run(&ctx, vec![]);
        let initial = central.get();

        // drag the right divider (left edge of the preview panel) to the left
        let right_divider = egui::Pos2::new(initial.right(), 450.0);
        drag(
            &ctx,
            right_divider,
            right_divider - egui::Vec2::new(150.0, 0.0),
        );
        run(&ctx, vec![]);
        run(&ctx, vec![]);
        let after_right = central.get();

        // drag the left divider (right edge of the settings panel) to the right
        let left_divider = egui::Pos2::new(initial.left(), 450.0);
        drag(
            &ctx,
            left_divider,
            left_divider + egui::Vec2::new(150.0, 0.0),
        );
        run(&ctx, vec![]);
        run(&ctx, vec![]);
        let after_left = central.get();

        assert!(
            after_right.right() < initial.right() - 100.0,
            "right divider snapped back: {initial:?} -> {after_right:?}"
        );
        assert!(
            after_left.left() > initial.left() + 100.0,
            "left divider snapped back: {initial:?} -> {after_left:?}"
        );
    }
}
