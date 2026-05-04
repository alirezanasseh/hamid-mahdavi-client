# Hamid Mahdavi Client

Single-exe Windows launcher for the [mhr-cfw](https://github.com/denuitt1/mhr-cfw) VPN.

For non-technical users: double-click the exe, accept the UAC prompt, fill in
`script_id` and `auth_key`, click Connect.

## Download

Grab the latest exe from the
[releases page](https://github.com/alirezanasseh/hamid-mahdavi-client/releases/latest),
or direct-link to the always-current build:

[**hamid-mahdavi-client.exe**](https://github.com/alirezanasseh/hamid-mahdavi-client/releases/latest/download/hamid-mahdavi-client.exe)

No installer, no dependencies — just save the file and run it.

## What it does

**First run**

1. Detects Python ≥ 3.10 in PATH and registry. If missing, downloads the
   official installer matching the OS architecture and installs it silently
   (all-users, PATH prepended).
2. Downloads the project zip from GitHub and extracts it to `C:\hamid-mahdavi-client`.
3. Runs `pip install -r requirements.txt`.
4. Prompts for `script_id` and `auth_key` and writes `config.json`.
5. Spawns `python main.py`.
6. Runs `python main.py --install-cert` (best-effort — the project may have
   already prompted).
7. Sets the system proxy to `127.0.0.1:8085` via the registry and broadcasts
   `WM_SETTINGCHANGE`.
8. Shows status (connected / failed / stopped) in the GUI.

**Subsequent runs** skip steps 1–4 (install marker file at
`C:\hamid-mahdavi-client\.launcher-installed`) and go straight to spawning the VPN
process and enabling the proxy.

**On exit** the launcher kills the child and clears the system proxy.

**On failure** click *Save Report* — produces a single text file under
`C:\hamid-mahdavi-client\logs\` with environment info and the last 1000 lines of child
output, suitable for pasting into an AI chat for diagnosis.

## Building

Windows-only. Build on a Windows machine with [Rust](https://rustup.rs/)
installed:

```cmd
cargo build --release
```

Output: `target\release\hamid-mahdavi-client.exe` (~2 MB after `strip`).

The exe is fully self-contained — no DLLs, no installer needed. The embedded
manifest requests admin rights (UAC), which are needed for installing
Python all-users, importing the certificate, and (depending on the project)
broadcasting proxy changes.

### Optional: smaller exe

After building, run [UPX](https://upx.github.io/) to compress further:

```cmd
upx --best --lzma target\release\hamid-mahdavi-client.exe
```

Typical result: ~1 MB.

### Cross-compiling from macOS / Linux

Not configured out of the box. The dependencies (`native-windows-gui`,
`winreg`, `windows`) are gated to `cfg(windows)` so `cargo check` /
`cargo test` work on any host for the cross-platform modules, but the
final exe must be built on a Windows target.

## Project layout

```
src/
  main.rs       — entry point, MessageBox-on-fatal
  gui.rs        — native-windows-gui window + worker thread
  python.rs     — detect / install Python
  project.rs    — download + extract repo zip, run pip
  config.rs     — read/write config.json, install marker
  paths.rs      — C:\hamid-mahdavi-client constants
  download.rs   — streaming HTTP download with progress
  proxy.rs      — set / clear system proxy via registry
  cert.rs       — invoke `python main.py --install-cert`
  runner.rs     — spawn + supervise the python child, ring-buffer logs
  report.rs     — write a failure report dump
app.manifest    — UAC requireAdministrator + DPI awareness + UTF-8 codepage
app.rc          — embeds the manifest into the exe
build.rs        — calls embed-resource on app.rc
```
