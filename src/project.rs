use anyhow::{anyhow, Context, Result};
use std::env;
use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::paths;
use crate::python::PythonInfo;

/// Download the project zip from GitHub and extract into `C:\mhr-cfw`.
/// The GitHub zip contains a single top-level dir like `mhr-cfw-main/` — we
/// flatten it so files land directly under `C:\mhr-cfw`.
pub fn download_and_extract<F>(mut progress: F) -> Result<()>
where
    F: FnMut(&str),
{
    let target = paths::project_dir();
    fs::create_dir_all(&target)
        .with_context(|| format!("creating {}", target.display()))?;

    let zip_path = env::temp_dir().join("mhr-cfw.zip");
    progress("Downloading project from GitHub...");
    crate::download::to_file(paths::PROJECT_REPO_ZIP, &zip_path, |bytes, total| {
        if let Some(total) = total {
            let pct = (bytes as f64 / total as f64) * 100.0;
            progress(&format!("Downloading project: {:.0}%", pct));
        } else {
            progress(&format!("Downloading project: {} bytes", bytes));
        }
    })
    .context("downloading project zip")?;

    progress("Extracting project files...");
    extract_flatten(&zip_path, &target)?;
    let _ = fs::remove_file(&zip_path);
    Ok(())
}

fn extract_flatten(zip_path: &Path, target: &Path) -> Result<()> {
    let file = fs::File::open(zip_path)
        .with_context(|| format!("opening {}", zip_path.display()))?;
    let mut archive = zip::ZipArchive::new(file)
        .with_context(|| format!("reading zip {}", zip_path.display()))?;

    for i in 0..archive.len() {
        let mut entry = archive.by_index(i)?;
        let raw_name = match entry.enclosed_name() {
            Some(n) => n.to_path_buf(),
            None => continue,
        };

        // Strip the GitHub-injected top-level directory (e.g. `mhr-cfw-main/`).
        let mut comps = raw_name.components();
        let _ = comps.next();
        let rel: PathBuf = comps.as_path().to_path_buf();
        if rel.as_os_str().is_empty() {
            continue;
        }

        let out = target.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out)
                .with_context(|| format!("mkdir {}", out.display()))?;
        } else {
            if let Some(parent) = out.parent() {
                fs::create_dir_all(parent)
                    .with_context(|| format!("mkdir {}", parent.display()))?;
            }
            let mut f = fs::File::create(&out)
                .with_context(|| format!("creating {}", out.display()))?;
            io::copy(&mut entry, &mut f)
                .with_context(|| format!("extracting {}", out.display()))?;
        }
    }

    if !target.join("main.py").is_file() {
        return Err(anyhow!(
            "extraction completed but main.py is missing under {}",
            target.display()
        ));
    }
    Ok(())
}

/// Run `python -m pip install -r requirements.txt` in the project dir.
pub fn pip_install<F>(py: &PythonInfo, mut progress: F) -> Result<()>
where
    F: FnMut(&str),
{
    let req = paths::requirements_path();
    if !req.exists() {
        progress("No requirements.txt found, skipping pip install");
        return Ok(());
    }
    progress("Installing project libraries (pip)...");
    let status = Command::new(&py.exe)
        .args(["-m", "pip", "install", "--upgrade", "pip"])
        .current_dir(paths::project_dir())
        .status()
        .context("running pip self-upgrade")?;
    if !status.success() {
        return Err(anyhow!("pip self-upgrade failed: {:?}", status.code()));
    }

    let status = Command::new(&py.exe)
        .args(["-m", "pip", "install", "-r"])
        .arg(&req)
        .current_dir(paths::project_dir())
        .status()
        .context("running pip install")?;
    if !status.success() {
        return Err(anyhow!("pip install failed: {:?}", status.code()));
    }
    Ok(())
}
