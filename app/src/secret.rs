use std::collections::HashMap;

use dbus_secret_service::{EncryptionType, SecretService};
use log::{debug, info, warn};

const ATTR_APP: &str = "application";
const APP_VALUE: &str = "com.orate.app";
const ATTR_KEY: &str = "key";

pub const ORATE_CLOUD_KEY: &str = "orateCloudAPIKey";
pub const GEMINI_KEY: &str = "geminiAPIKey";
pub const VERTEX_KEY: &str = "vertexAPIKey";

pub fn save(key: &str, value: &str) -> Result<(), String> {
    info!("keyring: saving secret for key={key} ({} bytes)", value.len());
    let ss = SecretService::connect(EncryptionType::Dh).map_err(|e| e.to_string())?;
    let collection = ss.get_default_collection().map_err(|e| e.to_string())?;
    if collection.is_locked().map_err(|e| e.to_string())? {
        debug!("keyring: collection locked, unlocking");
        collection.unlock().map_err(|e| e.to_string())?;
    }
    let mut attrs = HashMap::new();
    attrs.insert(ATTR_APP, APP_VALUE);
    attrs.insert(ATTR_KEY, key);
    collection
        .create_item(
            &format!("Orate ({key})"),
            attrs,
            value.as_bytes(),
            true,
            "text/plain",
        )
        .map_err(|e| e.to_string())?;
    info!("keyring: saved key={key}");
    Ok(())
}

pub fn delete(key: &str) -> Result<(), String> {
    info!("keyring: deleting secret for key={key}");
    let ss = SecretService::connect(EncryptionType::Dh).map_err(|e| e.to_string())?;
    let collection = ss.get_default_collection().map_err(|e| e.to_string())?;
    if collection.is_locked().map_err(|e| e.to_string())? {
        collection.unlock().map_err(|e| e.to_string())?;
    }
    let mut attrs = HashMap::new();
    attrs.insert(ATTR_APP, APP_VALUE);
    attrs.insert(ATTR_KEY, key);
    for item in collection.search_items(attrs).map_err(|e| e.to_string())? {
        item.delete().map_err(|e| e.to_string())?;
    }
    Ok(())
}

pub fn read(key: &str) -> Option<String> {
    let ss = match SecretService::connect(EncryptionType::Dh) {
        Ok(s) => s,
        Err(e) => {
            warn!("keyring: connect failed: {e}");
            return None;
        }
    };
    let collection = match ss.get_default_collection() {
        Ok(c) => c,
        Err(e) => {
            warn!("keyring: get_default_collection failed: {e}");
            return None;
        }
    };
    if matches!(collection.is_locked(), Ok(true)) {
        if let Err(e) = collection.unlock() {
            warn!("keyring: unlock failed: {e}");
            return None;
        }
    }
    let mut attrs = HashMap::new();
    attrs.insert(ATTR_APP, APP_VALUE);
    attrs.insert(ATTR_KEY, key);
    let items = match collection.search_items(attrs) {
        Ok(i) => i,
        Err(e) => {
            warn!("keyring: search_items failed: {e}");
            return None;
        }
    };
    let item = items.into_iter().next()?;
    let bytes = item.get_secret().ok()?;
    debug!("keyring: read key={key} ({} bytes)", bytes.len());
    String::from_utf8(bytes).ok()
}
