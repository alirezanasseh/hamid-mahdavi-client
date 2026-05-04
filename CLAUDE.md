# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## Build & test

Windows-only binary. The crate compiles on macOS/Linux for syntax checking because all Windows-specific deps are gated behind `cfg(windows)`, but the actual exe must be produced on Windows.

```cmd
cargo build --release          :: produces target\release\hamid-mahdavi-client.exe (~2 MB stripped)
cargo check                    :: works on any host; useful for cross-platform modules
cargo test                     :: only python.rs has unit tests (parse_version, meets_minimum)
cargo test python::tests::parses_python_v_output  :: run a single test
```

`build.rs` calls `embed-resource` on `app.rc`, which embeds `app.manifest` (UAC `requireAdministrator`, PerMonitorV2 DPI, UTF-8 codepage). Modifying the manifest requires a clean rebuild for the resource to re-embed.

The release profile is tuned for size (`opt-level = "z"`, LTO, `panic = "abort"`, `strip = true`). Optional further compression via `upx --best --lzma`.

## Runtime architecture

Three execution contexts cooperate; understanding the boundary between them is essential before changing flow:

1. **GUI thread** (`src/gui.rs`) — `native-windows-gui` event loop. Builds widgets, handles button clicks, and runs an `AnimationTimer` (~120ms) that calls `drain_messages` to pull `WorkerMsg` updates and refresh the log box from `LogRing`. The GUI thread never blocks on I/O.
2. **Worker thread** (`spawn_worker` in `gui.rs`) — receives `GuiCmd` from the GUI over an `mpsc::channel`, executes long-running pipelines (`handle_install`, `handle_connect`, `handle_disconnect`, `handle_save_report`), and reports progress back via `WorkerMsg`. All filesystem, HTTP, and process work happens here.
3. **Child Python process** (`src/runner.rs`) — `python main.py --no-cert-check` spawned with piped stdout/stderr. Two reader threads tag each line `[out]`/`[err]` and push to `LogRing` (a 1000-line `VecDeque` behind `Arc<Mutex>`). `--no-cert-check` is passed because the launcher installs the CA itself (see cert step below).

`AppState` (Arc-shared) holds the cross-thread mutables: detected `PythonInfo`, current `Runner`, the `LogRing`, and a `proxy_set` flag used during shutdown to know whether to revert the proxy.

## GUI mode machine

The single primary action button morphs through a `Mode` state machine in `gui.rs`:

```
NeedsInstall → Installing → Ready → Connecting → Connected → Disconnecting → Ready
```

In-flight states (`Installing`/`Connecting`/`Disconnecting`) disable the button. `WorkerMsg::Failed` rolls back to `NeedsInstall` (if we were installing) or `Ready` (otherwise) and reveals the Save Report button. The initial mode is `NeedsInstall` if `config::is_installed()` is false, else `Ready`.

## Install pipeline (`handle_install` in gui.rs)

Triggered by the user clicking **Install** on a fresh machine. Does not start the VPN.

1. **Python** — `ensure_python` returns the cached `PythonInfo` or runs `python::detect()` / `python::install()` (see python details below).
2. **Project** — guarded by `config::is_installed()` (marker file at `C:\hamid-mahdavi-client\.launcher-installed` AND `main.py` present). On first run: download `denuitt1/mhr-cfw` `main.zip`, `extract_flatten` strips the GitHub-injected top-level dir (e.g. `mhr-cfw-main/`), `pip install --upgrade pip` then `-r requirements.txt`, then write the marker.
3. Emit `InstallDone`; GUI flips to `Ready`.

## Connect pipeline (`handle_connect` in gui.rs)

Triggered by **Connect**. Order matters and is load-bearing for correctness:

1. **Python** — same `ensure_python` path as install.
2. **Defensive reinstall** — if `config::is_installed()` is somehow false, re-run the project download + pip install. The GUI normally gates Connect on this, but the worker rechecks.
3. **Credentials** — persisted to two locations:
   - Project's `config.json` (merged via `Config::save`, preserving every other key; seeded from `config.example.json` if missing). The Python child reads this on startup.
   - `%APPDATA%\hamid-mahdavi-client\credentials.json` via `credentials::save`. This survives a project reinstall and is preferred over `config.json` when prefilling the GUI on next launch.
4. **Spawn child** — `Runner::spawn` runs `python main.py --no-cert-check` with `current_dir = C:\hamid-mahdavi-client`, piped stdout/stderr, null stdin, and `CREATE_NO_WINDOW` so no console flashes.
5. **Cert** — gated by `cert::is_installed()` (marker file `.cert-installed`). If absent, `cert::install` runs in two steps:
   - `generate_ca`: invoke `python -c "from mitm import MITMCertManager; MITMCertManager()"` (with `sys.path` set to `<project>/src`) to write `<project>/ca/ca.crt` + `ca.key` if missing. Constructor is import-side-effect free; the install side lives in a separate upstream function we deliberately don't call.
   - `install_to_machine_root`: `certutil -addstore -f Root <ca.crt>`. Uses the **LocalMachine** trust store, which is silent under elevation (our manifest forces it). We bypass `python main.py --install-cert` upstream because that path uses `certutil -addstore -user Root`, which always pops a Windows confirmation dialog.
   - On success, `cert::mark_installed` writes the marker. On failure, log a warning but continue — the upstream may have already installed the cert, and we leave the marker absent so we'll retry next connect.
6. **Proxy** — `proxy::enable(PROXY_HOST_PORT)` writes `ProxyEnable=1`, `ProxyServer=127.0.0.1:8085`, and a long `ProxyOverride` bypass list to `HKCU\...\Internet Settings`, then broadcasts `WM_SETTINGCHANGE` (twice — once null lParam, once `"Internet"`) so browsers re-read.
7. **Health check** — sleep 2s, then `runner.poll()`; if the child already exited, the whole connect fails.

On `Disconnect` or window close: kill the child, then `proxy::disable()` clears `ProxyEnable` (leaves `ProxyServer`/`ProxyOverride` so users can re-enable a custom proxy).

## Python detection / install (`python.rs`)

`python::detect()` looks at `py.exe`/`python.exe`/`python3.exe` on `PATH`, parses `-V` output, requires ≥3.10. If absent, downloads the 3.11.9 installer matching pointer width and runs it silently with `InstallAllUsers=1 PrependPath=1`. After install, `detect()` is retried, then `detect_in_known_paths()` falls back to `C:\Program Files\Python31{0,1}\python.exe` because the current process won't see the updated PATH.

## Important hard-coded paths and constants

In `src/paths.rs`:

- Project root: `C:\hamid-mahdavi-client` (not configurable; many modules assume this)
- Project zip: `https://github.com/denuitt1/mhr-cfw/archive/refs/heads/main.zip`
- Proxy: `127.0.0.1:8085`
- Install marker: `C:\hamid-mahdavi-client\.launcher-installed`
- Cert marker: `C:\hamid-mahdavi-client\.cert-installed`
- CA cert (generated by upstream's `MITMCertManager`): `C:\hamid-mahdavi-client\ca\ca.crt`
- Per-user credentials store: `%APPDATA%\hamid-mahdavi-client\credentials.json` (lives outside project dir so it survives a reinstall)
- Failure reports: `C:\hamid-mahdavi-client\logs\report-YYYYMMDD-HHMMSS.txt`

Python installer URLs and minimum version live in `src/python.rs`.

## Module map

```
main.rs        cfg(windows) entrypoint; on error, MessageBoxW then exit. Non-Windows main prints and exits.
gui.rs         Window construction, event loop, worker thread, Mode state machine, install/connect/disconnect/report orchestration.
python.rs      Detect (PATH + known dirs) and silent-install Python; parse_version handles "Python 3.x.y\n" forms.
project.rs     Zip download + flatten extract + pip install.
config.rs      Read/write project config.json preserving unknown keys; install marker management.
credentials.rs Per-user credentials store at %APPDATA%\hamid-mahdavi-client\credentials.json (survives reinstall).
proxy.rs       HKCU registry writes + WM_SETTINGCHANGE broadcast (Windows-gated).
cert.rs        Bootstraps upstream's MITMCertManager in-process to mint ca.crt, then certutil -addstore -f Root (LocalMachine, silent under elevation). Marker-gated.
runner.rs      Child process supervisor + LogRing (1000-line ring buffer used by GUI and report). Spawns python main.py --no-cert-check.
download.rs    Streaming ureq → file with throttled progress callbacks (~256 KiB).
report.rs      Builds a single text dump (env + LogRing snapshot) for AI-assisted diagnosis.
util.rs        no_console (CREATE_NO_WINDOW for child Commands) and open_path (ShellExecuteW). Both have non-Windows stubs so cargo check stays green.
paths.rs       Constants and PathBuf helpers.
```

## Cross-platform gotchas when editing

- `gui.rs`, `cert.rs`, `proxy.rs` are `#[cfg(windows)]` modules and the `windows`/`winreg`/`native-windows-gui` crates are `cfg(windows)` deps. Anything imported from them won't compile on macOS/Linux. Keep cross-platform modules (`config`, `credentials`, `download`, `paths`, `project`, `python`, `report`, `runner`, `util`) free of those imports so `cargo check` stays green on any host.
- `proxy.rs` and `util.rs` already use `#[cfg(windows)]`/`#[cfg(not(windows))]` arms inside their functions — match that pattern if adding more Windows-only behavior to otherwise-portable modules.
- All child `Command`s should go through `util::no_console` to avoid flashing console windows. Spawning bare `Command::new(...)` will regress this on Windows.
