use anyhow::{anyhow, Context, Result};
use std::fs;
use std::process::Command;

use crate::paths;
use crate::python::PythonInfo;
use crate::util;

/// Generate the upstream's CA certificate (if missing) and install it into the
/// Windows LocalMachine Trusted Root store. We bypass `python main.py
/// --install-cert` because upstream installs into the per-user store via
/// `certutil -addstore -user Root`, which always pops the Windows "Security
/// Warning" confirmation dialog. The machine store path is silent under
/// elevation (which our manifest requires).
pub fn install(py: &PythonInfo) -> Result<()> {
    generate_ca(py)?;
    install_to_machine_root()?;
    Ok(())
}

/// Invoke upstream's `MITMCertManager` constructor in-process to write
/// `<project>/ca/ca.crt` + `ca.key` if either is missing. The constructor is
/// import-side-effect free; the install step lives in a separate function
/// upstream and is the one that prompts.
fn generate_ca(py: &PythonInfo) -> Result<()> {
    let src = paths::src_dir();
    if !src.is_dir() {
        return Err(anyhow!("cannot generate CA: {} not found", src.display()));
    }
    let bootstrap = format!(
        "import sys; sys.path.insert(0, r'{}'); from mitm import MITMCertManager; MITMCertManager()",
        src.display()
    );
    let status = util::no_console(
        Command::new(&py.exe)
            .args(["-c", &bootstrap])
            .current_dir(paths::project_dir()),
    )
    .status()
    .context("running CA bootstrap")?;
    if !status.success() {
        return Err(anyhow!(
            "CA bootstrap exited with status {:?}",
            status.code()
        ));
    }
    let ca = paths::ca_cert_path();
    if !ca.is_file() {
        return Err(anyhow!(
            "CA bootstrap completed but {} is missing",
            ca.display()
        ));
    }
    Ok(())
}

fn install_to_machine_root() -> Result<()> {
    let cert = paths::ca_cert_path();
    let status = util::no_console(
        Command::new("certutil")
            .args(["-addstore", "-f", "Root"])
            .arg(&cert),
    )
    .status()
    .context("running certutil -addstore Root")?;
    if !status.success() {
        return Err(anyhow!(
            "certutil -addstore Root exited with status {:?}",
            status.code()
        ));
    }
    Ok(())
}

/// Whether we've already imported the cert into the trust store on this
/// machine. Gates `install()` so we don't re-bootstrap and re-shell-out on
/// every connect.
pub fn is_installed() -> bool {
    paths::cert_marker_path().exists()
}

pub fn mark_installed() -> Result<()> {
    let path = paths::cert_marker_path();
    fs::write(&path, chrono::Local::now().to_rfc3339())
        .with_context(|| format!("writing {}", path.display()))?;
    Ok(())
}
