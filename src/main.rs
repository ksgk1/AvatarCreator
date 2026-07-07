use std::path::Path;

use app::AvatarCreatorApp;
use eframe::egui::{self};
use tracing::{info, warn};
use tracing_subscriber::layer::SubscriberExt;
use tracing_subscriber::util::SubscriberInitExt;
use tracing_subscriber::{EnvFilter, fmt};

mod app;

const APPLICATION_NAME: &str = "Avatar creator";
const VERSION: &str = env!("CARGO_PKG_VERSION");
/// Default window width
const UI_WINDOW_WIDTH: f32 = 1280.0;
/// Default window height
const UI_WINDOW_HEIGHT: f32 = 900.0;
/// All supported image extensions
const SUPPORTED_IMAGE_FORMATS: &[&str; 4] = &["png", "jpg", "jpeg", "bmp"];

fn main() -> eframe::Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let env_filter = EnvFilter::try_from_default_env().unwrap_or_else(|_| EnvFilter::new("info"));
    let custom_format = fmt::format().with_target(false).with_ansi(true).with_file(true).with_level(true).with_line_number(true).compact();
    let fmt_layer = fmt::layer().event_format(custom_format);
    tracing_subscriber::registry().with(env_filter).with(fmt_layer).init();

    let options =
        eframe::NativeOptions { viewport: egui::ViewportBuilder::default().with_inner_size([UI_WINDOW_WIDTH, UI_WINDOW_HEIGHT]).with_resizable(true).with_maximize_button(true), ..Default::default() };

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
            for ext in SUPPORTED_IMAGE_FORMATS {
                if file.to_ascii_lowercase().ends_with(ext) {
                    default_state.load_image_to_state(Path::new(file));
                    break;
                }
            }
        }
        _ => warn!("Too many arguments defined. Only one argument can be provided, the file path of the file to be used."),
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
