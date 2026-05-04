use anyhow::{anyhow, Context, Result};
use serde_json::Value;
use std::fs;

use crate::paths;

/// Just the two values the launcher manages — every other key in
/// `config.example.json` is preserved verbatim during save.
#[derive(Debug, Clone, Default)]
pub struct Config {
    pub script_id: String,
    pub auth_key: String,
}

impl Config {
    /// Read the relevant fields from `config.json`. Returns Ok(None) if the
    /// file doesn't exist yet (first run, before extraction).
    pub fn load() -> Result<Option<Self>> {
        let path = paths::config_path();
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let v: Value = serde_json::from_str(&raw)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(Some(Self {
            script_id: extract_string(&v, "script_id"),
            auth_key: extract_string(&v, "auth_key"),
        }))
    }

    /// Patch `script_id` and `auth_key` into `config.json`, preserving all
    /// other keys. If `config.json` is missing, seed it from
    /// `config.example.json` so required keys aren't dropped.
    pub fn save(&self) -> Result<()> {
        let target = paths::config_path();
        let example = paths::config_example_path();

        let mut value: Value = if target.exists() {
            let raw = fs::read_to_string(&target)
                .with_context(|| format!("reading {}", target.display()))?;
            serde_json::from_str(&raw)
                .with_context(|| format!("parsing {}", target.display()))?
        } else if example.exists() {
            let raw = fs::read_to_string(&example)
                .with_context(|| format!("reading {}", example.display()))?;
            serde_json::from_str(&raw)
                .with_context(|| format!("parsing {}", example.display()))?
        } else {
            return Err(anyhow!(
                "neither {} nor {} exist — cannot build config",
                target.display(),
                example.display()
            ));
        };

        let obj = value.as_object_mut().ok_or_else(|| {
            anyhow!("config root is not a JSON object")
        })?;
        obj.insert("script_id".into(), Value::String(self.script_id.clone()));
        obj.insert("auth_key".into(), Value::String(self.auth_key.clone()));

        if let Some(parent) = target.parent() {
            fs::create_dir_all(parent).ok();
        }
        let pretty = serde_json::to_string_pretty(&value)?;
        fs::write(&target, pretty)
            .with_context(|| format!("writing {}", target.display()))?;
        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        !self.script_id.trim().is_empty() && !self.auth_key.trim().is_empty()
    }
}

fn extract_string(v: &Value, key: &str) -> String {
    v.get(key)
        .and_then(|x| x.as_str())
        .unwrap_or("")
        .to_string()
}

pub fn is_installed() -> bool {
    paths::install_marker_path().exists()
        && paths::main_script_path().exists()
}

pub fn mark_installed() -> Result<()> {
    let path = paths::install_marker_path();
    fs::write(&path, chrono::Local::now().to_rfc3339())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
