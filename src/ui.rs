use crate::utils::get_formatted_size;
use egui::{Color32, Frame, Label, RichText, Sense, Window};
use std::time::Duration;
use log::info;
use crate::app::ArchiveManager;

pub fn draw_file_list(ui: &mut egui::Ui, files: &[std::path::PathBuf], files_to_remove: &mut Vec<usize>) {
    ui.horizontal(|ui| {
        if !files.is_empty() {
            if ui.button("Clear All").clicked() {
                // Add all indices to files_to_remove
                files_to_remove.extend(0..files.len());
            }
        }
    });

    for (idx, path) in files.iter().enumerate() {
        ui.horizontal(|ui| {
            if ui.button("❌").clicked() {
                files_to_remove.push(idx);
            }
            ui.label(path.file_name().unwrap_or_default().to_string_lossy());
        });
    }
}


pub fn format_duration(duration: Duration) -> String {
    let total_secs = duration.as_secs();
    let hours = total_secs / 3600;
    let minutes = (total_secs % 3600) / 60;
    let seconds = total_secs % 60;

    if hours > 0 {
        format!("{}h {}m {}s", hours, minutes, seconds)
    } else if minutes > 0 {
        format!("{}m {}s", minutes, seconds)
    } else {
        format!("{}s", seconds)
    }
}

pub fn draw_unified_drop_zone(ui: &mut egui::Ui) -> egui::Response {
    Frame::none()
        .fill(ui.style().visuals.extreme_bg_color)
        .stroke(ui.style().visuals.widgets.noninteractive.bg_stroke)
        .inner_margin(20.0)
        .rounding(8.0)
        .show(ui, |ui| {
            ui.vertical_centered(|ui| {
                ui.heading("Drop Files Here");
                ui.add_space(10.0);
                ui.label("Drag and drop files or archives here");
                ui.add_space(5.0);
                ui.small("Supports ZIP files and any other files for compression");
            });
        })
        .response
}

pub fn draw_action_dialog(
    ctx: &egui::Context,
    show: &mut bool,
    remember_choice: &mut bool,
    callback: &mut dyn FnMut(bool)
) {
    Window::new("Choose Action")
        .collapsible(false)
        .resizable(false)
        .show(ctx, |ui| {
            ui.label("This is an archive file. What would you like to do?");
            ui.add_space(10.0);

            ui.horizontal(|ui| {
                if ui.button("Decompress").clicked() {
                    callback(false);
                    *show = false;
                }
                if ui.button("Add to compression").clicked() {
                    callback(true);
                    *show = false;
                }
                if ui.button("Cancel").clicked() {
                    *show = false;
                }
            });

            ui.add_space(5.0);
            ui.checkbox(remember_choice, "Remember my choice for other archives");
        });
}

impl eframe::App for ArchiveManager {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        // Set theme
        if self.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        // Draw password dialog if needed
        if self.show_password_dialog {
            self.draw_password_dialog(ctx);
        }

        // Top panel with buttons
        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Add Files").clicked() {
                    if let Some(files) = rfd::FileDialog::new().pick_files() {
                        self.selected_files.extend(files);
                    }
                }

                if ui.button("Settings").clicked() {
                    self.show_settings = !self.show_settings;
                }
            });
        });

        // Main central panel
        egui::CentralPanel::default().show(ctx, |ui| {
            if self.show_settings {
                // Settings panel
                ui.group(|ui| {
                    ui.heading("Settings");
                    ui.checkbox(&mut self.dark_mode, "Dark Mode");
                });
            } else {
                // Drop zone
                let drop_response = draw_unified_drop_zone(ui);
                self.compress_zone.rect = Some(drop_response.rect);

                // Main content group
                ui.group(|ui| {
                    // Handle archive contents or file list
                    let mut current_archive_files = None;
                    if let Some((_, files)) = &self.current_archive {
                        current_archive_files = Some(files.clone());
                    }

                    if let Some(files) = current_archive_files {
                        // Show archive contents
                        ui.heading("Archive Contents");
                        egui::ScrollArea::vertical()
                            .max_height(200.0)
                            .show(ui, |ui| {
                                for file in &files {
                                    let text = if file.is_directory {
                                        format!("📁 {}", file.name)
                                    } else {
                                        format!("📄 {} ({} bytes)", file.name, file.size)
                                    };

                                    if !file.is_directory {
                                        let is_hovered = Some(file.name.clone()) == self.hover_file;
                                        let response = ui.add(
                                            Label::new(
                                                RichText::new(&text)
                                                    .color(if is_hovered {
                                                        Color32::YELLOW
                                                    } else {
                                                        ui.style().visuals.text_color()
                                                    })
                                            )
                                                .sense(Sense::click())
                                        );

                                        if response.hovered() {
                                            self.hover_file = Some(file.name.clone());
                                            ui.output_mut(|o| o.cursor_icon = egui::CursorIcon::PointingHand);
                                        } else if Some(file.name.clone()) == self.hover_file && !is_hovered {
                                            self.hover_file = None;
                                        }

                                        if response.double_clicked() {
                                            let _ = self.open_file(file.name.clone());
                                        }

                                        response.on_hover_text("Double-click to open");
                                    } else {
                                        ui.label(text);
                                    }
                                }
                            });
                    } else if !self.selected_files.is_empty() {
                        // Show files to compress
                        ui.heading("Files to Compress");
                        egui::ScrollArea::vertical()
                            .max_height(200.0)
                            .show(ui, |ui| {
                                draw_file_list(ui, &self.selected_files, &mut self.files_to_remove);
                            });

                        if ui.button("Compress Files").clicked() {
                            let _ = self.compress_files();
                        }
                    }

                    // Handle progress states
                    let show_action_dialog = if let Ok(state) = self.progress_state.lock() {
                        // Show compression progress if any
                        if let Some((progress, stats)) = &state.compression_progress {
                            ui.add_space(10.0);
                            ui.add(
                                egui::ProgressBar::new(*progress)
                                    .text(format!("Compressing... {:.1}%", progress * 100.0))
                                    .animate(true)
                            );

                            ui.label(format!(
                                "Original size: {}\nCompressed size: {}\nCompression ratio: {:.1}%\nTime elapsed: {}\nTime remaining: {}\nFiles processed: {}/{}",
                                get_formatted_size(stats.original_size),
                                get_formatted_size(stats.compressed_size),
                                if stats.original_size > 0 {
                                    (stats.compressed_size as f64 / stats.original_size as f64) * 100.0
                                } else {
                                    0.0
                                },
                                format_duration(stats.start_time.elapsed()),
                                format_duration(stats.estimated_time),
                                stats.files_processed,
                                stats.total_files
                            ));
                        }

                        // Show extraction progress if any
                        if let Some((progress, stats)) = &state.extraction_progress {
                            ui.add_space(10.0);
                            ui.add(
                                egui::ProgressBar::new(*progress)
                                    .text(format!("Extracting {}... {:.1}%", stats.current_file, progress * 100.0))
                                    .animate(true)
                            );

                            ui.label(format!(
                                "File size: {}\nExtracted: {}\nTime elapsed: {}\nTime remaining: {}",
                                get_formatted_size(stats.original_size),
                                get_formatted_size(stats.extracted_size),
                                format_duration(stats.start_time.elapsed()),
                                format_duration(stats.estimated_time)
                            ));
                        }
                        self.show_action_dialog
                    } else {
                        false
                    };

                    // Handle action dialog outside of progress state lock
                    if show_action_dialog {
                        if let Some(path) = &self.pending_archive_path {
                            let path_clone = path.clone();
                            let mut dialog_result = None;

                            draw_action_dialog(
                                ctx,
                                &mut self.show_action_dialog,
                                &mut self.remember_archive_choice,
                                &mut |compress| {
                                    dialog_result = Some(compress);
                                }
                            );

                            if let Some(compress) = dialog_result {
                                // Store the choice for future use
                                self.last_archive_choice = Some(compress);

                                if compress {
                                    info!("Adding archive to compression list");
                                    self.selected_files.push(path_clone.clone());
                                    self.status_message = "Archive added to compression list".to_string();
                                } else {
                                    info!("Opening archive for viewing");
                                    if let Err(e) = self.open_archive(&path_clone) {
                                        self.status_message = format!("Error opening archive: {}", e);
                                    }
                                }
                                self.pending_archive_path = None;
                            }
                        }
                    }
                });
            }

            // Status message
            if !self.status_message.is_empty() {
                ui.add_space(10.0);
                ui.separator();
                ui.add_space(5.0);
                ui.label(
                    RichText::new(&self.status_message)
                        .color(if self.status_message.starts_with("Error") {
                            Color32::RED
                        } else {
                            ui.style().visuals.text_color()
                        })
                );
            }
        });

        // Handle drag and drop
        self.handle_drops(ctx);
        self.cleanup_removed_files();
    }
}