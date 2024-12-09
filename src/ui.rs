use egui::Slider;
use egui::{Color32, Label, RichText, Sense};
use crate::models::ArchiveFile;
use std::thread;

pub fn draw_archive_contents(
    ui: &mut egui::Ui,
    files: &[ArchiveFile],
    open_file_callback: &mut dyn FnMut(&str) -> Result<(), Box<dyn std::error::Error>>,
    hover_file: &mut Option<String>
) {
    for file in files {
        let text = if file.is_directory {
            format!("📁 {}", file.name)
        } else {
            format!("📄 {} ({} bytes)", file.name, file.size)
        };

        if !file.is_directory {
            let response = ui.add(
                Label::new(
                    RichText::new(&text)
                        .color(if Some(file.name.clone()) == *hover_file {
                            Color32::YELLOW
                        } else {
                            ui.style().visuals.text_color()
                        })
                )
                    .sense(Sense::click())
            );

            if response.hovered() {
                *hover_file = Some(file.name.clone());
                ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
            } else if Some(file.name.clone()) == *hover_file {
                *hover_file = None;
            }

            if response.double_clicked() {
                if let Err(e) = open_file_callback(&file.name) {
                    ui.label(RichText::new(format!("Error: {}", e)).color(Color32::RED));
                }
            }

            response.on_hover_text("Double-click to open");
        } else {
            ui.label(text);
        }
    }
}

#[derive(Clone)]
pub struct CompressionSettings {
    pub compression_level: i32,
    pub thread_count: usize,
}

impl Default for CompressionSettings {
    fn default() -> Self {
        Self {
            compression_level: 5,
            thread_count: thread::available_parallelism().map(|p| p.get()).unwrap_or(1),
        }
    }
}

pub fn draw_settings(
    ui: &mut egui::Ui,
    dark_mode: &mut bool,
    password: &mut String,
    wrong_password: bool,
    compression_settings: &mut CompressionSettings,
    on_password_change: &mut dyn FnMut(String)
) {
    ui.heading("Settings");

    ui.group(|ui| {
        ui.heading("Appearance");
        ui.checkbox(dark_mode, "Dark Mode");
    });

    ui.group(|ui| {
        ui.heading("Security");
        ui.horizontal(|ui| {
            ui.label("Password:");
            if ui.text_edit_singleline(password).changed() {
                on_password_change(password.clone());
            }
        });

        if wrong_password {
            ui.colored_label(Color32::RED, "⚠️ Wrong password or corrupted file");
        }
    });

    ui.group(|ui| {
        ui.heading("Compression");

        // Compression level slider
        ui.horizontal(|ui| {
            ui.label("Compression Level:");
            ui.add(
                Slider::new(&mut compression_settings.compression_level, 0..=9)
                    .text("Level")
                    .clamp_to_range(true)
            );
        });

        let level_description = match compression_settings.compression_level {
            0 => "No compression (fastest)",
            1..=3 => "Fast compression",
            4..=6 => "Balanced compression",
            7..=8 => "High compression",
            9 => "Maximum compression (slowest)",
            _ => "Invalid level",
        };
        ui.label(RichText::new(level_description).italics());

        // Thread count slider
        let max_threads = thread::available_parallelism().map(|p| p.get()).unwrap_or(1);
        ui.horizontal(|ui| {
            ui.label("Thread Count:");
            ui.add(
                Slider::new(&mut compression_settings.thread_count, 1..=max_threads)
                    .text("Threads")
                    .clamp_to_range(true)
            );
        });
        ui.label(
            RichText::new(format!("Using {} out of {} available cores",
                                  compression_settings.thread_count, max_threads))
                .italics()
        );
    });
}

pub fn draw_file_list(ui: &mut egui::Ui, files: &[std::path::PathBuf], files_to_remove: &mut Vec<usize>) {
    for (idx, path) in files.iter().enumerate() {
        ui.horizontal(|ui| {
            if ui.button("❌").clicked() {
                files_to_remove.push(idx);
            }
            ui.label(path.file_name().unwrap_or_default().to_string_lossy());
        });
    }
}