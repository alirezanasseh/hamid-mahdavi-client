use anyhow::{anyhow, Context, Result};
use serde_json::{json, Value};
use std::fs;

use crate::paths;

/// Read previously-saved credentials from `%APPDATA%\mhr-cfw-launcher\credentials.json`.
/// Returns None if the file is missing, malformed, or APPDATA is unset.
pub fn load() -> Option<(String, String)> {
    let path = paths::credentials_path()?;
    if !path.exists() {
        return None;
    }
    let raw = fs::read_to_string(&path).ok()?;
    let v: Value = serde_json::from_str(&raw).ok()?;
    let script_id = v.get("script_id")?.as_str()?.to_string();
    let auth_key = v.get("auth_key")?.as_str()?.to_string();
    if script_id.is_empty() && auth_key.is_empty() {
        return None;
    }
    Some((script_id, auth_key))
}

/// Persist credentials. Creates the parent dir if needed. The file is rewritten
/// atomically enough for a credentials store (small, infrequent writes).
pub fn save(script_id: &str, auth_key: &str) -> Result<()> {
    let path = paths::credentials_path()
        .ok_or_else(|| anyhow!("APPDATA is not set; cannot persist credentials"))?;
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)
            .with_context(|| format!("creating {}", parent.display()))?;
    }
    let body = serde_json::to_string_pretty(&json!({
        "script_id": script_id,
        "auth_key": auth_key,
    }))?;
    fs::write(&path, body)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
