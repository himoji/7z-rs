mod app;
mod models;
mod ui;
mod utils;
mod parallel;

use app::ArchiveManager;
use std::panic;
use tracing::{error, info};

fn cleanup_on_panic() {
    error!("Application panicked - performing emergency cleanup");
}

fn main() -> Result<(), eframe::Error> {
    // Initialize logging with reasonable defaults
    tracing_subscriber::fmt::init();

    // Set up custom panic hook
    let original_hook = panic::take_hook();
    panic::set_hook(Box::new(move |panic_info| {
        // Call the original panic hook
        original_hook(panic_info);
        // Perform cleanup
        cleanup_on_panic();
    }));

    info!("Starting Archive Manager");

    let options = eframe::NativeOptions {
        vsync: true,
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([800.0, 600.0])
            .with_drag_and_drop(true),
        ..Default::default()
    };

    // Run the application with additional error handling
    match eframe::run_native(
        "Archive Manager",
        options,
        Box::new(|_| Ok(Box::<ArchiveManager>::default())),
    ) {
        Ok(_) => {
            info!("Application terminated normally");
            Ok(())
        }
        Err(e) => {
            error!("Application error: {:?}", e);
            cleanup_on_panic();
            Err(e)
        }
    }
}