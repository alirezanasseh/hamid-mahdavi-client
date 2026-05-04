use anyhow::Result;
use native_windows_gui as nwg;
use std::cell::{Cell, RefCell};
use std::path::PathBuf;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::config::Config;
use crate::python::PythonInfo;
use crate::runner::{LogRing, Runner};
use crate::{cert, config, credentials, paths, project, proxy, python, report, util};

/// Messages from the worker thread to the GUI.
#[derive(Debug, Clone)]
pub enum WorkerMsg {
    Progress(String),
    InstallDone,
    Connected,
    Disconnected,
    Failed(String),
    ReportSaved(PathBuf),
    SetPython { exe: PathBuf, version: (u32, u32, u32) },
}

#[derive(Debug, Clone)]
pub enum GuiCmd {
    Install,
    Connect { credentials: Config },
    Disconnect,
    SaveReport,
}

/// The user-visible lifecycle of the launcher. Drives the primary button's
/// label / enabled state and Save Report visibility.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Mode {
    NeedsInstall,
    Installing,
    Ready,
    Connecting,
    Connected,
    Disconnecting,
}

impl Mode {
    fn button_label(self) -> &'static str {
        match self {
            Mode::NeedsInstall => "Install",
            Mode::Installing => "Installing...",
            Mode::Ready => "Connect",
            Mode::Connecting => "Connecting...",
            Mode::Connected => "Disconnect",
            Mode::Disconnecting => "Disconnecting...",
        }
    }

    fn button_enabled(self) -> bool {
        matches!(self, Mode::NeedsInstall | Mode::Ready | Mode::Connected)
    }
}

/// Top-level state shared between threads.
pub struct AppState {
    pub python: Mutex<Option<PythonInfo>>,
    pub runner: Mutex<Option<Runner>>,
    pub logs: LogRing,
    pub proxy_set: Mutex<bool>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            python: Mutex::new(None),
            runner: Mutex::new(None),
            logs: LogRing::new(),
            proxy_set: Mutex::new(false),
        }
    }
}

/// Entry: build window, run event loop, return when user quits.
pub fn run() -> Result<()> {
    nwg::init().map_err(|e| anyhow::anyhow!("nwg init failed: {e:?}"))?;
    nwg::Font::set_global_family("Segoe UI")
        .map_err(|e| anyhow::anyhow!("font set failed: {e:?}"))?;

    let state = Arc::new(AppState::new());

    // Load the icon embedded by app.rc (resource ID 1) so it shows on the
    // window title bar, taskbar, and alt-tab. Explorer/taskbar already pick
    // the EXE icon from the same resource automatically.
    let embed = nwg::EmbedResource::load(None)
        .map_err(|e| anyhow::anyhow!("embed resource load: {e:?}"))?;
    let app_icon = embed.icon(1, None);

    let mut window = nwg::Window::default();
    let mut status_label = nwg::Label::default();
    let mut log_box = nwg::TextBox::default();
    let mut script_id_label = nwg::Label::default();
    let mut script_id_input = nwg::TextInput::default();
    let mut auth_key_label = nwg::Label::default();
    let mut auth_key_input = nwg::TextInput::default();
    let mut action_btn = nwg::Button::default();
    let mut report_btn = nwg::Button::default();
    let mut progress = nwg::ProgressBar::default();
    let mut tick_timer = nwg::AnimationTimer::default();

    nwg::Window::builder()
        .size((560, 460))
        .position((300, 200))
        .title("mhr-cfw VPN")
        .icon(app_icon.as_ref())
        .flags(nwg::WindowFlags::WINDOW | nwg::WindowFlags::VISIBLE)
        .build(&mut window)
        .map_err(|e| anyhow::anyhow!("window build: {e:?}"))?;

    nwg::Label::builder()
        .text("Status: idle")
        .position((16, 12))
        .size((520, 24))
        .parent(&window)
        .build(&mut status_label)
        .map_err(|e| anyhow::anyhow!("status label: {e:?}"))?;

    nwg::Label::builder()
        .text("Script ID:")
        .position((16, 48))
        .size((90, 22))
        .parent(&window)
        .build(&mut script_id_label)
        .map_err(|e| anyhow::anyhow!("script id label: {e:?}"))?;

    nwg::TextInput::builder()
        .position((110, 46))
        .size((426, 24))
        .parent(&window)
        .build(&mut script_id_input)
        .map_err(|e| anyhow::anyhow!("script id input: {e:?}"))?;

    nwg::Label::builder()
        .text("Auth Key:")
        .position((16, 80))
        .size((90, 22))
        .parent(&window)
        .build(&mut auth_key_label)
        .map_err(|e| anyhow::anyhow!("auth key label: {e:?}"))?;

    nwg::TextInput::builder()
        .position((110, 78))
        .size((426, 24))
        .parent(&window)
        .build(&mut auth_key_input)
        .map_err(|e| anyhow::anyhow!("auth key input: {e:?}"))?;

    // Single primary action button. The label and enabled state morph based on Mode.
    nwg::Button::builder()
        .text("Connect")
        .position((16, 116))
        .size((200, 32))
        .parent(&window)
        .build(&mut action_btn)
        .map_err(|e| anyhow::anyhow!("action btn: {e:?}"))?;

    // Save Report sits to the right and is hidden until something fails.
    nwg::Button::builder()
        .text("Save Report")
        .position((376, 116))
        .size((160, 32))
        .parent(&window)
        .build(&mut report_btn)
        .map_err(|e| anyhow::anyhow!("report btn: {e:?}"))?;
    report_btn.set_visible(false);

    nwg::ProgressBar::builder()
        .position((16, 156))
        .size((520, 18))
        .range(0..100)
        .parent(&window)
        .build(&mut progress)
        .map_err(|e| anyhow::anyhow!("progress: {e:?}"))?;

    nwg::TextBox::builder()
        .position((16, 184))
        .size((520, 250))
        .parent(&window)
        .readonly(true)
        .flags(
            nwg::TextBoxFlags::VISIBLE
                | nwg::TextBoxFlags::VSCROLL
                | nwg::TextBoxFlags::AUTOVSCROLL,
        )
        .build(&mut log_box)
        .map_err(|e| anyhow::anyhow!("log box: {e:?}"))?;

    nwg::AnimationTimer::builder()
        .interval(std::time::Duration::from_millis(120))
        .parent(&window)
        .build(&mut tick_timer)
        .map_err(|e| anyhow::anyhow!("timer: {e:?}"))?;

    // Pre-fill credentials. Prefer the persistent per-user store
    // (%APPDATA%\hamid-mahdavi-client\credentials.json), then fall back to the
    // project's config.json. Either way, both fields show up across runs.
    if let Some((sid, ak)) = credentials::load() {
        script_id_input.set_text(&sid);
        auth_key_input.set_text(&ak);
    } else if let Ok(Some(cfg)) = Config::load() {
        script_id_input.set_text(&cfg.script_id);
        auth_key_input.set_text(&cfg.auth_key);
    }

    // Initial mode: Install button only on a fresh machine; Connect thereafter.
    let initial_mode = if config::is_installed() {
        Mode::Ready
    } else {
        Mode::NeedsInstall
    };
    let mode = Rc::new(Cell::new(initial_mode));
    action_btn.set_text(initial_mode.button_label());
    action_btn.set_enabled(initial_mode.button_enabled());

    // Channels: GUI → worker (commands), worker → GUI (status updates).
    let (gui_tx, gui_rx) = mpsc::channel::<GuiCmd>();
    let (worker_tx, worker_rx) = mpsc::channel::<WorkerMsg>();
    let worker_rx = Rc::new(RefCell::new(worker_rx));

    spawn_worker(state.clone(), gui_rx, worker_tx);

    // Wrap shared widgets in Rc so the event closure can borrow them.
    let window = Rc::new(window);
    let status_label = Rc::new(status_label);
    let log_box = Rc::new(log_box);
    let progress = Rc::new(progress);
    let script_id_input = Rc::new(script_id_input);
    let auth_key_input = Rc::new(auth_key_input);
    let action_btn = Rc::new(action_btn);
    let report_btn = Rc::new(report_btn);
    let last_report: Rc<RefCell<Option<PathBuf>>> = Rc::new(RefCell::new(None));

    let action_btn_h = action_btn.handle;
    let report_btn_h = report_btn.handle;
    let timer_h = tick_timer.handle;
    let window_h = window.handle;

    tick_timer.start();

    let gui_tx_evt = gui_tx.clone();
    let state_evt = state.clone();
    let status_label_evt = status_label.clone();
    let log_box_evt = log_box.clone();
    let progress_evt = progress.clone();
    let script_id_evt = script_id_input.clone();
    let auth_key_evt = auth_key_input.clone();
    let worker_rx_evt = worker_rx.clone();
    let action_btn_evt = action_btn.clone();
    let report_btn_evt = report_btn.clone();
    let mode_evt = mode.clone();
    let last_report_evt = last_report.clone();

    let _handler = nwg::full_bind_event_handler(&window.handle, move |evt, _data, handle| {
        use nwg::Event as E;
        match evt {
            E::OnButtonClick => {
                if handle == action_btn_h {
                    on_action_click(
                        &mode_evt,
                        &gui_tx_evt,
                        &script_id_evt,
                        &auth_key_evt,
                        &action_btn_evt,
                        &report_btn_evt,
                        &status_label_evt,
                        &progress_evt,
                    );
                } else if handle == report_btn_h {
                    // If the report has already been saved this session, just open it again
                    // — no need to re-write the file. Otherwise ask the worker to write one
                    // (it will send ReportSaved when done and we'll open it then).
                    if let Some(path) = last_report_evt.borrow().clone() {
                        if let Err(e) = util::open_path(&path) {
                            status_label_evt
                                .set_text(&format!("Status: could not open report — {e:#}"));
                        }
                    } else {
                        let _ = gui_tx_evt.send(GuiCmd::SaveReport);
                    }
                }
            }
            E::OnTimerTick => {
                if handle == timer_h {
                    drain_messages(
                        &worker_rx_evt,
                        &status_label_evt,
                        &log_box_evt,
                        &progress_evt,
                        &state_evt,
                        &mode_evt,
                        &action_btn_evt,
                        &report_btn_evt,
                        &last_report_evt,
                    );
                }
            }
            E::OnWindowClose => {
                if handle == window_h {
                    // Best-effort cleanup before quitting.
                    if let Ok(mut runner) = state_evt.runner.lock() {
                        if let Some(mut r) = runner.take() {
                            r.kill();
                        }
                    }
                    if *state_evt.proxy_set.lock().unwrap() {
                        let _ = proxy::disable();
                    }
                    nwg::stop_thread_dispatch();
                }
            }
            _ => {}
        }
    });

    nwg::dispatch_thread_events();
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn on_action_click(
    mode: &Rc<Cell<Mode>>,
    gui_tx: &Sender<GuiCmd>,
    script_id: &Rc<nwg::TextInput>,
    auth_key: &Rc<nwg::TextInput>,
    action_btn: &Rc<nwg::Button>,
    report_btn: &Rc<nwg::Button>,
    status: &Rc<nwg::Label>,
    progress: &Rc<nwg::ProgressBar>,
) {
    // Any new user-initiated action hides a stale Save Report from a previous failure.
    report_btn.set_visible(false);

    match mode.get() {
        Mode::NeedsInstall => {
            set_mode(mode, Mode::Installing, action_btn);
            status.set_text("Status: installing...");
            progress.set_pos(0);
            let _ = gui_tx.send(GuiCmd::Install);
        }
        Mode::Ready => {
            let cfg = Config {
                script_id: script_id.text(),
                auth_key: auth_key.text(),
            };
            if !cfg.is_complete() {
                status.set_text(
                    "Status: please fill both Script ID and Auth Key before connecting",
                );
                report_btn.set_visible(false);
                return;
            }
            set_mode(mode, Mode::Connecting, action_btn);
            status.set_text("Status: connecting...");
            progress.set_pos(0);
            let _ = gui_tx.send(GuiCmd::Connect { credentials: cfg });
        }
        Mode::Connected => {
            set_mode(mode, Mode::Disconnecting, action_btn);
            status.set_text("Status: disconnecting...");
            let _ = gui_tx.send(GuiCmd::Disconnect);
        }
        // In-flight states shouldn't be reachable (button is disabled), but be safe.
        Mode::Installing | Mode::Connecting | Mode::Disconnecting => {}
    }
}

fn set_mode(mode_cell: &Rc<Cell<Mode>>, new_mode: Mode, btn: &Rc<nwg::Button>) {
    mode_cell.set(new_mode);
    btn.set_text(new_mode.button_label());
    btn.set_enabled(new_mode.button_enabled());
}

#[allow(clippy::too_many_arguments)]
fn drain_messages(
    rx: &Rc<RefCell<Receiver<WorkerMsg>>>,
    status: &Rc<nwg::Label>,
    log_box: &Rc<nwg::TextBox>,
    progress: &Rc<nwg::ProgressBar>,
    state: &Arc<AppState>,
    mode: &Rc<Cell<Mode>>,
    action_btn: &Rc<nwg::Button>,
    report_btn: &Rc<nwg::Button>,
    last_report: &Rc<RefCell<Option<PathBuf>>>,
) {
    let rx = rx.borrow();
    while let Ok(msg) = rx.try_recv() {
        match msg {
            WorkerMsg::Progress(s) => {
                status.set_text(&format!("Status: {s}"));
                let mut p = progress.pos();
                if p < 95 {
                    p += 5;
                }
                progress.set_pos(p);
            }
            WorkerMsg::SetPython { exe, version } => {
                *state.python.lock().unwrap() = Some(PythonInfo { exe, version });
            }
            WorkerMsg::InstallDone => {
                progress.set_pos(100);
                status.set_text(
                    "Status: installation complete — enter credentials and click Connect",
                );
                set_mode(mode, Mode::Ready, action_btn);
            }
            WorkerMsg::Connected => {
                status.set_text("Status: connected (proxy 127.0.0.1:8085)");
                progress.set_pos(100);
                set_mode(mode, Mode::Connected, action_btn);
            }
            WorkerMsg::Disconnected => {
                status.set_text("Status: disconnected");
                progress.set_pos(0);
                set_mode(mode, Mode::Ready, action_btn);
            }
            WorkerMsg::Failed(reason) => {
                status.set_text(&format!("Status: failed — {reason}"));
                progress.set_pos(0);
                // Roll back to whatever idle state matches what we were trying to do.
                let revert_to = match mode.get() {
                    Mode::Installing => Mode::NeedsInstall,
                    _ => Mode::Ready,
                };
                set_mode(mode, revert_to, action_btn);
                // Failure → offer the report. Drop any cached path so the next click
                // writes a fresh report capturing the current logs.
                last_report.borrow_mut().take();
                report_btn.set_visible(true);
            }
            WorkerMsg::ReportSaved(path) => {
                status.set_text(&format!("Status: report saved → {}", path.display()));
                if let Err(e) = util::open_path(&path) {
                    status.set_text(&format!(
                        "Status: report saved at {} (could not auto-open: {e:#})",
                        path.display()
                    ));
                }
                *last_report.borrow_mut() = Some(path);
            }
        }
    }

    // Refresh log box from the ring buffer (cheap; bounded to 1000 lines).
    let lines = state.logs.snapshot();
    let joined = lines.join("\r\n");
    if joined != log_box.text() {
        log_box.set_text(&joined);
    }
}

fn spawn_worker(
    state: Arc<AppState>,
    rx: Receiver<GuiCmd>,
    tx: Sender<WorkerMsg>,
) {
    thread::spawn(move || {
        for cmd in rx {
            match cmd {
                GuiCmd::Install => {
                    if let Err(e) = handle_install(&state, &tx) {
                        let _ = tx.send(WorkerMsg::Failed(format!("{e:#}")));
                    }
                }
                GuiCmd::Connect { credentials: cfg } => {
                    if let Err(e) = handle_connect(&state, &tx, cfg) {
                        let _ = tx.send(WorkerMsg::Failed(format!("{e:#}")));
                    }
                }
                GuiCmd::Disconnect => handle_disconnect(&state, &tx),
                GuiCmd::SaveReport => handle_save_report(&state, &tx),
            }
        }
    });
}

/// First-run setup: ensure Python, download project, pip install. No network /
/// child VPN. Connecting is a separate step the user triggers afterward.
fn handle_install(state: &Arc<AppState>, tx: &Sender<WorkerMsg>) -> Result<()> {
    let py = ensure_python(state, tx)?;

    if !config::is_installed() {
        let tx2 = tx.clone();
        project::download_and_extract(move |s| {
            let _ = tx2.send(WorkerMsg::Progress(s.to_string()));
        })?;
        let tx2 = tx.clone();
        project::pip_install(&py, move |s| {
            let _ = tx2.send(WorkerMsg::Progress(s.to_string()));
        })?;
        config::mark_installed()?;
    } else {
        let _ = tx.send(WorkerMsg::Progress(
            "Project already installed".into(),
        ));
    }

    let _ = tx.send(WorkerMsg::InstallDone);
    Ok(())
}

fn handle_connect(
    state: &Arc<AppState>,
    tx: &Sender<WorkerMsg>,
    cfg: Config,
) -> Result<()> {
    let py = ensure_python(state, tx)?;

    // Defensive: if somehow we got here without an install (shouldn't happen
    // because the GUI gates Connect on is_installed), do it now.
    if !config::is_installed() {
        let tx2 = tx.clone();
        project::download_and_extract(move |s| {
            let _ = tx2.send(WorkerMsg::Progress(s.to_string()));
        })?;
        let tx2 = tx.clone();
        project::pip_install(&py, move |s| {
            let _ = tx2.send(WorkerMsg::Progress(s.to_string()));
        })?;
        config::mark_installed()?;
    }

    // Persist credentials in two places: the project's config.json (which the
    // python child reads on startup) and the per-user store outside the project
    // dir (so they survive a reinstall).
    cfg.save()?;
    if let Err(e) = credentials::save(&cfg.script_id, &cfg.auth_key) {
        state
            .logs
            .push(format!("[launcher] credentials persistence warning: {e:#}"));
    }

    // Spawn the VPN child.
    let _ = tx.send(WorkerMsg::Progress("Starting VPN process...".into()));
    let runner = Runner::spawn(&py, state.logs.clone())?;
    *state.runner.lock().unwrap() = Some(runner);

    // Cert install: only on the first connect. The mhr-cfw project is itself
    // idempotent here, but invoking its `--install-cert` path on every run is
    // slow and pops a console window from the project's own subprocess.
    if !cert::is_installed() {
        let _ = tx.send(WorkerMsg::Progress("Installing certificate...".into()));
        match cert::install(&py) {
            Ok(()) => {
                if let Err(e) = cert::mark_installed() {
                    state
                        .logs
                        .push(format!("[launcher] cert marker write warning: {e:#}"));
                }
            }
            Err(e) => {
                // Don't abort: the cert may already be installed by the project itself.
                // Don't write the marker either, so we'll retry next run.
                state
                    .logs
                    .push(format!("[launcher] cert install warning: {e:#}"));
            }
        }
    }

    // Proxy
    proxy::enable(paths::PROXY_HOST_PORT)?;
    *state.proxy_set.lock().unwrap() = true;

    // Health check: did the child stay alive for ~2s?
    std::thread::sleep(std::time::Duration::from_secs(2));
    if let Some(runner) = state.runner.lock().unwrap().as_mut() {
        if let Some(status) = runner.poll()? {
            return Err(anyhow::anyhow!(
                "VPN process exited immediately (code {:?})",
                status.code()
            ));
        }
    }

    let _ = tx.send(WorkerMsg::Connected);
    Ok(())
}

/// Detect or install Python, caching the result on AppState.
fn ensure_python(state: &Arc<AppState>, tx: &Sender<WorkerMsg>) -> Result<PythonInfo> {
    if let Some(p) = state.python.lock().unwrap().clone() {
        return Ok(p);
    }
    let py = if let Some(p) = python::detect() {
        p
    } else {
        let _ = tx.send(WorkerMsg::Progress("Installing Python 3.11...".into()));
        let tx2 = tx.clone();
        python::install(move |s| {
            let _ = tx2.send(WorkerMsg::Progress(s.to_string()));
        })?
    };
    let _ = tx.send(WorkerMsg::Progress(format!(
        "Python {} ready",
        py.version_string()
    )));
    let _ = tx.send(WorkerMsg::SetPython {
        exe: py.exe.clone(),
        version: py.version,
    });
    *state.python.lock().unwrap() = Some(py.clone());
    Ok(py)
}

fn handle_disconnect(state: &Arc<AppState>, tx: &Sender<WorkerMsg>) {
    if let Some(mut runner) = state.runner.lock().unwrap().take() {
        runner.kill();
    }
    if *state.proxy_set.lock().unwrap() {
        let _ = proxy::disable();
        *state.proxy_set.lock().unwrap() = false;
    }
    let _ = tx.send(WorkerMsg::Disconnected);
}

fn handle_save_report(state: &Arc<AppState>, tx: &Sender<WorkerMsg>) {
    let py_guard = state.python.lock().unwrap();
    let py_ref = py_guard.as_ref();
    match report::write("user-requested report", py_ref, &state.logs) {
        Ok(path) => {
            let _ = tx.send(WorkerMsg::ReportSaved(path));
        }
        Err(e) => {
            let _ = tx.send(WorkerMsg::Failed(format!("could not save report: {e:#}")));
        }
    }
}
