# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & test

Windows-only binary. The crate compiles on macOS/Linux for syntax checking because all Windows-specific deps are gated behind `cfg(windows)`, but the actual exe must be produced on Windows.

```cmd
cargo build --release          :: produces target\release\mhr-cfw-launcher.exe (~2 MB stripped)
cargo check                    :: works on any host; useful for cross-platform modules
cargo test                     :: only python.rs has unit tests (parse_version, meets_minimum)
cargo test python::tests::parses_python_v_output  :: run a single test
```

`build.rs` calls `embed-resource` on `app.rc`, which embeds `app.manifest` (UAC `requireAdministrator`, PerMonitorV2 DPI, UTF-8 codepage). Modifying the manifest requires a clean rebuild for the resource to re-embed.

The release profile is tuned for size (`opt-level = "z"`, LTO, `panic = "abort"`, `strip = true`). Optional further compression via `upx --best --lzma`.

## Runtime architecture

Three execution contexts cooperate; understanding the boundary between them is essential before changing flow:

1. **GUI thread** (`src/gui.rs`) — `native-windows-gui` event loop. Builds widgets, handles button clicks, and runs an `AnimationTimer` (~120ms) that calls `drain_messages` to pull `WorkerMsg` updates and refresh the log box from `LogRing`. The GUI thread never blocks on I/O.
2. **Worker thread** (`spawn_worker` in `gui.rs`) — receives `GuiCmd` from the GUI over an `mpsc::channel`, executes the long-running setup pipeline (`handle_connect`), and reports progress back via `WorkerMsg`. All filesystem, HTTP, and process work happens here.
3. **Child Python process** (`src/runner.rs`) — `python main.py` spawned with piped stdout/stderr. Two reader threads tag each line `[out]`/`[err]` and push to `LogRing` (a 1000-line `VecDeque` behind `Arc<Mutex>`).

`AppState` (Arc-shared) holds the cross-thread mutables: detected `PythonInfo`, current `Runner`, the `LogRing`, and a `proxy_set` flag used during shutdown to know whether to revert the proxy.

## Setup pipeline (`handle_connect` in gui.rs)

The order matters and is load-bearing for correctness:

1. **Python** — `python::detect()` looks at `py.exe`/`python.exe`/`python3.exe` on `PATH`, parses `-V` output, requires ≥3.10. If absent, downloads the 3.11.9 installer matching pointer width and runs it silently with `InstallAllUsers=1 PrependPath=1`. After install, `detect()` is retried, then `detect_in_known_paths()` falls back to `C:\Program Files\Python31{0,1}\python.exe` because the current process won't see the updated PATH.
2. **Project** — guarded by `config::is_installed()` (marker file at `C:\mhr-cfw\.launcher-installed` AND `main.py` present). On first run: download `denuitt1/mhr-cfw` `main.zip`, `extract_flatten` strips the GitHub-injected top-level dir (e.g. `mhr-cfw-main/`), `pip install --upgrade pip` then `-r requirements.txt`, then write the marker.
3. **Credentials** — merge GUI input with existing `config.json`, preserving every other key. If `config.json` is missing, seed from `config.example.json` so required keys aren't lost. Only `script_id` and `auth_key` are touched.
4. **Spawn child** — `Runner::spawn` runs `python main.py` with `current_dir = C:\mhr-cfw`, piped stdout/stderr, null stdin.
5. **Cert** — `python main.py --install-cert` invoked best-effort; failures only log a warning because the project may have already prompted during step 4.
6. **Proxy** — write `ProxyEnable=1`, `ProxyServer=127.0.0.1:8085`, and a long `ProxyOverride` bypass list to `HKCU\...\Internet Settings`, then broadcast `WM_SETTINGCHANGE` (twice — once null lParam, once `"Internet"`) so browsers re-read.
7. **Health check** — sleep 2s, then `runner.poll()`; if the child already exited, the whole connect fails.

On `Disconnect` or window close: kill the child, then `proxy::disable()` clears `ProxyEnable` (leaves `ProxyServer`/`ProxyOverride` so users can re-enable a custom proxy).

## Important hard-coded paths and constants

All in `src/paths.rs`:

- Project root: `C:\mhr-cfw` (not configurable; many modules assume this)
- Project zip: `https://github.com/denuitt1/mhr-cfw/archive/refs/heads/main.zip`
- Proxy: `127.0.0.1:8085`
- Install marker: `C:\mhr-cfw\.launcher-installed`
- Failure reports: `C:\mhr-cfw\logs\report-YYYYMMDD-HHMMSS.txt`

Python installer URLs and minimum version live in `src/python.rs`.

## Module map

```
main.rs       cfg(windows) entrypoint; on error, MessageBoxW then exit. Non-Windows main prints and exits.
gui.rs        Window construction, event loop, worker thread, and the connect/disconnect/report orchestration.
python.rs     Detect (PATH + known dirs) and silent-install Python; parse_version handles "Python 3.x.y\n" forms.
project.rs    Zip download + flatten extract + pip install.
config.rs     Read/write config.json preserving unknown keys; install marker management.
proxy.rs      HKCU registry writes + WM_SETTINGCHANGE broadcast (Windows-gated).
cert.rs       Invokes the project's own `--install-cert` flag.
runner.rs     Child process supervisor + LogRing (1000-line ring buffer used by GUI and report).
download.rs   Streaming ureq → file with throttled progress callbacks (~256 KiB).
report.rs     Builds a single text dump (env + LogRing snapshot) for AI-assisted diagnosis.
paths.rs      Constants and PathBuf helpers.
```

## Cross-platform gotchas when editing

- `gui.rs`, `cert.rs`, `proxy.rs` are `#[cfg(windows)]` modules and the `windows`/`winreg`/`native-windows-gui` crates are `cfg(windows)` deps. Anything imported from them won't compile on macOS/Linux. Keep cross-platform modules (`config`, `download`, `paths`, `project`, `python`, `report`, `runner`) free of those imports so `cargo check` stays green on any host.
- `proxy.rs` already uses `#[cfg(windows)]`/`#[cfg(not(windows))]` arms inside its functions — match that pattern if adding more Windows-only behavior to otherwise-portable modules.
