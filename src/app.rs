use crate::models::{ArchiveFile, ArchiveZone};
use crate::parallel::compress_files_parallel;
use crate::utils::{get_temp_dir, open_system_file};
use egui::{Window};
use std::fs::File;
use std::io::{Read, Write};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{channel, Sender};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};
use tracing::{error, info, warn};
use zip::ZipArchive;

#[derive(Clone, Default)]
pub struct ProgressState {
    pub compression_progress: Option<(f32, CompressionStats)>,
    pub extraction_progress: Option<(f32, ExtractionStats)>,
}

#[derive(Clone)]
pub struct CompressionStats {
    pub original_size: u64,
    pub compressed_size: u64,
    pub start_time: Instant,
    pub estimated_time: Duration,
    pub output_path: PathBuf,
    pub files_processed: usize,
    pub total_files: usize,
}

#[derive(Clone)]
pub struct ExtractionStats {
    pub original_size: u64,
    pub extracted_size: u64,
    pub start_time: Instant,
    pub estimated_time: Duration,
    pub current_file: String,
}

pub struct ArchiveManager {
    pub selected_files: Vec<PathBuf>,
    pub files_to_remove: Vec<usize>,
    pub dark_mode: bool,
    pub status_message: String,
    pub show_settings: bool,
    pub current_archive: Option<(PathBuf, Vec<ArchiveFile>)>,
    pub compress_zone: ArchiveZone,
    pub extract_zone: ArchiveZone,
    pub progress_state: Arc<Mutex<ProgressState>>,
    pub compression_sender: Option<Sender<()>>,
    pub hover_file: Option<String>,
    pub show_password_dialog: bool,
    pub temp_password: String,
    pub current_operation: Option<PasswordOperation>,
    pub show_action_dialog: bool,
    pub pending_archive_path: Option<PathBuf>,
    pub remember_archive_choice: bool,pub last_archive_choice: Option<bool>,
}

#[derive(Clone)]
pub enum PasswordOperation {
    Compress,
    OpenArchive(PathBuf),
    ExtractFile(String),
}

impl Default for ArchiveManager {
    fn default() -> Self {
        Self {
            selected_files: Vec::new(),
            files_to_remove: Vec::new(),
            dark_mode: true,
            status_message: String::new(),
            show_settings: false,
            remember_archive_choice: false,
            current_archive: None,
            compress_zone: ArchiveZone::default(),
            extract_zone: ArchiveZone::default(),
            compression_sender: None,
            hover_file: None,
            progress_state: Arc::new(Mutex::new(ProgressState::default())),
            show_password_dialog: false,
            temp_password: String::new(),
            current_operation: None,
            show_action_dialog: false,
            pending_archive_path: None,
            last_archive_choice: None,
        }
    }
}

impl ArchiveManager {
    pub fn compress_files_with_password(&mut self, password: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
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
                files_processed: 0,
                total_files: files.len(),
            };
            let stats = Arc::new(Mutex::new(stats));

            self.compression_sender = Some(cancel_tx.lock().unwrap().clone());

            let password = password.unwrap_or_default();

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

    pub fn open_archive_with_password(&mut self, path: &Path, password: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
        let file = File::open(path)?;
        let mut archive = ZipArchive::new(file)?;

        let needs_password = archive
            .get_aes_verification_key_and_salt(0)
            .unwrap()
            .is_none()
            == false;

        if needs_password && password.is_none() {
            self.show_password_dialog = true;
            self.current_operation = Some(PasswordOperation::OpenArchive(path.to_path_buf()));
            self.status_message = "Archive is encrypted. Please enter password.".to_string();
            return Ok(());
        }

        let mut files = Vec::new();
        if needs_password {
            let password = password.as_ref().unwrap().clone();
            for i in 0..archive.len() {
                let file = archive.by_index_decrypt(i, password.as_bytes())?;
                files.push(ArchiveFile {
                    name: file.name().to_string(),
                    is_directory: file.is_dir(),
                    size: file.size(),
                });
            }
        } else {
            for i in 0..archive.len() {
                let file = archive.by_index(i)?;
                files.push(ArchiveFile {
                    name: file.name().to_string(),
                    is_directory: file.is_dir(),
                    size: file.size(),
                });
            }
        }

        self.current_archive = Some((path.to_path_buf(), files));
        self.status_message = "Archive opened successfully".to_string();
        Ok(())
    }

    pub fn draw_password_dialog(&mut self, ctx: &egui::Context) {
        Window::new("Enter Password")
            .collapsible(false)
            .resizable(false)
            .show(ctx, |ui| {
                ui.horizontal(|ui| {
                    ui.label("Password:");
                    ui.text_edit_singleline(&mut self.temp_password);
                });

                ui.horizontal(|ui| {
                    if ui.button("Cancel").clicked() {
                        self.show_password_dialog = false;
                        self.temp_password.clear();
                        self.current_operation = None;
                    }

                    if ui.button("OK").clicked() {
                        let password = std::mem::take(&mut self.temp_password);
                        let operation = self.current_operation.clone();
                        self.show_password_dialog = false;
                        self.current_operation = None;

                        match operation {
                            Some(PasswordOperation::Compress) => {
                                let _ = self.compress_files_with_password(Some(password));
                            }
                            Some(PasswordOperation::OpenArchive(path)) => {
                                let _ = self.open_archive_with_password(&path, Some(password));
                            }
                            Some(PasswordOperation::ExtractFile(file_name)) => {
                                let _ = self.open_file_with_password(file_name, Some(password));
                            }
                            None => {}
                        }
                    }
                });
            });
    }
    pub fn open_file_with_password(&mut self, file_name: String, password: Option<String>) -> Result<(), Box<dyn std::error::Error>> {
        if let Some((archive_path, _)) = &self.current_archive {
            let file = File::open(archive_path)?;
            let mut archive = ZipArchive::new(file)?;

            let needs_password = archive
                .get_aes_verification_key_and_salt(0)
                .unwrap()
                .is_none()
                == false;

            if needs_password && password.is_none() {
                self.show_password_dialog = true;
                self.current_operation = Some(PasswordOperation::ExtractFile(file_name));
                self.status_message = "File is encrypted. Please enter password.".to_string();
                return Ok(());
            }

            let temp_dir = get_temp_dir();
            std::fs::create_dir_all(&temp_dir)?;

            let archive_path = archive_path.clone();
            let progress_state = Arc::clone(&self.progress_state);

            thread::spawn(move || {
                let file = File::open(&archive_path).unwrap();
                let mut archive = ZipArchive::new(file).unwrap();

                let mut zip_file = if needs_password {
                    archive.by_name_decrypt(&file_name, password.unwrap().as_bytes()).unwrap()
                } else {
                    archive.by_name(&file_name).unwrap()
                };

                let total_size = zip_file.size();
                let temp_path = temp_dir.join(&file_name);

                if let Some(parent) = temp_path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }

                let mut temp_file = File::create(&temp_path).unwrap();
                let mut buffer = [0; 8192];
                let mut processed_size = 0;
                let start_time = Instant::now();

                while let Ok(n) = zip_file.read(&mut buffer) {
                    if n == 0 {
                        break;
                    }
                    temp_file.write_all(&buffer[..n]).unwrap();
                    processed_size += n as u64;

                    let elapsed = start_time.elapsed();
                    let progress = processed_size as f32 / total_size as f32;
                    let estimated_time = if progress > 0.0 {
                        Duration::from_secs_f32(elapsed.as_secs_f32() / progress)
                    } else {
                        Duration::from_secs(0)
                    };

                    if let Ok(mut state) = progress_state.lock() {
                        state.extraction_progress = Some((
                            progress,
                            ExtractionStats {
                                original_size: total_size,
                                extracted_size: processed_size,
                                start_time,
                                estimated_time,
                                current_file: file_name.clone(),
                            },
                        ));
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

    pub fn compress_files(&mut self) -> Result<(), Box<dyn std::error::Error>> {
        self.show_password_dialog = true;
        self.current_operation = Some(PasswordOperation::Compress);
        Ok(())
    }
    pub fn open_file(&mut self, file_name: String) -> Result<(), Box<dyn std::error::Error>> {
        if let Some((archive_path, _)) = &self.current_archive {
            let file = File::open(archive_path)?;
            let mut archive = ZipArchive::new(file)?;

            let needs_password = archive
                .get_aes_verification_key_and_salt(0)
                .unwrap()
                .is_none()
                == false;

            if needs_password {
                self.show_password_dialog = true;
                self.current_operation = Some(PasswordOperation::ExtractFile(file_name));
                self.status_message = "File is encrypted. Please enter password.".to_string();
                return Ok(());
            }

            let temp_dir = get_temp_dir();
            std::fs::create_dir_all(&temp_dir)?;

            let archive_path = archive_path.clone();
            let progress_state = Arc::clone(&self.progress_state);

            thread::spawn(move || {
                let file = File::open(&archive_path).unwrap();
                let mut archive = ZipArchive::new(file).unwrap();
                let mut zip_file = archive.by_name(&file_name).unwrap();
                let total_size = zip_file.size();
                let temp_path = temp_dir.join(&file_name);

                if let Some(parent) = temp_path.parent() {
                    std::fs::create_dir_all(parent).unwrap();
                }

                let mut temp_file = File::create(&temp_path).unwrap();
                let mut buffer = [0; 8192];
                let mut processed_size = 0;
                let start_time = Instant::now();

                while let Ok(n) = zip_file.read(&mut buffer) {
                    if n == 0 {
                        break;
                    }
                    temp_file.write_all(&buffer[..n]).unwrap();
                    processed_size += n as u64;

                    let elapsed = start_time.elapsed();
                    let progress = processed_size as f32 / total_size as f32;
                    let estimated_time = if progress > 0.0 {
                        Duration::from_secs_f32(elapsed.as_secs_f32() / progress)
                    } else {
                        Duration::from_secs(0)
                    };

                    if let Ok(mut state) = progress_state.lock() {
                        state.extraction_progress = Some((
                            progress,
                            ExtractionStats {
                                original_size: total_size,
                                extracted_size: processed_size,
                                start_time,
                                estimated_time,
                                current_file: file_name.clone(),
                            },
                        ));
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

        if needs_password {
            self.show_password_dialog = true;
            self.current_operation = Some(PasswordOperation::OpenArchive(path.to_path_buf()));
            self.status_message = "Archive is encrypted. Please enter password.".to_string();
            return Ok(());
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

    pub fn handle_file_drop(
        &mut self,
        path: &Path,
    ) -> Result<(), Box<dyn std::error::Error>> {
        info!("Handling file drop: {:?}", path);

        let extension = path.extension().and_then(|ext| ext.to_str()).unwrap_or("");
        info!("File extension: {}", extension);

        match extension {
            "zip" | "7z" | "rar" => {
                if self.remember_archive_choice {
                    // If we're remembering the choice, follow the last decision
                    if let Some(compress) = self.last_archive_choice {
                        if compress {
                            info!("Adding archive to compression list (remembered choice)");
                            self.selected_files.push(path.to_path_buf());
                            self.status_message = "Archive added to compression list".to_string();
                        } else {
                            info!("Opening archive for viewing (remembered choice)");
                            if let Err(e) = self.open_archive(path) {
                                self.status_message = format!("Error opening archive: {}", e);
                            }
                        }
                    } else {
                        // If no previous choice, show dialog
                        self.pending_archive_path = Some(path.to_path_buf());
                        self.show_action_dialog = true;
                    }
                } else {
                    // Always show dialog if not remembering choice
                    self.pending_archive_path = Some(path.to_path_buf());
                    self.show_action_dialog = true;
                }
            }
            _ => {
                info!("Adding file to compression list");
                self.selected_files.push(path.to_path_buf());
                self.status_message = "File added to compression list".to_string();
            }
        }
        Ok(())
    }


    pub fn handle_drops(&mut self, ctx: &egui::Context) {
        // Store the zone at the start of the frame
        let drop_zone = self.compress_zone.rect;

        ctx.input(|i| {
            let pointer_pos = i.pointer.hover_pos();

            if !i.raw.dropped_files.is_empty() {
                info!("Files dropped: {} files", i.raw.dropped_files.len());

                // Check if pointer is in drop zone
                let in_drop_zone = match (pointer_pos, drop_zone) {
                    (Some(pos), Some(rect)) => {
                        info!("Checking drop zone - pointer: {:?}, zone: {:?}", pos, rect);
                        rect.contains(pos)
                    }
                    _ => {
                        warn!("Could not determine drop zone - using default (true)");
                        true
                    }
                };

                if in_drop_zone {
                    for dropped_file in &i.raw.dropped_files {
                        if let Some(path) = &dropped_file.path {
                            info!("Processing dropped file: {:?}", path);

                            match self.handle_file_drop(path) {
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
            }
        });
    }
}

