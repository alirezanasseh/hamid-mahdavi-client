use anyhow::Result;
use native_windows_gui as nwg;
use std::cell::RefCell;
use std::rc::Rc;
use std::sync::mpsc::{self, Receiver, Sender};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::config::Config;
use crate::python::PythonInfo;
use crate::runner::{LogRing, Runner};
use crate::{cert, config, paths, project, proxy, python, report};

/// Messages from the worker thread to the GUI.
#[derive(Debug, Clone)]
pub enum WorkerMsg {
    Progress(String),
    SetupDone(SetupOutcome),
    Connected,
    Failed(String),
    ChildExited(String),
}

#[derive(Debug, Clone)]
pub struct SetupOutcome {
    pub python_exe: std::path::PathBuf,
    pub python_version: (u32, u32, u32),
}

#[derive(Debug, Clone)]
pub enum GuiCmd {
    StartSetupAndConnect { credentials: Option<Config> },
    Disconnect,
    SaveReport,
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
    let mut window = nwg::Window::default();
    let mut status_label = nwg::Label::default();
    let mut log_box = nwg::TextBox::default();
    let mut script_id_label = nwg::Label::default();
    let mut script_id_input = nwg::TextInput::default();
    let mut auth_key_label = nwg::Label::default();
    let mut auth_key_input = nwg::TextInput::default();
    let mut connect_btn = nwg::Button::default();
    let mut disconnect_btn = nwg::Button::default();
    let mut report_btn = nwg::Button::default();
    let mut progress = nwg::ProgressBar::default();
    let mut tick_timer = nwg::AnimationTimer::default();

    nwg::Window::builder()
        .size((560, 460))
        .position((300, 200))
        .title("mhr-cfw VPN")
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

    nwg::Button::builder()
        .text("Connect")
        .position((16, 116))
        .size((120, 32))
        .parent(&window)
        .build(&mut connect_btn)
        .map_err(|e| anyhow::anyhow!("connect btn: {e:?}"))?;

    nwg::Button::builder()
        .text("Disconnect")
        .position((148, 116))
        .size((120, 32))
        .parent(&window)
        .build(&mut disconnect_btn)
        .map_err(|e| anyhow::anyhow!("disconnect btn: {e:?}"))?;

    nwg::Button::builder()
        .text("Save Report")
        .position((280, 116))
        .size((120, 32))
        .parent(&window)
        .build(&mut report_btn)
        .map_err(|e| anyhow::anyhow!("report btn: {e:?}"))?;

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

    // Pre-fill credentials if a saved config exists.
    if let Ok(Some(cfg)) = Config::load() {
        script_id_input.set_text(&cfg.script_id);
        auth_key_input.set_text(&cfg.auth_key);
    }

    // Channels: GUI → worker (commands), worker → GUI (status updates).
    let (gui_tx, gui_rx) = mpsc::channel::<GuiCmd>();
    let (worker_tx, worker_rx) = mpsc::channel::<WorkerMsg>();
    let worker_rx = Rc::new(RefCell::new(worker_rx));

    spawn_worker(state.clone(), gui_rx, worker_tx);

    // We need handles inside the event closure; clone them via Rc so they
    // outlive the borrow of `window`.
    let window = Rc::new(window);
    let status_label = Rc::new(status_label);
    let log_box = Rc::new(log_box);
    let progress = Rc::new(progress);
    let script_id_input = Rc::new(script_id_input);
    let auth_key_input = Rc::new(auth_key_input);
    let connect_btn_h = connect_btn.handle;
    let disconnect_btn_h = disconnect_btn.handle;
    let report_btn_h = report_btn.handle;
    let timer_h = tick_timer.handle;
    let window_h = window.handle;

    let connect_btn = Rc::new(connect_btn);
    let disconnect_btn = Rc::new(disconnect_btn);
    let report_btn = Rc::new(report_btn);

    tick_timer.start();

    let gui_tx_evt = gui_tx.clone();
    let state_evt = state.clone();
    let status_label_evt = status_label.clone();
    let log_box_evt = log_box.clone();
    let progress_evt = progress.clone();
    let script_id_evt = script_id_input.clone();
    let auth_key_evt = auth_key_input.clone();
    let worker_rx_evt = worker_rx.clone();

    let _handler = nwg::full_bind_event_handler(&window.handle, move |evt, _data, handle| {
        use nwg::Event as E;
        match evt {
            E::OnButtonClick => {
                if handle == connect_btn_h {
                    let cfg = Config {
                        script_id: script_id_evt.text(),
                        auth_key: auth_key_evt.text(),
                    };
                    let credentials = if cfg.is_complete() { Some(cfg) } else { None };
                    let _ = gui_tx_evt.send(GuiCmd::StartSetupAndConnect { credentials });
                } else if handle == disconnect_btn_h {
                    let _ = gui_tx_evt.send(GuiCmd::Disconnect);
                } else if handle == report_btn_h {
                    let _ = gui_tx_evt.send(GuiCmd::SaveReport);
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

fn drain_messages(
    rx: &Rc<RefCell<Receiver<WorkerMsg>>>,
    status: &Rc<nwg::Label>,
    log_box: &Rc<nwg::TextBox>,
    progress: &Rc<nwg::ProgressBar>,
    state: &Arc<AppState>,
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
            WorkerMsg::SetupDone(o) => {
                *state.python.lock().unwrap() = Some(PythonInfo {
                    exe: o.python_exe,
                    version: o.python_version,
                });
                progress.set_pos(100);
            }
            WorkerMsg::Connected => {
                status.set_text("Status: connected (proxy 127.0.0.1:8085)");
                progress.set_pos(100);
            }
            WorkerMsg::Failed(reason) => {
                status.set_text(&format!("Status: failed — {reason}"));
                progress.set_pos(0);
            }
            WorkerMsg::ChildExited(detail) => {
                status.set_text(&format!("Status: stopped — {detail}"));
                progress.set_pos(0);
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
                GuiCmd::StartSetupAndConnect { credentials } => {
                    if let Err(e) =
                        handle_connect(&state, &tx, credentials)
                    {
                        let _ = tx.send(WorkerMsg::Failed(format!("{e:#}")));
                    }
                }
                GuiCmd::Disconnect => {
                    handle_disconnect(&state, &tx);
                }
                GuiCmd::SaveReport => {
                    handle_save_report(&state, &tx);
                }
            }
        }
    });
}

fn handle_connect(
    state: &Arc<AppState>,
    tx: &Sender<WorkerMsg>,
    credentials: Option<Config>,
) -> Result<()> {
    // 1. Python
    let py = if let Some(p) = state.python.lock().unwrap().clone() {
        p
    } else if let Some(p) = python::detect() {
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

    // 2. Project
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
            "Project already installed, skipping setup".into(),
        ));
    }

    // 3. Credentials
    let cfg = match credentials {
        Some(c) if c.is_complete() => c,
        _ => match Config::load()? {
            Some(c) if c.is_complete() => c,
            _ => {
                return Err(anyhow::anyhow!(
                    "missing script_id or auth_key — fill both fields"
                ));
            }
        },
    };
    cfg.save()?;

    let _ = tx.send(WorkerMsg::SetupDone(SetupOutcome {
        python_exe: py.exe.clone(),
        python_version: py.version,
    }));

    // 4. Spawn the VPN child.
    let _ = tx.send(WorkerMsg::Progress("Starting VPN process...".into()));
    let runner = Runner::spawn(&py, state.logs.clone())?;
    *state.runner.lock().unwrap() = Some(runner);

    // 5. Cert (best-effort — the prompt may already have appeared).
    let _ = tx.send(WorkerMsg::Progress("Ensuring certificate is installed...".into()));
    if let Err(e) = cert::install(&py) {
        // Don't abort: the cert may already be installed.
        state
            .logs
            .push(format!("[launcher] cert install warning: {e:#}"));
    }

    // 6. Proxy
    proxy::enable(paths::PROXY_HOST_PORT)?;
    *state.proxy_set.lock().unwrap() = true;

    // 7. Health check: did the child stay alive for ~2s?
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

fn handle_disconnect(state: &Arc<AppState>, tx: &Sender<WorkerMsg>) {
    if let Some(mut runner) = state.runner.lock().unwrap().take() {
        runner.kill();
    }
    if *state.proxy_set.lock().unwrap() {
        let _ = proxy::disable();
        *state.proxy_set.lock().unwrap() = false;
    }
    let _ = tx.send(WorkerMsg::ChildExited("disconnected by user".into()));
}

fn handle_save_report(state: &Arc<AppState>, tx: &Sender<WorkerMsg>) {
    let py_guard = state.python.lock().unwrap();
    let py_ref = py_guard.as_ref();
    match report::write("user-requested report", py_ref, &state.logs) {
        Ok(path) => {
            let _ = tx.send(WorkerMsg::Progress(format!(
                "Report saved: {}",
                path.display()
            )));
        }
        Err(e) => {
            let _ = tx.send(WorkerMsg::Failed(format!("could not save report: {e:#}")));
        }
    }
}
