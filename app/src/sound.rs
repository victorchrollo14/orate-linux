// Short audible cues for the record/transcribe cycle, mirroring the macOS
// app's start/stop/error sounds. Files come from the freedesktop sound theme
// so we inherit whatever the user's desktop already ships.

use std::path::PathBuf;

use gstreamer as gst;
use gstreamer::prelude::*;
use log::{debug, warn};

use crate::config;

#[derive(Clone, Copy, Debug)]
pub enum Cue {
    Start,
    Stop,
    Error,
}

impl Cue {
    // Ordered by preference; the first file that exists wins.
    fn candidates(self) -> &'static [&'static str] {
        match self {
            Cue::Start => &["message.oga", "bell.oga", "dialog-information.oga"],
            Cue::Stop => &["complete.oga", "message-new-instant.oga", "bell.oga"],
            Cue::Error => &["dialog-error.oga", "dialog-warning.oga", "bell.oga"],
        }
    }
}

/// Fire-and-forget. Silently does nothing when the preference is off or no
/// sound theme is installed — a missing cue must never break a transcription.
pub fn play(cue: Cue) {
    if !config::sound_feedback() {
        return;
    }
    let Some(path) = resolve(cue) else {
        debug!("sound: no file found for {cue:?}");
        return;
    };
    play_file(path);
}

fn resolve(cue: Cue) -> Option<PathBuf> {
    for dir in theme_dirs() {
        for name in cue.candidates() {
            let path = dir.join(name);
            if path.exists() {
                return Some(path);
            }
        }
    }
    None
}

fn theme_dirs() -> Vec<PathBuf> {
    let data_dirs = std::env::var("XDG_DATA_DIRS")
        .unwrap_or_else(|_| "/usr/local/share:/usr/share".to_string());
    data_dirs
        .split(':')
        .filter(|s| !s.is_empty())
        .flat_map(|base| {
            let base = PathBuf::from(base);
            [
                base.join("sounds/freedesktop/stereo"),
                base.join("sounds/gnome/default/alerts"),
            ]
        })
        .collect()
}

// Runs the pipeline to completion on its own thread: the service process has a
// tokio runtime but no GLib main loop, so we block on the bus instead of using
// a watch the way `player::Player` does.
fn play_file(path: PathBuf) {
    std::thread::spawn(move || {
        if let Err(e) = gst::init() {
            warn!("sound: gstreamer init failed: {e}");
            return;
        }
        let uri = format!("file://{}", path.to_string_lossy());
        let pipeline = match gst::ElementFactory::make("playbin").property("uri", &uri).build() {
            Ok(p) => p,
            Err(e) => {
                warn!("sound: playbin make failed: {e}");
                return;
            }
        };
        if let Err(e) = pipeline.set_state(gst::State::Playing) {
            warn!("sound: set_state(Playing) failed: {e}");
            return;
        }
        if let Some(bus) = pipeline.bus() {
            // Cues are under a second; the timeout is only a safety net so a
            // stalled sink can never leak this thread.
            let _ = bus.timed_pop_filtered(
                gst::ClockTime::from_seconds(5),
                &[gst::MessageType::Eos, gst::MessageType::Error],
            );
        }
        let _ = pipeline.set_state(gst::State::Null);
        debug!("sound: finished {}", path.display());
    });
}
