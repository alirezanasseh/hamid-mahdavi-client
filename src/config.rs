use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::fs;

use crate::paths;

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct Config {
    pub script_id: String,
    pub auth_key: String,
}

impl Config {
    pub fn load() -> Result<Option<Self>> {
        let path = paths::config_path();
        if !path.exists() {
            return Ok(None);
        }
        let raw = fs::read_to_string(&path)
            .with_context(|| format!("reading {}", path.display()))?;
        let cfg: Config = serde_json::from_str(&raw)
            .with_context(|| format!("parsing {}", path.display()))?;
        Ok(Some(cfg))
    }

    pub fn save(&self) -> Result<()> {
        let path = paths::config_path();
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).ok();
        }
        let raw = serde_json::to_string_pretty(self)?;
        fs::write(&path, raw)
            .with_context(|| format!("writing {}", path.display()))?;
        Ok(())
    }

    pub fn is_complete(&self) -> bool {
        !self.script_id.trim().is_empty() && !self.auth_key.trim().is_empty()
    }
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
