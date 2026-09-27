// Preferences live in GSettings so the GUI process and the D-Bus service
// process see the same values, and so changes propagate without a restart.
//
// Every accessor degrades gracefully when the schema is not installed (running
// `cargo run` straight out of the source tree, for instance): reads fall back
// to the documented defaults and writes are dropped with a warning, instead of
// GLib aborting the process the way `Settings::new` would.

use std::path::PathBuf;

use gtk4::prelude::*;
use gtk4::{gio, glib};
use log::warn;

pub const APP_SCHEMA: &str = "org.orate.app";
pub const EXT_SCHEMA: &str = "org.gnome.shell.extensions.orate";
pub const EXT_UUID: &str = "orate@orate.app";
pub const SHORTCUT_KEY: &str = "toggle-recording";

pub const DEFAULT_SHORTCUT: &str = "<Control>F8";

/// Settings for the app's own schema, or `None` when it is not installed.
pub fn app_settings() -> Option<gio::Settings> {
    let source = gio::SettingsSchemaSource::default()?;
    if source.lookup(APP_SCHEMA, true).is_none() {
        warn!("schema {APP_SCHEMA} not installed \u{2014} using built-in defaults (run `make install-app`)");
        return None;
    }
    Some(gio::Settings::new(APP_SCHEMA))
}

/// Settings for the GNOME Shell extension, which owns the push-to-talk
/// shortcut. Its schema ships inside the extension directory rather than the
/// system schema path, so we look there first.
pub fn ext_settings() -> Option<gio::Settings> {
    let default_source = gio::SettingsSchemaSource::default();

    for dir in ext_schema_dirs() {
        if !dir.exists() {
            continue;
        }
        let source =
            match gio::SettingsSchemaSource::from_directory(&dir, default_source.as_ref(), true) {
                Ok(s) => s,
                Err(e) => {
                    warn!("reading schemas from {}: {e}", dir.display());
                    continue;
                }
            };
        if let Some(schema) = source.lookup(EXT_SCHEMA, false) {
            return Some(gio::Settings::new_full(
                &schema,
                None::<&gio::SettingsBackend>,
                None,
            ));
        }
    }

    // System-wide extension installs put the schema on the normal search path.
    let source = default_source?;
    source.lookup(EXT_SCHEMA, true)?;
    Some(gio::Settings::new(EXT_SCHEMA))
}

fn ext_schema_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    let data_home = std::env::var_os("XDG_DATA_HOME")
        .map(PathBuf::from)
        .unwrap_or_else(|| {
            let home = std::env::var_os("HOME")
                .map(PathBuf::from)
                .unwrap_or_default();
            home.join(".local/share")
        });
    dirs.push(data_home.join(format!("gnome-shell/extensions/{EXT_UUID}/schemas")));
    dirs.push(PathBuf::from(format!(
        "/usr/share/gnome-shell/extensions/{EXT_UUID}/schemas"
    )));
    dirs
}

// MARK: - Transcription provider

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Provider {
    OrateCloud,
    GoogleAI,
    VertexAI,
}

impl Provider {
    pub const ALL: [Provider; 3] = [Provider::OrateCloud, Provider::GoogleAI, Provider::VertexAI];

    /// Value stored in GSettings; matches the macOS app's `aiProvider` raw values.
    pub fn id(self) -> &'static str {
        match self {
            Provider::OrateCloud => "orateCloud",
            Provider::GoogleAI => "googleAI",
            Provider::VertexAI => "vertexAI",
        }
    }

    pub fn from_id(id: &str) -> Provider {
        Provider::ALL
            .into_iter()
            .find(|p| p.id() == id)
            .unwrap_or(Provider::OrateCloud)
    }

    pub fn display_name(self) -> &'static str {
        match self {
            Provider::OrateCloud => "Orate Cloud",
            Provider::GoogleAI => "Google AI Studio",
            Provider::VertexAI => "Vertex AI",
        }
    }

    /// Each provider keeps its own keyring entry so switching back and forth
    /// doesn't lose a key.
    pub fn keyring_key(self) -> &'static str {
        match self {
            Provider::OrateCloud => crate::secret::ORATE_CLOUD_KEY,
            Provider::GoogleAI => crate::secret::GEMINI_KEY,
            Provider::VertexAI => crate::secret::VERTEX_KEY,
        }
    }
}

pub const DEFAULT_VERTEX_REGION: &str = "us-central1";

pub const VERTEX_REGIONS: &[&str] = &[
    "global",
    "us-central1",
    "us-east4",
    "us-west1",
    "europe-west1",
    "europe-west4",
    "asia-northeast1",
    "asia-southeast1",
];

pub fn provider() -> Provider {
    app_settings()
        .map(|s| Provider::from_id(&s.string("ai-provider")))
        .unwrap_or(Provider::OrateCloud)
}

pub fn set_provider(value: Provider) {
    set(|s| s.set_string("ai-provider", value.id()));
}

pub fn vertex_project_id() -> String {
    app_settings()
        .map(|s| s.string("vertex-project-id").to_string())
        .unwrap_or_default()
}

pub fn set_vertex_project_id(value: &str) {
    set(|s| s.set_string("vertex-project-id", value.trim()));
}

pub fn vertex_region() -> String {
    app_settings()
        .map(|s| s.string("vertex-region").to_string())
        .unwrap_or_else(|| DEFAULT_VERTEX_REGION.to_string())
}

pub fn set_vertex_region(value: &str) {
    set(|s| s.set_string("vertex-region", value));
}

// MARK: - Transcription preferences

/// Everything the transcription request needs, read in one shot so the service
/// only touches GSettings once per recording.
pub struct Prefs {
    pub provider: Provider,
    pub vertex_project_id: String,
    pub vertex_region: String,
    pub custom_instructions: String,
    pub vocabulary: Vec<String>,
    pub save_audio: bool,
}

impl Default for Prefs {
    fn default() -> Self {
        Prefs {
            provider: Provider::OrateCloud,
            vertex_project_id: String::new(),
            vertex_region: DEFAULT_VERTEX_REGION.to_string(),
            custom_instructions: String::new(),
            vocabulary: Vec::new(),
            save_audio: true,
        }
    }
}

pub fn load_prefs() -> Prefs {
    let Some(settings) = app_settings() else {
        return Prefs::default();
    };
    Prefs {
        provider: Provider::from_id(&settings.string("ai-provider")),
        vertex_project_id: settings.string("vertex-project-id").trim().to_string(),
        vertex_region: settings.string("vertex-region").to_string(),
        custom_instructions: settings.string("custom-instructions").to_string(),
        vocabulary: settings
            .strv("vocabulary-words")
            .iter()
            .map(|s| s.to_string())
            .collect(),
        save_audio: settings.boolean("save-audio"),
    }
}

pub fn custom_instructions() -> String {
    app_settings()
        .map(|s| s.string("custom-instructions").to_string())
        .unwrap_or_default()
}

pub fn set_custom_instructions(value: &str) {
    set(|s| s.set_string("custom-instructions", value));
}

pub fn vocabulary() -> Vec<String> {
    app_settings()
        .map(|s| s.strv("vocabulary-words").iter().map(|v| v.to_string()).collect())
        .unwrap_or_default()
}

pub fn set_vocabulary(words: &[String]) {
    let refs: Vec<&str> = words.iter().map(|w| w.as_str()).collect();
    set(|s| s.set_strv("vocabulary-words", refs.clone()));
}

pub fn sound_feedback() -> bool {
    app_settings()
        .map(|s| s.boolean("sound-feedback"))
        .unwrap_or(true)
}

pub fn set_sound_feedback(value: bool) {
    set(|s| s.set_boolean("sound-feedback", value));
}

pub fn save_audio() -> bool {
    app_settings()
        .map(|s| s.boolean("save-audio"))
        .unwrap_or(true)
}

pub fn set_save_audio(value: bool) {
    set(|s| s.set_boolean("save-audio", value));
}

fn set<F>(apply: F)
where
    F: Fn(&gio::Settings) -> Result<(), glib::BoolError>,
{
    let Some(settings) = app_settings() else {
        warn!("cannot persist preference: schema {APP_SCHEMA} not installed");
        return;
    };
    if let Err(e) = apply(&settings) {
        warn!("writing preference failed: {e}");
    }
}

// MARK: - Push-to-talk shortcut

/// The accelerator the shell extension listens for, e.g. `<Control>F8`.
pub fn shortcut() -> String {
    ext_settings()
        .and_then(|s| s.strv(SHORTCUT_KEY).first().map(|v| v.to_string()))
        .unwrap_or_else(|| DEFAULT_SHORTCUT.to_string())
}

/// Returns false when the extension's schema is missing, in which case the UI
/// shows the shortcut read-only instead of pretending it can be changed.
pub fn shortcut_is_editable() -> bool {
    ext_settings().is_some()
}

pub fn set_shortcut(accelerator: &str) -> Result<(), String> {
    let settings = ext_settings().ok_or_else(|| {
        format!("the Orate GNOME Shell extension is not installed ({EXT_SCHEMA} schema not found)")
    })?;
    settings
        .set_strv(SHORTCUT_KEY, [accelerator])
        .map_err(|e| e.to_string())?;
    gio::Settings::sync();
    Ok(())
}
