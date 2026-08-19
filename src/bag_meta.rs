use anyhow::{Context, Result};
use chrono::{DateTime, Local};
use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TopicMetadata {
    pub name: String,
    #[serde(rename = "type")]
    pub type_name: String,
    pub serialization_format: Option<String>,
    pub offered_qos_profiles: Option<String>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct TopicWithCount {
    pub topic_metadata: TopicMetadata,
    pub message_count: u64,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct Rosbag2Info {
    pub storage_identifier: Option<String>,
    pub relative_file_paths: Option<Vec<String>>,
    pub starting_time: Option<StartingTime>,
    pub duration: Option<DurationInfo>,
    pub message_count: Option<u64>,
    pub topics_with_message_count: Option<Vec<TopicWithCount>>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct StartingTime {
    pub nanoseconds_since_epoch: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct DurationInfo {
    pub nanoseconds: Option<u64>,
}

#[derive(Debug, Deserialize, Serialize, Clone)]
pub struct RosbagMetadataWrapper {
    pub rosbag2_bagfile_information: Rosbag2Info,
}

#[derive(Debug, Clone)]
pub struct ParsedBagSummary {
    pub bag_name: String,
    pub bag_path: PathBuf,
    pub total_raw_size_bytes: u64,
    pub storage_format: String,
    pub message_count: u64,
    pub duration_sec: f64,
    pub start_time_str: String,
    pub topics: Vec<(String, String, u64)>, // (topic_name, type, count)
}

pub fn find_metadata_yaml(bag_path: &Path) -> Result<PathBuf> {
    if bag_path.is_file() {
        if let Some(parent) = bag_path.parent() {
            let meta_in_parent = parent.join("metadata.yaml");
            if meta_in_parent.exists() {
                return Ok(meta_in_parent);
            }
        }
        anyhow::bail!("Path is a file, but no metadata.yaml found in parent directory");
    }

    let meta_file = bag_path.join("metadata.yaml");
    if meta_file.exists() {
        return Ok(meta_file);
    }
    anyhow::bail!("metadata.yaml not found in {}", bag_path.display());
}

pub fn calculate_dir_size(path: &Path) -> u64 {
    if path.is_file() {
        return path.metadata().map(|m| m.len()).unwrap_or(0);
    }
    let mut total = 0;
    if let Ok(entries) = fs::read_dir(path) {
        for entry in entries.flatten() {
            let p = entry.path();
            if p.is_dir() {
                total += calculate_dir_size(&p);
            } else if let Ok(meta) = p.metadata() {
                total += meta.len();
            }
        }
    }
    total
}

pub fn parse_bag_metadata(bag_path: &Path) -> Result<ParsedBagSummary> {
    let bag_dir = if bag_path.is_file() {
        bag_path.parent().unwrap_or(bag_path)
    } else {
        bag_path
    };

    let bag_name = bag_dir
        .file_name()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| "rosbag".to_string());

    let meta_file = find_metadata_yaml(bag_dir)?;
    let content = fs::read_to_string(&meta_file)
        .with_context(|| format!("Failed to read metadata file: {}", meta_file.display()))?;

    let wrapper: RosbagMetadataWrapper = serde_yaml::from_str(&content)
        .with_context(|| "Failed to parse metadata.yaml as ROS 2 metadata")?;

    let info = wrapper.rosbag2_bagfile_information;
    let storage_format = info.storage_identifier.unwrap_or_else(|| "sqlite3/mcap".to_string());
    let message_count = info.message_count.unwrap_or(0);

    let duration_sec = info
        .duration
        .and_then(|d| d.nanoseconds)
        .map(|ns| ns as f64 / 1_000_000_000.0)
        .unwrap_or(0.0);

    let start_time_str = if let Some(st) = info.starting_time.and_then(|t| t.nanoseconds_since_epoch) {
        let sec = (st / 1_000_000_000) as i64;
        let nsec = (st % 1_000_000_000) as u32;
        if let Some(utc) = DateTime::from_timestamp(sec, nsec) {
            let local: DateTime<Local> = DateTime::from(utc);
            local.format("%Y-%m-%d %H:%M:%S").to_string()
        } else {
            "Unknown".to_string()
        }
    } else {
        "Unknown".to_string()
    };

    let mut topics = Vec::new();
    if let Some(list) = info.topics_with_message_count {
        for t in list {
            topics.push((
                t.topic_metadata.name,
                t.topic_metadata.type_name,
                t.message_count,
            ));
        }
    }
    // Sort topics by message count descending
    topics.sort_by(|a, b| b.2.cmp(&a.2));

    let total_raw_size_bytes = calculate_dir_size(bag_dir);

    Ok(ParsedBagSummary {
        bag_name,
        bag_path: bag_dir.to_path_buf(),
        total_raw_size_bytes,
        storage_format,
        message_count,
        duration_sec,
        start_time_str,
        topics,
    })
}
