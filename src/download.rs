use anyhow::{anyhow, Context, Result};
use std::fs::File;
use std::io::{Read, Write};
use std::path::Path;

/// Stream `url` to `dest`, calling `progress(bytes_so_far, total_bytes)` periodically.
/// `total_bytes` is `None` if Content-Length is missing.
pub fn to_file<F>(url: &str, dest: &Path, mut progress: F) -> Result<()>
where
    F: FnMut(u64, Option<u64>),
{
    let resp = ureq::get(url)
        .call()
        .with_context(|| format!("GET {url}"))?;

    if resp.status() < 200 || resp.status() >= 300 {
        return Err(anyhow!("HTTP {} from {}", resp.status(), url));
    }

    let total: Option<u64> = resp
        .header("Content-Length")
        .and_then(|v| v.parse().ok());

    if let Some(parent) = dest.parent() {
        std::fs::create_dir_all(parent).ok();
    }

    let mut file = File::create(dest)
        .with_context(|| format!("creating {}", dest.display()))?;
    let mut reader = resp.into_reader();
    let mut buf = [0u8; 64 * 1024];
    let mut written: u64 = 0;
    let mut last_report: u64 = 0;

    loop {
        let n = reader
            .read(&mut buf)
            .with_context(|| format!("reading from {url}"))?;
        if n == 0 {
            break;
        }
        file.write_all(&buf[..n])
            .with_context(|| format!("writing to {}", dest.display()))?;
        written += n as u64;
        // Throttle progress callbacks to ~256 KiB increments to avoid GUI thrash.
        if written - last_report > 256 * 1024 {
            progress(written, total);
            last_report = written;
        }
    }
    progress(written, total);
    Ok(())
}
