use crate::models::{ArchiveFile, ArchiveZone, Password};
use crate::parallel::compress_files_parallel;
use crate::utils::{get_formatted_size, get_temp_dir, open_system_file};
use eframe::epaint::Color32;
use egui::{Label, RichText, Sense};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use zip::result::ZipError;
use zip::write::FileOptions;
use zip::ZipArchive;

#[derive(Clone, Default)]
pub struct ProgressState {
    pub compression_progress: Option<(f32, CompressionStats)>,
    pub extraction_progress: Option<f32>,
}

pub struct ArchiveManager {
    pub selected_files: Vec<PathBuf>,
    pub files_to_remove: Vec<usize>,
    pub password: Password,
    pub dark_mode: bool,
    pub status_message: String,
    pub show_settings: bool,
    pub current_archive: Option<(PathBuf, Vec<ArchiveFile>)>,
    pub compress_zone: ArchiveZone,
    pub extract_zone: ArchiveZone,
    pub last_used_password: Option<String>,
    progress_state: Arc<Mutex<ProgressState>>,
    pub compression_sender: Option<Sender<()>>,
    pub hover_file: Option<String>,
}

#[derive(Clone)]
pub struct CompressionStats {
    pub original_size: u64,
    pub compressed_size: u64,
    pub start_time: Instant,
    pub estimated_time: Duration,
    pub output_path: PathBuf,
}

impl Default for ArchiveManager {
    fn default() -> Self {
        Self {
            selected_files: Vec::new(),
            files_to_remove: Vec::new(),
            password: Password::default(),
            dark_mode: true,
            status_message: String::new(),
            show_settings: false,
            current_archive: None,
            compress_zone: ArchiveZone {
                is_compress_zone: true,
                ..Default::default()
            },
            extract_zone: ArchiveZone {
                is_compress_zone: false,
                ..Default::default()
            },
            last_used_password: None,
            compression_sender: None,
            hover_file: None,
            progress_state: Arc::new(Mutex::new(ProgressState::default())),
        }
    }
}

impl ArchiveManager {
    pub fn compress_files(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        if self.selected_files.is_empty() {
            self.status_message = "No files selected".to_string();
            return Ok(());
        }

        if let Some(output_path) = rfd::FileDialog::new()
            .add_filter("ZIP", &["zip"])
            .set_file_name("archive.zip")
            .save_file()
        {
            let files = self.selected_files.clone();
            let (progress_tx, progress_rx) = channel();
            let (cancel_tx, _cancel_rx) = channel();
            let cancel_tx = Arc::new(Mutex::new(cancel_tx));
            let progress_state = Arc::clone(&self.progress_state);
            let total_size: u64 = files
                .iter()
                .filter_map(|path| std::fs::metadata(path).ok())
                .map(|meta| meta.len())
                .sum();

            let stats = CompressionStats {
                original_size: total_size,
                compressed_size: 0,
                start_time: Instant::now(),
                estimated_time: Duration::from_secs(0),
                output_path: output_path.clone(),
            };
            let stats = Arc::new(Mutex::new(stats));

            let password = self.password.0.clone().unwrap();

            // Store sender for cancellation
            self.compression_sender = Some(cancel_tx.lock().unwrap().clone());
            // Spawn compression thread
            thread::spawn(move || {
                if let Err(e) = compress_files_parallel(
                    files,
                    output_path,
                    progress_tx,
                    cancel_tx,
                    stats,
                    password,
                ) {
                    error!("Compression error: {}", e);
                }
            });

            // Progress update thread
            thread::spawn(move || {
                while let Ok((progress, stats)) = progress_rx.recv() {
                    if let Ok(mut state) = progress_state.lock() {
                        state.compression_progress = Some((progress, stats));
                    }
                }
            });
        }

        Ok(())
    }
    pub fn open_file(&self, file_name: String) -> Result<(), Box<dyn std::error::Error>> {
        if let Some((archive_path, _)) = &self.current_archive {
            let temp_dir = get_temp_dir();
            std::fs::create_dir_all(&temp_dir)?;

            let archive_path = archive_path.clone();
            let progress_state = Arc::clone(&self.progress_state);

            // Spawn extraction thread
            thread::spawn(move || {
                let file = File::open(&archive_path).unwrap();
                let mut archive = ZipArchive::new(file).unwrap();
                let total_size = archive.by_name(&file_name).unwrap().size();

                let mut zip_file = archive.by_name(&file_name).unwrap();
                let temp_path = temp_dir.join(&file_name);

                if let Some(parent) = temp_path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }

                let mut temp_file = File::create(&temp_path).unwrap();
                let mut buffer = [0; 8192];
                let mut processed_size = 0;

                while let Ok(n) = zip_file.read(&mut buffer) {
                    if n == 0 {
                        break;
                    }
                    temp_file.write_all(&buffer[..n]).unwrap();
                    processed_size += n as u64;

                    if let Ok(mut state) = progress_state.lock() {
                        state.extraction_progress = Some(processed_size as f32 / total_size as f32);
                    }
                }

                open_system_file(&temp_path).unwrap();

                if let Ok(mut state) = progress_state.lock() {
                    state.extraction_progress = None;
                }
            });
        }
        Ok(())
    }

    pub fn open_archive(&mut self, path: &Path) -> Result<(), Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let mut archive = ZipArchive::new(file)?;

        let needs_password = archive
            .get_aes_verification_key_and_salt(0)
            .unwrap()
            .is_none()
            == false;

        if needs_password && self.password.0.is_none() {
                self.status_message =
                    "Archive is encrypted. Please enter password in settings.".to_string();
                self.show_settings = true;
                return Ok(());

        }

        if self.password.0.is_some() {
            let mut files = Vec::new();
            for i in 0..archive.len() {
                let file = archive.by_index_decrypt(i, self.password.0.clone().unwrap().as_bytes())?;
                files.push(ArchiveFile {
                    name: file.name().to_string(),
                    is_directory: file.is_dir(),
                    size: file.size(),
                });
            }
            self.current_archive = Some((path.to_path_buf(), files));
            self.status_message = "Archive opened successfully".to_string();
            return Ok(())
        }

        let mut files = Vec::new();
        for i in 0..archive.len() {
            let file = archive.by_index(i)?;
            files.push(ArchiveFile {
                name: file.name().to_string(),
                is_directory: file.is_dir(),
                size: file.size(),
            });
        }
        self.current_archive = Some((path.to_path_buf(), files));
        self.status_message = "Archive opened successfully".to_string();
        Ok(())

    }

    pub fn cleanup_removed_files(&mut self) {
        self.files_to_remove.sort_unstable_by(|a, b| b.cmp(a));
        for &index in &self.files_to_remove {
            if index < self.selected_files.len() {
                self.selected_files.remove(index);
            }
        }
        self.files_to_remove.clear();
    }

    fn handle_drops(&mut self, ctx: &egui::Context) {
        // Store the zones at the start of the frame
        let compress_zone = self.compress_zone.rect;
        // let extract_zone = self.extract_zone.rect;

        ctx.input(|i| {
            // Get the pointer position from the input state
            let pointer_pos = i.pointer.hover_pos();

            if !i.raw.dropped_files.is_empty() {
                info!("Files dropped: {} files", i.raw.dropped_files.len());

                // Determine which zone we're in based on pointer position
                let dropped_in_compress = match (pointer_pos, compress_zone) {
                    (Some(pos), Some(rect)) => {
                        info!("Checking drop zone - pointer: {:?}, zone: {:?}", pos, rect);
                        rect.contains(pos)
                    }
                    _ => {
                        warn!("Could not determine drop zone - using default (compress zone)");
                        true
                    }
                };

                info!("Dropped in compress zone: {}", dropped_in_compress);

                for dropped_file in &i.raw.dropped_files {
                    if let Some(path) = &dropped_file.path {
                        info!("Processing dropped file: {:?}", path);

                        match self.handle_file_drop(path, dropped_in_compress) {
                            Ok(_) => info!("File drop handled successfully"),
                            Err(e) => {
                                error!("Error handling file drop: {}", e);
                                self.status_message = format!("Error handling dropped file: {}", e);
                            }
                        }
                    } else {
                        warn!("Dropped file has no path");
                        self.status_message = "Error: Dropped file has no path".to_string();
                    }
                }
            }
        });
    }
    pub fn handle_file_drop(
        &mut self,
        path: &Path,
        dropped_in_compress_zone: bool,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("Handling file drop: {:?}", path);

        let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");

        info!("File extension: {}", extension);

        match (extension, dropped_in_compress_zone) {
            ("zip" | "7z" | "rar", true) => {
                info!("Adding archive to compression list");
                self.selected_files.push(path.to_path_buf());
                self.status_message = "Archive added to compression list".to_string();
            }
            ("zip" | "7z" | "rar", false) => {
                info!("Opening archive for viewing");

                self.open_archive(path)?;
            }
            _ if dropped_in_compress_zone => {
                info!("Adding regular file to compression list");
                self.selected_files.push(path.to_path_buf());
                self.status_message = "File added to compression list".to_string();
            }
            _ => {
                warn!("Unsupported file type for viewing");
                self.status_message = "Unsupported file type for viewing".to_string();
            }
        }
        Ok(())
    }
}

impl eframe::App for ArchiveManager {
    fn update(&mut self, ctx: &egui::Context, frame: &mut eframe::Frame) {
        if ctx.input(|i| i.pointer.is_moving()) {
            ctx.request_repaint_after(std::time::Duration::from_millis(1000 / 60));
        }

        if self.dark_mode {
            ctx.set_visuals(egui::Visuals::dark());
        } else {
            ctx.set_visuals(egui::Visuals::light());
        }

        egui::TopBottomPanel::top("top_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                if ui.button("Add Files").clicked() {
                    if let Some(files) = rfd::FileDialog::new().pick_files() {
                        self.selected_files.extend(files);
                    }
                }

                if ui.button("Compress").clicked() {
                    if let Err(e) = self.compress_files() {
                        self.status_message = format!("Error: {}", e);
                    }
                }

                if ui.button("Settings").clicked() {
                    self.show_settings = !self.show_settings;
                }
            });
        });

        egui::CentralPanel::default().show(ctx, |ui| {
            if self.show_settings {
                ui.group(|ui| {
                    let mut password_str = self.password.0.clone().unwrap_or_default();
                    crate::ui::draw_settings(
                        ui,
                        &mut self.dark_mode,
                        &mut password_str,
                        false,
                        &mut Default::default(),
                        (&mut |new_password| self.password.set(new_password)),
                    );
                });
            } else {
                // Compression zone at the top
                ui.group(|ui| {

                    ui.heading("Files to Compress");
                    let compress_response = ui.allocate_ui_with_layout(
                        egui::Vec2::new(200.0, 100.0), // Width and height of the area
                        egui::Layout::centered_and_justified(egui::Direction::TopDown), // Center text
                        |ui| {
                            ui.add(egui::Label::new("Drop files here to add to compression list")
                                .sense(egui::Sense::hover()));
                        },
                    );

                    self.compress_zone.rect = Some(compress_response.response.rect);

                    // Scrollable area for file list
                    egui::ScrollArea::vertical()
                        .id_salt("filestocompress")
                        .max_height(200.0) // Fixed height for scroll area
                        .show(ui, |ui| {
                            crate::ui::draw_file_list(
                                ui,
                                &self.selected_files,
                                &mut self.files_to_remove,
                            );
                        });

                    // Show compression progress in the compression zone
                    if let Ok(state) = self.progress_state.lock() {
                        if let Some((progress, stats)) = &state.compression_progress {
                            ui.add_space(10.0);
                            let progress_bar = egui::ProgressBar::new(*progress)
                                .text(format!("Compressing... {:.1}%", progress * 100.0))
                                .animate(true);
                            ui.add(progress_bar);

                            ui.label(format!(
                                "Original size: {}\nCompressed size: {}\nTime remaining: {:.1}s",
                                get_formatted_size(stats.original_size),
                                get_formatted_size(stats.compressed_size),
                                stats.estimated_time.as_secs_f32()
                            ));
                        }
                    }
                });

                ui.add_space(10.0);
                ui.separator();
                ui.add_space(10.0);

                // Archive viewing zone
                ui.group(|ui| {
                    ui.heading("Archive Contents");

                    let extract_response = ui.allocate_ui_with_layout(
                        egui::Vec2::new(200.0, 100.0), // Width and height of the area
                        egui::Layout::centered_and_justified(egui::Direction::TopDown), // Center text
                        |ui| {
                            ui.add(egui::Label::new("Drop archive here to view contents")
                                .sense(egui::Sense::hover()));
                        },
                    );

                    self.extract_zone.rect = Some(extract_response.response.rect);

                    // Scrollable area for archive contents
                    egui::ScrollArea::vertical()
                        .id_salt("ArchiveContents")
                        .max_height(200.0) // Fixed height for scroll area
                        .show(ui, |ui| {
                            // Display archive contents if available
                            if let Some((_, files)) = &self.current_archive {
                                ui.add_space(5.0);
                                let hover_file = self.hover_file.clone();
                                let mut new_hover = None;

                                // Handle archive contents with separate closure for opening files
                                let open_result = {
                                    let mut open_error = None;
                                    for file in files {
                                        let text = if file.is_directory {
                                            format!("📁 {}", file.name)
                                        } else {
                                            format!("📄 {} ({} bytes)", file.name, file.size)
                                        };

                                        if !file.is_directory {
                                            let response = ui.add(
                                                Label::new(RichText::new(&text).color(
                                                    if Some(file.name.clone()) == hover_file {
                                                        Color32::YELLOW
                                                    } else {
                                                        ui.style().visuals.text_color()
                                                    },
                                                ))
                                                .sense(Sense::click()),
                                            );

                                            if response.hovered() {
                                                new_hover = Some(file.name.clone());
                                                ui.output_mut(|o| {
                                                    o.cursor_icon = egui::CursorIcon::PointingHand
                                                });
                                            }

                                            if response.double_clicked() {
                                                if let Err(e) = self.open_file(file.name.clone()) {
                                                    open_error = Some(e);
                                                }
                                            }

                                            response.on_hover_text("Double-click to open");
                                        } else {
                                            ui.label(text);
                                        }
                                    }
                                    open_error
                                };

                                // Update hover state
                                self.hover_file = new_hover;

                                // Handle any errors from opening files
                                if let Some(e) = open_result {
                                    ui.label(
                                        RichText::new(format!("Error: {}", e)).color(Color32::RED),
                                    );
                                }
                            }
                        });

                    // Show extraction progress outside scroll area
                    if let Ok(state) = self.progress_state.lock() {
                        if let Some(progress) = state.extraction_progress {
                            ui.add_space(10.0);
                            let progress_bar = egui::ProgressBar::new(progress)
                                .text(format!("Extracting... {:.1}%", progress * 100.0))
                                .animate(true);
                            ui.add(progress_bar);
                        }
                    }
                });
            }

            ui.add_space(10.0);
            ui.separator();
            ui.add_space(5.0);

            // Status message area at the bottom
            if !self.status_message.is_empty() {
                ui.group(|ui| {
                    ui.label(egui::RichText::new(&self.status_message).color(
                        if self.status_message.starts_with("Error") {
                            egui::Color32::RED
                        } else {
                            ui.style().visuals.text_color()
                        },
                    ));
                });
            }
        });

        // Handle drag and drop
        if !ctx.input(|i| i.pointer.is_moving()) {
            self.handle_drops(ctx);
            self.cleanup_removed_files();
        }
    }
}
