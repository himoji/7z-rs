mod app;
mod models;
mod ui;
mod utils;
mod parallel;

use app::ArchiveManager;

use tracing::{info};

fn main() -> Result<(), eframe::Error> {
    // Initialize logging with reasonable defaults
    tracing_subscriber::fmt::init();

    info!("Starting Archive Manager");

    let options = eframe::NativeOptions {
        vsync: true,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    eframe::run_native(
        "Archive Manager",
        options,
        Box::new(|_| Ok(Box::<ArchiveManager>::default())),
    )
}