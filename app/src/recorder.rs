use std::path::{Path, PathBuf};
use std::sync::Once;

use futures_util::StreamExt;
use gstreamer as gst;
use gstreamer::prelude::*;
use log::{debug, error, info, trace, warn};
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

const DB_FLOOR: f64 = -60.0;
// log only every N level messages to avoid flooding the log file at ~30 Hz
const LEVEL_LOG_EVERY: u32 = 30;

static GST_INIT: Once = Once::new();

fn ensure_gst_init() -> Result<(), String> {
    let mut err: Option<String> = None;
    GST_INIT.call_once(|| {
        match gst::init() {
            Ok(()) => info!("gstreamer initialized (version {})", gst::version_string()),
            Err(e) => err = Some(e.to_string()),
        }
    });
    if let Some(e) = err {
        return Err(e);
    }
    Ok(())
}

pub struct Recorder {
    pipeline: gst::Pipeline,
    output_path: PathBuf,
    eos_rx: oneshot::Receiver<Result<(), String>>,
}

impl Recorder {
    pub fn start(output_path: &Path, level_tx: UnboundedSender<f64>) -> Result<Self, String> {
        ensure_gst_init()?;

        // pulsesrc routes through PipeWire's Pulse compat layer on Fedora, picking up
        // the user's default audio source (settable via wpctl / GNOME Sound settings).
        // Direct pipewiresrc requires an explicit target-object and is fragile.
        let pipeline_str = format!(
            "pulsesrc ! audioconvert ! audioresample ! \
             audio/x-raw,format=S16LE,rate=16000,channels=1 ! \
             level interval=33000000 post-messages=true ! \
             flacenc ! filesink location={}",
            gst_quote(output_path)
        );
        info!(
            "starting recorder pipeline -> {} ({})",
            output_path.display(),
            pipeline_str
        );

        let element = gst::parse::launch(&pipeline_str).map_err(|e| {
            error!("gst::parse::launch failed: {e}");
            e.to_string()
        })?;
        let pipeline = element
            .downcast::<gst::Pipeline>()
            .map_err(|_| "expected pipeline".to_string())?;

        // Log which audio source pulsesrc actually opens (e.g. monitor vs. mic).
        // Helps when the transcription seems "wrong" because the wrong device
        // was being captured.
        if let Some(src) = pipeline.by_name("pulsesrc0") {
            let device = src.property::<Option<String>>("device").unwrap_or_default();
            let actual = src
                .property::<Option<String>>("current-device")
                .unwrap_or_default();
            info!(
                "pulsesrc device requested={:?} (actual will appear after PLAYING; current-device={:?})",
                device, actual
            );
        }

        let bus = pipeline.bus().ok_or("pipeline has no bus")?;
        let (eos_tx, eos_rx) = oneshot::channel();

        tokio::spawn(async move {
            let mut stream = bus.stream();
            let mut eos_tx = Some(eos_tx);
            let mut level_count: u32 = 0;
            while let Some(msg) = stream.next().await {
                use gst::MessageView;
                match msg.view() {
                    MessageView::Element(elem) => {
                        let Some(s) = elem.structure() else { continue };
                        if !s.has_name("level") {
                            continue;
                        }
                        match s.get::<glib::ValueArray>("peak") {
                            Ok(arr) => {
                                let Some(v) = arr.as_slice().first() else {
                                    warn!("level message had empty peak array");
                                    continue;
                                };
                                match v.get::<f64>() {
                                    Ok(db) => {
                                        let normalized =
                                            ((db - DB_FLOOR) / -DB_FLOOR).clamp(0.0, 1.0);
                                        level_count = level_count.wrapping_add(1);
                                        if level_count % LEVEL_LOG_EVERY == 1 {
                                            trace!(
                                                "level peak={db:.1} dB normalized={normalized:.3}"
                                            );
                                        }
                                        if level_tx.send(normalized).is_err() {
                                            debug!("level receiver dropped; ending bus task");
                                            break;
                                        }
                                    }
                                    Err(e) => warn!("level peak[0] not f64: {e}"),
                                }
                            }
                            Err(e) => warn!("level message had no peak GValueArray: {e}"),
                        }
                    }
                    MessageView::StateChanged(sc) => {
                        if sc
                            .src()
                            .and_then(|o| o.downcast_ref::<gst::Pipeline>().map(|_| ()))
                            .is_some()
                        {
                            debug!(
                                "pipeline state: {:?} -> {:?}",
                                sc.old(),
                                sc.current()
                            );
                        }
                    }
                    MessageView::Eos(_) => {
                        info!("pipeline EOS");
                        if let Some(tx) = eos_tx.take() {
                            let _ = tx.send(Ok(()));
                        }
                        break;
                    }
                    MessageView::Error(e) => {
                        let msg = format!("{}: {}", e.error(), e.debug().unwrap_or_default());
                        error!("pipeline error: {msg}");
                        if let Some(tx) = eos_tx.take() {
                            let _ = tx.send(Err(msg));
                        }
                        break;
                    }
                    MessageView::Warning(w) => {
                        warn!(
                            "pipeline warning: {}: {}",
                            w.error(),
                            w.debug().unwrap_or_default()
                        );
                    }
                    _ => {}
                }
            }
            debug!("bus task ended (got {} level msgs)", level_count);
        });

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| {
                error!("pipeline set_state(Playing) failed: {e}");
                e.to_string()
            })?;
        debug!("pipeline set_state(Playing) accepted");

        Ok(Self {
            pipeline,
            output_path: output_path.to_path_buf(),
            eos_rx,
        })
    }

    pub async fn stop(self) -> Result<Vec<u8>, String> {
        info!("stopping recorder; sending EOS");
        self.pipeline.send_event(gst::event::Eos::new());

        let result = match self.eos_rx.await {
            Ok(r) => r,
            Err(_) => Err("bus task ended before EOS".to_string()),
        };
        let _ = self.pipeline.set_state(gst::State::Null);
        result?;

        let bytes = std::fs::read(&self.output_path).map_err(|e| {
            error!(
                "failed to read recorded file {}: {e}",
                self.output_path.display()
            );
            e.to_string()
        })?;
        info!(
            "recorded {} bytes from {}",
            bytes.len(),
            self.output_path.display()
        );
        if bytes.len() < 1024 {
            warn!(
                "recording is suspiciously small ({} bytes) \u{2014} the mic may have captured nothing",
                bytes.len()
            );
        }
        Ok(bytes)
    }

    pub fn cancel(&self) {
        info!("recorder cancelled \u{2014} setting pipeline to NULL");
        let _ = self.pipeline.set_state(gst::State::Null);
    }
}

// Pipeline-string filename quoting: parse_launch tokenizes on whitespace,
// so spaces in the path would break it. We restrict to typical tmp dirs.
fn gst_quote(path: &Path) -> String {
    let s = path.to_string_lossy();
    if s.contains(char::is_whitespace) {
        format!("\"{}\"", s.replace('"', "\\\""))
    } else {
        s.into_owned()
    }
}
