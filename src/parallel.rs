use std::fs::File;
use std::io::{Read, Write, BufReader, BufWriter};
use std::path::PathBuf;
use std::sync::mpsc::Sender;
use std::sync::{Arc, Mutex};
use rayon::prelude::*;
use zip::write::FileOptions;
use crate::app::CompressionStats;

const BUFFER_SIZE: usize = 1024 * 1024; // 1MB buffer
const COMPRESSION_LEVEL: i32 = 5; // Faster compression, still decent ratio

pub fn compress_files_parallel(
    files: Vec<PathBuf>,
    output_path: PathBuf,
    progress_tx: Sender<(f32, CompressionStats)>,
    cancel_rx: Arc<Mutex<Sender<()>>>,
    stats: Arc<Mutex<CompressionStats>>,
) -> Result<(), Box<dyn std::error::Error>> {
    let total_size: u64 = files.iter()
        .filter_map(|path| std::fs::metadata(path).ok())
        .map(|meta| meta.len())
        .sum();

    // Use BufWriter for better write performance
    let file = BufWriter::new(File::create(&output_path)?);
    let zip = Arc::new(Mutex::new(zip::ZipWriter::new(file)));
    let processed_size = Arc::new(Mutex::new(0u64));

    // Pre-calculate file metadata to avoid redundant filesystem operations
    let file_metadata: Vec<_> = files.iter()
        .filter_map(|path| {
            std::fs::metadata(path)
                .ok()
                .map(|meta| (path.clone(), meta.len()))
        })
        .collect();

    // Process files in chunks for better parallelization
    file_metadata.par_chunks(4).try_for_each(|chunk| -> Result<(), Box<dyn std::error::Error + Send + Sync>> {
        let mut buffer = Vec::with_capacity(BUFFER_SIZE);

        for (path, file_size) in chunk {
            // Check cancellation
            if cancel_rx.lock().unwrap().send(()).is_ok() {
                return Ok(());
            }

            let file = File::open(path)?;
            let mut reader = BufReader::with_capacity(BUFFER_SIZE, file);
            buffer.clear();
            reader.read_to_end(&mut buffer)?;

            let file_name = path.file_name()
                .unwrap_or_default()
                .to_string_lossy()
                .into_owned();

            let options: FileOptions<'_, ()> = FileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated)
                .compression_level(Some(COMPRESSION_LEVEL as i64))
                .unix_permissions(0o755);

            // Minimize lock contention by reducing the critical section
            {
                let mut zip = zip.lock().unwrap();
                zip.start_file(&file_name, options)?;
                zip.write_all(&buffer)?;
            }

            // Update progress
            let mut processed = processed_size.lock().unwrap();
            *processed += file_size;
            let progress = *processed as f32 / total_size as f32;

            // Update stats less frequently to reduce lock contention
            if progress - (progress * 100.0).floor() / 100.0 < 0.01 {
                if let Ok(mut stats) = stats.lock() {
                    let elapsed = stats.start_time.elapsed();
                    stats.estimated_time = if progress > 0.0 {
                        std::time::Duration::from_secs_f32(elapsed.as_secs_f32() / progress)
                    } else {
                        std::time::Duration::from_secs(0)
                    };

                    let stats_clone = (*stats).clone();
                    progress_tx.send((progress, stats_clone))?;
                }
            }
        }
        Ok(())
    }).expect("Failed to parallel");

    // Finalize the zip file
    let final_zip = Arc::try_unwrap(zip)
        .map_err(|_| "Could not get exclusive ownership of zip writer")?
        .into_inner()?;
    let file = final_zip.finish()?;
    let compressed_size = file.into_inner()?.metadata()?.len();

    if let Ok(mut stats) = stats.lock() {
        stats.compressed_size = compressed_size;
        let stats_clone = (*stats).clone();
        progress_tx.send((1.0, stats_clone))?;
    }

    Ok(())
}