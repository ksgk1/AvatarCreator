use std::path::Path;

use app::AvatarCreatorApp;
use eframe::egui;
use tracing::{error, info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

mod app;

const APPLICATION_NAME: &str = "Avatar creator";
const VERSION: &str = env!("CARGO_PKG_VERSION");
const UI_WINDOW_WIDTH: f32 = 1280.0;
const UI_WINDOW_HEIGHT: f32 = 900.0;
const UI_WINDOW_MIN_WIDTH: f32 = 640.0;
const UI_WINDOW_MIN_HEIGHT: f32 = 480.0;
const SUPPORTED_IMAGE_FORMATS: &[&str; 5] = &["bmp", "jpeg", "jpg", "png", "webp"];

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let custom_format = fmt::format()
        .with_target(false)
        .with_ansi(true)
        .with_file(true)
        .with_level(true)
        .with_line_number(true)
        .compact();
    let fmt_layer = fmt::layer().event_format(custom_format);
    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([UI_WINDOW_WIDTH, UI_WINDOW_HEIGHT])
            .with_min_inner_size([UI_WINDOW_MIN_WIDTH, UI_WINDOW_MIN_HEIGHT])
            .with_resizable(true)
            .with_maximize_button(true),
        ..Default::default()
    };

    info!("Starting application [Version: {VERSION}]");
    let mut default_state = Box::<AvatarCreatorApp>::default();
    match args.len() {
        1 => {
            // normal operation, we select the image manually
        }
        2 => {
            // we check if the first argument is a valid path to an image, so we can load it
            // immediately
            let file = args[1].as_str();
            let path = Path::new(file);
            let extension = path
                .extension()
                .and_then(|extension| extension.to_str())
                .map(str::to_ascii_lowercase);
            match extension {
                Some(extension) if SUPPORTED_IMAGE_FORMATS.contains(&extension.as_str()) => {
                    if path.exists() {
                        if let Err(e) = default_state.load_image_to_state(path) {
                            error!("Failed to load image {file}: {e}");
                        }
                    } else {
                        warn!("Path {file} does not exist.");
                    }
                }
                _ => warn!(
                    "Unsupported file type for {file}. Supported formats: {formats}.",
                    formats = SUPPORTED_IMAGE_FORMATS.join(", ")
                ),
            }
        }
        _ => warn!(
            "Too many arguments defined. Only one argument can be provided, the file path of the file to be used."
        ),
    }

    eframe::run_native(
        APPLICATION_NAME,
        options,
        Box::new(|cc| {
            egui_extras::install_image_loaders(&cc.egui_ctx);
            Ok(default_state)
        }),
    )
}
