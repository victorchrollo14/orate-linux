use std::future::pending;
use std::path::PathBuf;
use std::sync::Arc;

use log::{debug, error, info, warn};
use tokio::sync::{mpsc, Mutex};
use zbus::{connection, interface, object_server::SignalEmitter};

use crate::clipboard;
use crate::config;
use crate::history;
use crate::logger;
use crate::recorder::Recorder;
use crate::secret;
use crate::sound::{self, Cue};
use crate::transcription;

const BUS_NAME: &str = "com.orate.App.Service";
const OBJECT_PATH: &str = "/com/orate/App";

fn recording_path() -> PathBuf {
    std::env::temp_dir().join("orate_recording.flac")
}

// Persistent copy of the last recording the user can play back when debugging
// "the transcription doesn't match what I said".
fn last_recording_path() -> PathBuf {
    logger::state_dir().join("last_recording.flac")
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
        info!("D-Bus: StartRecording received");
        let mut state = self.state.lock().await;
        if *state != State::Idle {
            warn!("StartRecording ignored (state={})", state.as_str());
            return Ok(());
        }

        let (level_tx, mut level_rx) = mpsc::unbounded_channel::<f64>();
        let path = recording_path();

        match Recorder::start(&path, level_tx) {
            Ok(rec) => {
                self.recorder.lock().await.replace(rec);
                *state = State::Listening;
                drop(state);
                info!("state -> listening");
                sound::play(Cue::Start);
                let _ = Self::state_changed(&emitter, State::Listening.as_str()).await;

                let level_emitter = emitter.to_owned();
                tokio::spawn(async move {
                    let mut emitted: u64 = 0;
                    while let Some(level) = level_rx.recv().await {
                        match Service::level_update(&level_emitter, level).await {
                            Ok(()) => {
                                emitted += 1;
                                if emitted == 1 {
                                    debug!("first LevelUpdate signal sent (level={level:.3})");
                                }
                            }
                            Err(e) => warn!("LevelUpdate emit failed: {e}"),
                        }
                    }
                    debug!("level emit task ended after {emitted} signals");
                });
                Ok(())
            }
            Err(e) => {
                drop(state);
                error!("recorder start failed: {e}");
                sound::play(Cue::Error);
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
        info!("D-Bus: StopRecording received");
        let mut state = self.state.lock().await;
        if *state != State::Listening {
            warn!("StopRecording ignored (state={})", state.as_str());
            return Ok(());
        }
        *state = State::Transcribing;
        drop(state);
        info!("state -> transcribing");
        sound::play(Cue::Stop);
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
                    // Keep a copy so the user can replay it: ~/.local/state/orate/last_recording.flac
                    let copy_path = last_recording_path();
                    match std::fs::write(&copy_path, &bytes) {
                        Ok(()) => debug!(
                            "saved debug copy of recording to {} ({} bytes)",
                            copy_path.display(),
                            bytes.len()
                        ),
                        Err(e) => warn!(
                            "could not save debug copy to {}: {e}",
                            copy_path.display()
                        ),
                    }
                    transcribe_and_save(bytes, &emitter_owned).await;
                }
                Err(e) => {
                    error!("recorder stop failed: {e}");
                    sound::play(Cue::Error);
                    let _ =
                        Service::error_occurred(&emitter_owned, &format!("recorder: {e}")).await;
                }
            }
            *state_arc.lock().await = State::Idle;
            info!("state -> idle");
            let _ = Service::state_changed(&emitter_owned, State::Idle.as_str()).await;
        });
        Ok(())
    }

    async fn cancel(
        &self,
        #[zbus(signal_emitter)] emitter: SignalEmitter<'_>,
    ) -> zbus::fdo::Result<()> {
        info!("D-Bus: Cancel received");
        let mut state = self.state.lock().await;
        if *state == State::Idle {
            return Ok(());
        }
        if let Some(r) = self.recorder.lock().await.take() {
            r.cancel();
        }
        *state = State::Idle;
        info!("state -> idle (cancelled)");
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

async fn transcribe_and_save(audio: Vec<u8>, emitter: &SignalEmitter<'static>) {
    // Read once per transcription so edits in the settings window take effect
    // immediately, without restarting the service.
    let prefs = config::load_prefs();
    let provider = prefs.provider;
    debug!(
        "prefs: provider={}, {} chars of custom instructions, {} vocabulary words, save_audio={}",
        provider.id(),
        prefs.custom_instructions.len(),
        prefs.vocabulary.len(),
        prefs.save_audio
    );

    let api_key = match tokio::task::spawn_blocking(move || secret::read(provider.keyring_key()))
        .await
        .ok()
        .flatten()
    {
        Some(k) if !k.is_empty() => {
            debug!("loaded {} API key from keyring ({} chars)", provider.id(), k.len());
            k
        }
        _ => {
            error!("missing {} API key (keyring entry empty or unreadable)", provider.id());
            sound::play(Cue::Error);
            let _ = Service::error_occurred(
                emitter,
                &format!(
                    "Missing API key. Open Orate to add your {} key.",
                    provider.display_name()
                ),
            )
            .await;
            return;
        }
    };

    info!(
        "posting {} bytes of FLAC audio to {}",
        audio.len(),
        provider.display_name()
    );
    match transcription::transcribe(&audio, &api_key, &prefs).await {
        Ok(result) => {
            info!(
                "transcribed in {}ms ({:?} words used, {:?} remaining): {:?}",
                result.latency_ms,
                result.words_used,
                result.words_remaining,
                truncate(&result.transcript, 200)
            );
            if result.transcript.is_empty() {
                warn!("empty transcript (silence or non-speech) \u{2014} clipboard untouched");
                return;
            }
            if let Err(e) = clipboard::set(&result.transcript) {
                error!("clipboard write failed: {e}");
                let _ =
                    Service::error_occurred(emitter, &format!("clipboard: {e}")).await;
            } else {
                info!("copied to clipboard ({} chars)", result.transcript.len());
                let _ = Service::paste_requested(emitter).await;
                debug!("PasteRequested signal sent");
            }

            let transcript = result.transcript.clone();
            let latency = result.latency_ms as u64;
            let words_remaining = result.words_remaining;
            let keep_audio = prefs.save_audio;
            let _ = tokio::task::spawn_blocking(move || {
                let audio = keep_audio.then_some(audio.as_slice());
                match history::save(&transcript, latency, audio, words_remaining) {
                    Ok(entry) => debug!("history entry saved: {}", entry.id),
                    Err(e) => warn!("history save failed: {e}"),
                }
            })
            .await;
        }
        Err(e) => {
            error!("transcription failed: {e}");
            sound::play(Cue::Error);
            let _ = Service::error_occurred(emitter, &format!("transcribe: {e}")).await;
        }
    }
}

fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        let mut out: String = s.chars().take(n).collect();
        out.push_str("\u{2026}");
        out
    }
}

pub fn start_in_background() {
    debug!("spawning D-Bus service on background thread");
    std::thread::spawn(|| {
        let rt = tokio::runtime::Builder::new_multi_thread()
            .enable_all()
            .build()
            .expect("tokio runtime");
        rt.block_on(async {
            if let Err(e) = run().await {
                error!("dbus service failed: {e}");
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
    info!(
        "D-Bus service ready: name={BUS_NAME} path={OBJECT_PATH} interface=com.orate.App1"
    );
    pending::<()>().await;
    Ok(())
}
