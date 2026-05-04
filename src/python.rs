use anyhow::{anyhow, Context, Result};
use std::env;
use std::path::{Path, PathBuf};
use std::process::Command;

use crate::util;

const MIN_MAJOR: u32 = 3;
const MIN_MINOR: u32 = 10;

const PYTHON_INSTALLER_URL_X64: &str =
    "https://www.python.org/ftp/python/3.11.9/python-3.11.9-amd64.exe";
const PYTHON_INSTALLER_URL_X86: &str =
    "https://www.python.org/ftp/python/3.11.9/python-3.11.9.exe";

#[derive(Debug, Clone)]
pub struct PythonInfo {
    pub exe: PathBuf,
    pub version: (u32, u32, u32),
}

impl PythonInfo {
    pub fn version_string(&self) -> String {
        format!("{}.{}.{}", self.version.0, self.version.1, self.version.2)
    }

    pub fn meets_minimum(&self) -> bool {
        let (maj, min, _) = self.version;
        maj > MIN_MAJOR || (maj == MIN_MAJOR && min >= MIN_MINOR)
    }
}

/// Find a Python ≥ 3.10 already on the system.
pub fn detect() -> Option<PythonInfo> {
    for candidate in candidate_executables() {
        if let Some(info) = probe(&candidate) {
            if info.meets_minimum() {
                return Some(info);
            }
        }
    }
    None
}

fn candidate_executables() -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = Vec::new();
    for name in ["py.exe", "python.exe", "python3.exe"] {
        if let Some(path) = which(name) {
            found.push(path);
        }
    }
    found
}

fn which(exe: &str) -> Option<PathBuf> {
    let path_var = env::var_os("PATH")?;
    for dir in env::split_paths(&path_var) {
        let candidate = dir.join(exe);
        if candidate.is_file() {
            return Some(candidate);
        }
    }
    None
}

fn probe(exe: &Path) -> Option<PythonInfo> {
    // `py -3` style: launcher exe still accepts -V
    let output = util::no_console(Command::new(exe).arg("-V")).output().ok()?;
    let combined = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
    let version = parse_version(&combined)?;
    Some(PythonInfo {
        exe: exe.to_path_buf(),
        version,
    })
}

fn parse_version(s: &str) -> Option<(u32, u32, u32)> {
    let s = s.trim();
    let after_python = s.strip_prefix("Python ").unwrap_or(s);
    let mut parts = after_python.split('.');
    let maj: u32 = parts.next()?.trim().parse().ok()?;
    let min: u32 = parts.next()?.trim().parse().ok()?;
    let patch_raw = parts.next().unwrap_or("0");
    let patch: u32 = patch_raw
        .chars()
        .take_while(|c| c.is_ascii_digit())
        .collect::<String>()
        .parse()
        .unwrap_or(0);
    Some((maj, min, patch))
}

/// Detect installer URL for the running OS architecture.
fn installer_url() -> &'static str {
    if cfg!(target_pointer_width = "64") {
        PYTHON_INSTALLER_URL_X64
    } else {
        PYTHON_INSTALLER_URL_X86
    }
}

/// Download the Python installer to a temp file and run it silently.
/// Adds Python to PATH and installs for all users.
pub fn install<F>(mut progress: F) -> Result<PythonInfo>
where
    F: FnMut(&str),
{
    let url = installer_url();
    progress(&format!("Downloading Python installer from {url}"));

    let tmp_dir = env::temp_dir();
    let installer_path = tmp_dir.join("python-installer.exe");
    crate::download::to_file(url, &installer_path, |bytes, total| {
        if let Some(total) = total {
            let pct = (bytes as f64 / total as f64) * 100.0;
            progress(&format!("Downloading Python: {:.0}%", pct));
        } else {
            progress(&format!("Downloading Python: {} bytes", bytes));
        }
    })
    .context("downloading Python installer")?;

    progress("Running Python installer (this may take a minute)...");
    let status = util::no_console(
        Command::new(&installer_path).args([
            "/quiet",
            "InstallAllUsers=1",
            "PrependPath=1",
            "Include_test=0",
            "Include_launcher=1",
            "SimpleInstall=1",
        ]),
    )
    .status()
    .context("running python installer")?;

    if !status.success() {
        return Err(anyhow!(
            "Python installer exited with status {:?}",
            status.code()
        ));
    }

    // Refresh PATH from registry won't propagate to this process; spawn detection
    // should still work because the installer writes registry keys we can read.
    // As a fallback, look in well-known install dirs.
    if let Some(info) = detect() {
        return Ok(info);
    }
    if let Some(info) = detect_in_known_paths() {
        return Ok(info);
    }
    Err(anyhow!(
        "Python installer finished but no usable interpreter was found"
    ))
}

fn detect_in_known_paths() -> Option<PythonInfo> {
    let candidates = [
        r"C:\Program Files\Python311\python.exe",
        r"C:\Program Files\Python310\python.exe",
        r"C:\Program Files (x86)\Python311\python.exe",
        r"C:\Program Files (x86)\Python310\python.exe",
    ];
    for c in candidates {
        let p = Path::new(c);
        if p.is_file() {
            if let Some(info) = probe(p) {
                if info.meets_minimum() {
                    return Some(info);
                }
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_python_v_output() {
        assert_eq!(parse_version("Python 3.11.9"), Some((3, 11, 9)));
        assert_eq!(parse_version("Python 3.10.4\n"), Some((3, 10, 4)));
        assert_eq!(parse_version("Python 3.12"), Some((3, 12, 0)));
    }

    #[test]
    fn minimum_check() {
        let ok = PythonInfo {
            exe: PathBuf::new(),
            version: (3, 10, 0),
        };
        assert!(ok.meets_minimum());
        let too_old = PythonInfo {
            exe: PathBuf::new(),
            version: (3, 9, 18),
        };
        assert!(!too_old.meets_minimum());
    }
}
