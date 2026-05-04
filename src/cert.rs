use anyhow::{anyhow, Context, Result};
use std::process::Command;

use crate::paths;
use crate::python::PythonInfo;

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
    let status = Command::new(&py.exe)
        .arg(&main_py)
        .arg("--install-cert")
        .current_dir(paths::project_dir())
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
