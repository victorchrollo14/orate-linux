use std::io::Write;
use std::process::{Command, Stdio};

pub fn set(text: &str) -> Result<(), String> {
    let mut child = Command::new("wl-copy")
        .stdin(Stdio::piped())
        .stdout(Stdio::null())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("wl-copy not found (install wl-clipboard): {e}"))?;

    child
        .stdin
        .take()
        .ok_or_else(|| "wl-copy stdin unavailable".to_string())?
        .write_all(text.as_bytes())
        .map_err(|e| e.to_string())?;

    let status = child.wait().map_err(|e| e.to_string())?;
    if !status.success() {
        return Err(format!("wl-copy exited with {status}"));
    }
    Ok(())
}
