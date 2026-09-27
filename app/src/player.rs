use std::path::Path;

use gstreamer as gst;
use gstreamer::prelude::*;
use log::{debug, error, warn};

// Plays a single audio file via gstreamer's playbin. The `on_finished` closure
// runs on the GLib main context when the file ends or errors out, so callers
// can toggle a "stop" button back to "play".
pub struct Player {
    pipeline: gst::Element,
    // Keep the bus watch alive for the player's lifetime; dropping it removes
    // the watch and we'd never see EOS/Error.
    _watch: gst::bus::BusWatchGuard,
}

impl Player {
    pub fn play<F: Fn() + 'static>(path: &Path, on_finished: F) -> Result<Player, String> {
        // gst::init is idempotent; the recorder may already have done it.
        gst::init().map_err(|e| e.to_string())?;

        if !path.exists() {
            return Err(format!("audio file not found: {}", path.display()));
        }

        let uri = format!("file://{}", path.to_string_lossy());
        debug!("player: playing {uri}");

        let pipeline = gst::ElementFactory::make("playbin")
            .property("uri", &uri)
            .build()
            .map_err(|e| format!("playbin make: {e}"))?;

        let bus = pipeline
            .bus()
            .ok_or_else(|| "playbin has no bus".to_string())?;

        let pipeline_weak = pipeline.downgrade();
        let watch = bus
            .add_watch_local(move |_bus, msg| {
                use gst::MessageView;
                match msg.view() {
                    MessageView::Eos(_) => {
                        debug!("player: EOS");
                        if let Some(p) = pipeline_weak.upgrade() {
                            let _ = p.set_state(gst::State::Null);
                        }
                        on_finished();
                        glib::ControlFlow::Break
                    }
                    MessageView::Error(e) => {
                        error!(
                            "player: pipeline error: {}: {}",
                            e.error(),
                            e.debug().unwrap_or_default()
                        );
                        if let Some(p) = pipeline_weak.upgrade() {
                            let _ = p.set_state(gst::State::Null);
                        }
                        on_finished();
                        glib::ControlFlow::Break
                    }
                    _ => glib::ControlFlow::Continue,
                }
            })
            .map_err(|e| e.to_string())?;

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| format!("set_state(Playing): {e}"))?;

        Ok(Player {
            pipeline,
            _watch: watch,
        })
    }
}

impl Drop for Player {
    fn drop(&mut self) {
        debug!("player: dropped \u{2014} stopping pipeline");
        if let Err(e) = self.pipeline.set_state(gst::State::Null) {
            warn!("player: set_state(Null) failed: {e}");
        }
    }
}
