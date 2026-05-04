use anyhow::{anyhow, Context, Result};
use std::fs;
use std::process::Command;

use crate::paths;
use crate::python::PythonInfo;
use crate::util;

/// Run `python main.py --install-cert` to install the local CA cert into the
/// Windows trust store. The mhr-cfw project handles the actual installation;
/// we just invoke its CLI flag. Requires admin (we already elevated).
pub fn install(py: &PythonInfo) -> Result<()> {
    let main_py = paths::main_script_path();
    if !main_py.is_file() {
        return Err(anyhow!(
            "cannot install cert: {} not found",
            main_py.display()
        ));
    }
    let status = util::no_console(
        Command::new(&py.exe)
            .arg(&main_py)
            .arg("--install-cert")
            .current_dir(paths::project_dir()),
    )
    .status()
    .context("running --install-cert")?;
    if !status.success() {
        return Err(anyhow!(
            "--install-cert exited with status {:?}",
            status.code()
        ));
    }
    Ok(())
}

/// Whether we've already imported the cert into the trust store on this
/// machine. The mhr-cfw project itself is idempotent, but invoking its
/// `--install-cert` path on every connect is slow and pops a console window
/// from the project's own subprocess, so gate it behind a marker.
pub fn is_installed() -> bool {
    paths::cert_marker_path().exists()
}

pub fn mark_installed() -> Result<()> {
    let path = paths::cert_marker_path();
    fs::write(&path, chrono::Local::now().to_rfc3339())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
