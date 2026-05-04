use anyhow::{Context, Result};
use std::env;
use std::fs;
use std::path::PathBuf;

use crate::paths;
use crate::python::PythonInfo;
use crate::runner::LogRing;

/// Build a single text report describing the failure context, formatted to
/// be pastable into an AI assistant. Returns the path to the saved report.
pub fn write(
    summary: &str,
    py: Option<&PythonInfo>,
    logs: &LogRing,
) -> Result<PathBuf> {
    let dir = paths::log_dir();
    fs::create_dir_all(&dir)
        .with_context(|| format!("creating {}", dir.display()))?;
    let stamp = chrono::Local::now().format("%Y%m%d-%H%M%S");
    let path = dir.join(format!("report-{stamp}.txt"));

    let mut out = String::new();
    out.push_str("=== hamid-mahdavi-client failure report ===\n");
    out.push_str(&format!(
        "When: {}\n",
        chrono::Local::now().to_rfc3339()
    ));
    out.push_str(&format!("Summary: {summary}\n\n"));

    out.push_str("--- environment ---\n");
    out.push_str(&format!("OS: {}\n", env::consts::OS));
    out.push_str(&format!("Arch: {}\n", env::consts::ARCH));
    out.push_str(&format!("Pointer width: {}-bit\n", usize::BITS));
    if let Ok(v) = env::var("OS") {
        out.push_str(&format!("OS env: {v}\n"));
    }
    if let Ok(v) = env::var("PROCESSOR_ARCHITECTURE") {
        out.push_str(&format!("PROCESSOR_ARCHITECTURE: {v}\n"));
    }
    if let Some(py) = py {
        out.push_str(&format!(
            "Python: {} ({})\n",
            py.version_string(),
            py.exe.display()
        ));
    } else {
        out.push_str("Python: not detected\n");
    }
    out.push_str(&format!(
        "Project dir present: {}\n",
        paths::project_dir().exists()
    ));
    out.push_str(&format!(
        "main.py present: {}\n",
        paths::main_script_path().exists()
    ));
    out.push_str(&format!(
        "config.json present: {}\n",
        paths::config_path().exists()
    ));
    out.push_str(&format!(
        "Install marker: {}\n",
        paths::install_marker_path().exists()
    ));
    out.push('\n');

    out.push_str("--- recent child output (last 1000 lines) ---\n");
    for line in logs.snapshot() {
        out.push_str(&line);
        out.push('\n');
    }

    fs::write(&path, out)
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(path)
}
