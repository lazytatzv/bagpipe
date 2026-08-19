use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::PathBuf;

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct Config {
    pub webhook_url: Option<String>,
    pub rsync_target: Option<String>, // e.g. "user@server:/path/to/bags"
    pub max_file_size_mb: Option<u64>,
    pub zstd_level: Option<i32>,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            webhook_url: None,
            rsync_target: None,
            max_file_size_mb: Some(25), // Discord standard max upload size
            zstd_level: None,           // None = Smart Adaptive Auto-Tuning
        }
    }
}

pub fn get_config_path() -> Result<PathBuf> {
    let base = dirs::config_dir().context("Failed to find config directory")?;
    let path = base.join("bagpipe").join("config.json");
    Ok(path)
}

pub fn load_config() -> Config {
    if let Ok(path) = get_config_path() {
        if path.exists() {
            if let Ok(content) = fs::read_to_string(&path) {
                if let Ok(cfg) = serde_json::from_str::<Config>(&content) {
                    return cfg;
                }
            }
        }
    }
    Config::default()
}

pub fn save_config(config: &Config) -> Result<()> {
    let path = get_config_path()?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(config)?;
    fs::write(path, json)?;
    Ok(())
}
