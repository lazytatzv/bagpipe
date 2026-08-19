use anyhow::{Context, Result};
use std::fs::File;
use std::io::BufWriter;
use std::path::Path;
use tar::Builder;

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
