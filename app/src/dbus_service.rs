use std::future::pending;
use std::path::PathBuf;
use std::sync::Arc;

use tokio::sync::{mpsc, Mutex};
use zbus::{connection, interface, object_server::SignalEmitter};

use crate::clipboard;
use crate::recorder::Recorder;
use crate::secret;
use crate::settings_window::ORATE_CLOUD_KEY;
use crate::transcription;

const BUS_NAME: &str = "com.orate.App.Service";
const OBJECT_PATH: &str = "/com/orate/App";

fn recording_path() -> PathBuf {
    std::env::temp_dir().join("orate_recording.flac")
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
#[allow(dead_code)]
pub enum State {
    Idle,
    Listening,
    Transcribing,
    Error,
}

impl State {
    fn as_str(self) -> &'static str {
        match self {
            State::Idle => "idle",
            State::Listening => "listening",
            State::Transcribing => "transcribing",
            State::Error => "error",
        }
    }
}

struct Service {
    state: Arc<Mutex<State>>,
    recorder: Arc<Mutex<Option<Recorder>>>,
}

#[interface(name = "com.orate.App1")]
impl Service {
    async fn start_recording(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let mut state = self.state.lock().await;
        if *state != State::Idle {
            eprintln!("[orate] StartRecording ignored (state={})", state.as_str());
            return Ok(());
        }

        let (level_tx, mut level_rx) = mpsc::unbounded_channel::<f64>();
        let path = recording_path();

        match Recorder::start(&path, level_tx) {
            Ok(rec) => {
                self.recorder.lock().await.replace(rec);
                *state = State::Listening;
                drop(state);
                eprintln!("[orate] state -> listening");
                let _ = Self::state_changed(&emitter, State::Listening.as_str()).await;

                let level_emitter = emitter.to_owned();
                tokio::spawn(async move {
                    while let Some(level) = level_rx.recv().await {
                        let _ = Service::level_update(&level_emitter, level).await;
                    }
                });
                Ok(())
            }
            Err(e) => {
                drop(state);
                eprintln!("[orate] recorder start failed: {e}");
                let _ = Self::error_occurred(&emitter, &format!("recorder: {e}")).await;
                let _ = Self::state_changed(&emitter, State::Idle.as_str()).await;
                Ok(())
            }
        }
    }

    async fn stop_recording(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let mut state = self.state.lock().await;
        if *state != State::Listening {
            eprintln!("[orate] StopRecording ignored (state={})", state.as_str());
            return Ok(());
        }
        *state = State::Transcribing;
        drop(state);
        eprintln!("[orate] state -> transcribing");
        let _ = Self::state_changed(&emitter, State::Transcribing.as_str()).await;

        let recorder = self.recorder.lock().await.take();
        let state_arc = self.state.clone();
        let emitter_owned = emitter.to_owned();
        tokio::spawn(async move {
            let recording = match recorder {
                Some(r) => r.stop().await,
                None => Err("no active recorder".to_string()),
            };
            match recording {
                Ok(bytes) => {
                    eprintln!("[orate] recorded {} bytes (FLAC)", bytes.len());
                    transcribe_and_copy(bytes, &emitter_owned).await;
                }
                Err(e) => {
                    eprintln!("[orate] recorder stop failed: {e}");
                    let _ =
                        Service::error_occurred(&emitter_owned, &format!("recorder: {e}")).await;
                }
            }
            *state_arc.lock().await = State::Idle;
            eprintln!("[orate] state -> idle");
            let _ = Service::state_changed(&emitter_owned, State::Idle.as_str()).await;
        });
        Ok(())
    }

    async fn cancel(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        let mut state = self.state.lock().await;
        if *state == State::Idle {
            return Ok(());
        }
        if let Some(r) = self.recorder.lock().await.take() {
            r.cancel();
        }
        *state = State::Idle;
        eprintln!("[orate] state -> idle (cancelled)");
        let _ = Self::state_changed(&emitter, State::Idle.as_str()).await;
        Ok(())
    }

    #[zbus(signal)]
    async fn state_changed(emitter: &SignalEmitter<'_>, state: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn level_update(emitter: &SignalEmitter<'_>, level: f64) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn error_occurred(emitter: &SignalEmitter<'_>, message: &str) -> zbus::Result<()>;

    #[zbus(signal)]
    async fn paste_requested(emitter: &SignalEmitter<'_>) -> zbus::Result<()>;
}

async fn transcribe_and_copy(audio: Vec<u8>, emitter: &SignalEmitter<'static>) {
    let api_key = match tokio::task::spawn_blocking(|| secret::read(ORATE_CLOUD_KEY))
        .await
        .ok()
        .flatten()
    {
        Some(k) if !k.is_empty() => k,
        _ => {
            eprintln!("[orate] missing API key");
            let _ = Service::error_occurred(
                emitter,
                "Missing API key. Open Orate to add your Orate Cloud key.",
            )
            .await;
            return;
        }
    };

    match transcription::transcribe(&audio, &api_key, None, &[]).await {
        Ok(result) => {
            eprintln!(
                "[orate] transcribed in {}ms ({} words used, {} remaining)",
                result.latency_ms, result.words_used, result.words_remaining
            );
            if result.transcript.is_empty() {
                eprintln!("[orate] empty transcript (silence) \u{2014} clipboard untouched");
                return;
            }
            if let Err(e) = clipboard::set(&result.transcript) {
                eprintln!("[orate] clipboard write failed: {e}");
                let _ =
                    Service::error_occurred(emitter, &format!("clipboard: {e}")).await;
            } else {
                eprintln!("[orate] copied to clipboard ({} chars)", result.transcript.len());
                let _ = Service::paste_requested(emitter).await;
            }
        }
        Err(e) => {
            eprintln!("[orate] transcription failed: {e}");
            let _ = Service::error_occurred(emitter, &format!("transcribe: {e}")).await;
        }
    }
}

pub fn start_in_background() {
    std::thread::spawn(|| {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            if let Err(e) = run().await {
                eprintln!("[orate] dbus service failed: {e}");
            }
        });
    });
}

pub fn run_blocking() -> zbus::Result<()> {
    let rt = tokio::runtime::Builder::new_multi_thread()
        .enable_all()
        .build()
        .expect("tokio runtime");
    rt.block_on(run())
}

pub async fn run() -> zbus::Result<()> {
    let service = Service {
        state: Arc::new(Mutex::new(State::Idle)),
        recorder: Arc::new(Mutex::new(None)),
    };
    let _conn = connection::Builder::session()?
        .name(BUS_NAME)?
        .serve_at(OBJECT_PATH, service)?
        .build()
        .await?;
    eprintln!(
        "[orate] D-Bus service ready: name={BUS_NAME} path={OBJECT_PATH} interface=com.orate.App1"
    );
    pending::<()>().await;
    Ok(())
}
