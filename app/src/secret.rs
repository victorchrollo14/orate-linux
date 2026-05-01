use std::collections::HashMap;

use dbus_secret_service::{EncryptionType, SecretService};

const ATTR_APP: &str = "application";
const APP_VALUE: &str = "com.orate.app";
const ATTR_KEY: &str = "key";

pub fn save(key: &str, value: &str) -> Result<(), String> {
    let ss = SecretService::connect(EncryptionType::Dh).map_err(|e| e.to_string())?;
    let collection = ss.get_default_collection().map_err(|e| e.to_string())?;
    if collection.is_locked().map_err(|e| e.to_string())? {
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
    Ok(())
}

pub fn read(key: &str) -> Option<String> {
    let ss = SecretService::connect(EncryptionType::Dh).ok()?;
    let collection = ss.get_default_collection().ok()?;
    if collection.is_locked().ok()? {
        collection.unlock().ok()?;
    }
    let mut attrs = HashMap::new();
    attrs.insert(ATTR_APP, APP_VALUE);
    attrs.insert(ATTR_KEY, key);
    let items = collection.search_items(attrs).ok()?;
    let item = items.into_iter().next()?;
    let bytes = item.get_secret().ok()?;
    String::from_utf8(bytes).ok()
}
