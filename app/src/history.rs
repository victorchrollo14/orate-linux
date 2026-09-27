use std::fs;
use std::path::PathBuf;

use chrono::{DateTime, Duration, Utc};
use log::{debug, warn};
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct HistoryEntry {
    pub id: String,
    pub timestamp: DateTime<Utc>,
    pub transcript: String,
    pub latency_ms: u64,
    // Optional so older JSON files still deserialize cleanly.
    #[serde(default)]
    pub words_remaining: Option<u64>,
}

impl HistoryEntry {
    pub fn audio_path(&self) -> PathBuf {
        history_dir().join(format!("{}.flac", self.id))
    }

    pub fn has_audio(&self) -> bool {
        self.audio_path().exists()
    }
}

pub fn history_dir() -> PathBuf {
    let base = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_default();
            home.join(".local/share")
        });
    base.join("orate/history")
}

pub fn save(
    transcript: &str,
    latency_ms: u64,
    audio: Option<&[u8]>,
    words_remaining: Option<u64>,
) -> Result<HistoryEntry, String> {
    let dir = history_dir();
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;

    let now = Utc::now();
    let id = format!(
        "{}-{:09}",
        now.format("%Y%m%dT%H%M%S"),
        now.timestamp_subsec_nanos()
    );

    if let Some(bytes) = audio {
        let audio_path = dir.join(format!("{id}.flac"));
        if let Err(e) = fs::write(&audio_path, bytes) {
            warn!("history: failed to save audio for {id}: {e}");
        } else {
            debug!("history: wrote {} bytes to {}", bytes.len(), audio_path.display());
        }
    }

    let entry = HistoryEntry {
        id: id.clone(),
        timestamp: now,
        transcript: transcript.to_string(),
        latency_ms,
        words_remaining,
    };

    let path = dir.join(format!("{id}.json"));
    let json = serde_json::to_string_pretty(&entry).map_err(|e| e.to_string())?;
    fs::write(&path, json).map_err(|e| e.to_string())?;
    Ok(entry)
}

pub fn load_all() -> Vec<HistoryEntry> {
    let dir = history_dir();
    let Ok(read_dir) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    let mut result: Vec<HistoryEntry> = read_dir
        .filter_map(|e| e.ok())
        .filter(|e| e.path().extension().and_then(|s| s.to_str()) == Some("json"))
        .filter_map(|e| {
            let data = fs::read_to_string(e.path()).ok()?;
            serde_json::from_str::<HistoryEntry>(&data).ok()
        })
        .collect();
    result.sort_by(|a, b| b.timestamp.cmp(&a.timestamp));
    result
}

pub fn delete_older_than(days: Option<i64>) -> usize {
    let dir = history_dir();
    let Ok(read_dir) = fs::read_dir(&dir) else {
        return 0;
    };
    let cutoff = days.map(|d| Utc::now() - Duration::days(d));
    let mut count = 0;
    for entry in read_dir.filter_map(|e| e.ok()) {
        let path = entry.path();
        if path.extension().and_then(|s| s.to_str()) != Some("json") {
            continue;
        }
        let should_delete = match cutoff {
            None => true,
            Some(cutoff_ts) => {
                let Ok(data) = fs::read_to_string(&path) else { continue };
                let Ok(entry): Result<HistoryEntry, _> = serde_json::from_str(&data) else {
                    continue;
                };
                entry.timestamp < cutoff_ts
            }
        };
        if should_delete {
            // remove the sibling FLAC too if present
            let flac = path.with_extension("flac");
            if flac.exists() {
                let _ = fs::remove_file(&flac);
            }
            if fs::remove_file(&path).is_ok() {
                count += 1;
            }
        }
    }
    count
}
