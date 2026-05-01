use std::path::{Path, PathBuf};
use std::sync::Once;

use futures_util::StreamExt;
use gstreamer as gst;
use gstreamer::prelude::*;
use tokio::sync::mpsc::UnboundedSender;
use tokio::sync::oneshot;

const DB_FLOOR: f64 = -60.0;

static GST_INIT: Once = Once::new();

fn ensure_gst_init() -> Result<(), String> {
    let mut err: Option<String> = None;
    GST_INIT.call_once(|| {
        if let Err(e) = gst::init() {
            err = Some(e.to_string());
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

        let element = gst::parse::launch(&pipeline_str).map_err(|e| e.to_string())?;
        let pipeline = element
            .downcast::<gst::Pipeline>()
            .map_err(|_| "expected pipeline".to_string())?;
        let bus = pipeline.bus().ok_or("pipeline has no bus")?;

        let (eos_tx, eos_rx) = oneshot::channel();

        tokio::spawn(async move {
            let mut stream = bus.stream();
            let mut eos_tx = Some(eos_tx);
            while let Some(msg) = stream.next().await {
                use gst::MessageView;
                match msg.view() {
                    MessageView::Element(elem) => {
                        if let Some(s) = elem.structure() {
                            if s.has_name("level") {
                                if let Ok(arr) = s.get::<glib::ValueArray>("peak") {
                                    if let Some(v) = arr.as_slice().first() {
                                        if let Ok(db) = v.get::<f64>() {
                                            let normalized =
                                                ((db - DB_FLOOR) / -DB_FLOOR).clamp(0.0, 1.0);
                                            let _ = level_tx.send(normalized);
                                        }
                                    }
                                }
                            }
                        }
                    }
                    MessageView::Eos(_) => {
                        if let Some(tx) = eos_tx.take() {
                            let _ = tx.send(Ok(()));
                        }
                        break;
                    }
                    MessageView::Error(e) => {
                        let msg = format!("{}: {}", e.error(), e.debug().unwrap_or_default());
                        if let Some(tx) = eos_tx.take() {
                            let _ = tx.send(Err(msg));
                        }
                        break;
                    }
                    _ => {}
                }
            }
        });

        pipeline
            .set_state(gst::State::Playing)
            .map_err(|e| e.to_string())?;

        Ok(Self {
            pipeline,
            output_path: output_path.to_path_buf(),
            eos_rx,
        })
    }

    pub async fn stop(self) -> Result<Vec<u8>, String> {
        self.pipeline.send_event(gst::event::Eos::new());

        let result = match self.eos_rx.await {
            Ok(r) => r,
            Err(_) => Err("bus task ended before EOS".to_string()),
        };
        let _ = self.pipeline.set_state(gst::State::Null);
        result?;

        std::fs::read(&self.output_path).map_err(|e| e.to_string())
    }

    pub fn cancel(&self) {
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
