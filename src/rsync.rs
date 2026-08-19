use anyhow::{Context, Result};
use std::path::Path;
use std::process::Command;

/// Transmit compressed archive file or directory to remote target using rsync.
pub fn sync_to_remote(source_path: &Path, remote_target: &str) -> Result<()> {
    let mut cmd = Command::new("rsync");
    
    // -a: archive mode, -v: verbose, -z: compress during transfer (safe fallback), -P: progress/partial
    cmd.args(["-avP", "--progress"]);
    cmd.arg(source_path);
    cmd.arg(remote_target);

    let status = cmd.status()
        .context("Failed to execute 'rsync'. Please make sure rsync is installed on your machine.")?;

    if !status.success() {
        anyhow::bail!("rsync process failed with exit code: {:?}", status.code());
    }

    Ok(())
}
