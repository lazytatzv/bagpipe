use anyhow::{Context, Result};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use tar::Builder;
use crate::bag_meta::ParsedBagSummary;

/// Dynamically determine optimal zstd compression level based on bag characteristics and target limit.
pub fn determine_optimal_zstd_level(summary: &ParsedBagSummary, max_upload_mb: u64) -> (i32, &'static str) {
    let raw_mb = summary.total_raw_size_bytes as f64 / (1024.0 * 1024.0);
    let target_mb = max_upload_mb as f64;

    // If already very small (< 10MB), ultra-fast level 1 is plenty
    if raw_mb <= 10.0 {
        return (1, "Ultra-fast (small bag)");
    }

    // Check data composition
    let mut image_count = 0;
    let mut compressed_image_count = 0;
    let mut lidar_pointcloud_count = 0;
    let mut general_count = 0;

    for (_, type_name, count) in &summary.topics {
        if type_name.contains("CompressedImage") {
            compressed_image_count += count;
        } else if type_name.contains("Image") {
            image_count += count;
        } else if type_name.contains("PointCloud2") || type_name.contains("LaserScan") {
            lidar_pointcloud_count += count;
        } else {
            general_count += count;
        }
    }

    let total_msgs = summary.message_count.max(1) as f64;
    let compressed_ratio = compressed_image_count as f64 / total_msgs;

    // If bag is borderline (25MB ~ 100MB), aggressively compress (Level 7~11) to fit in Discord limit
    if raw_mb > target_mb && raw_mb <= target_mb * 3.5 {
        if compressed_ratio > 0.6 {
            // Mostly JPEG/PNG already compressed, higher zstd won't help much, level 3 is optimal
            (3, "Fast (pre-compressed data dominant)")
        } else {
            (9, "High compression (adaptive fit to Discord limit)")
        }
    } else if raw_mb > target_mb * 3.5 {
        // Way larger than Discord upload limit (> 85MB):
        // It won't fit anyway, so don't waste CPU time, compress fast at level 3 for local storage
        (3, "Balanced speed (oversized bag)")
    } else {
        // Fits comfortably under Discord limit (< 25MB)
        (3, "Balanced")
    }
}

/// Compresses a target bag directory into a single .tar.zst archive file.
pub fn compress_bag_dir(bag_dir: &Path, output_path: &Path, zstd_level: i32) -> Result<u64> {
    let bag_dir = bag_dir.canonicalize().context("Failed to canonicalize bag directory")?;
    let dir_name = bag_dir.file_name().context("Failed to get bag directory name")?;

    let out_file = File::create(output_path)
        .with_context(|| format!("Failed to create output file: {}", output_path.display()))?;
    let buf_writer = BufWriter::with_capacity(1024 * 1024, out_file);

    // Initialize zstd encoder
    let mut encoder = zstd::stream::write::Encoder::new(buf_writer, zstd_level)
        .context("Failed to initialize zstd encoder")?;
    
    // Enable multi-threading if level permits
    let _ = encoder.set_parameter(zstd::zstd_safe::CParameter::NbWorkers(0));

    {
        let mut tar_builder = Builder::new(&mut encoder);
        tar_builder.append_dir_all(dir_name, &bag_dir)
            .with_context(|| format!("Failed to archive directory {}", bag_dir.display()))?;
        tar_builder.finish()?;
    }

    let mut buf_writer = encoder.finish()?;
    use std::io::Write;
    buf_writer.flush()?;

    let compressed_size = output_path.metadata()?.len();
    Ok(compressed_size)
}
