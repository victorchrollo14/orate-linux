use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use chrono::Local;
use log::{Level, Log, Metadata, Record};

pub fn state_dir() -> PathBuf {
    let base = std::env::var_os("XDG_STATE_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            home.join(".local/state")
        });
    base.join("orate")
}

pub fn log_path() -> PathBuf {
    state_dir().join("orate.log")
}

struct OrateLogger {
    file: Mutex<Option<File>>,
    max_level: Level,
}

impl Log for OrateLogger {
    fn enabled(&self, metadata: &Metadata) -> bool {
        metadata.level() <= self.max_level
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        let ts = Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let module = record.module_path().unwrap_or("orate");
        let line = format!(
            "{ts} {:<5} {module} \u{2014} {}",
            record.level(),
            record.args()
        );
        eprintln!("{line}");
        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = writeln!(file, "{line}");
                let _ = file.flush();
            }
        }
    }

    fn flush(&self) {
        if let Ok(mut guard) = self.file.lock() {
            if let Some(file) = guard.as_mut() {
                let _ = file.flush();
            }
        }
    }
}

// Initialize once. Idempotent: extra calls are silently ignored so callers in
// both the GUI and --service entry points can call init() unconditionally.
pub fn init() {
    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    let file = OpenOptions::new()
        .create(true)
        .append(true)
        .open(&path)
        .ok();

    let max_level = match std::env::var("ORATE_LOG").as_deref() {
        Ok("trace") => Level::Trace,
        Ok("debug") => Level::Debug,
        Ok("info") => Level::Info,
        Ok("warn") => Level::Warn,
        Ok("error") => Level::Error,
        _ => Level::Debug,
    };

    let logger = OrateLogger {
        file: Mutex::new(file),
        max_level,
    };

    if log::set_boxed_logger(Box::new(logger)).is_ok() {
        log::set_max_level(max_level.to_level_filter());
        log::info!("logger initialized \u{2014} writing to {}", path.display());
    }
}
