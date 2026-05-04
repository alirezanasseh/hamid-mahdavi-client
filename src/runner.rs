use anyhow::{Context, Result};
use std::collections::VecDeque;
use std::io::{BufRead, BufReader};
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

use crate::paths;
use crate::python::PythonInfo;
use crate::util;

const LOG_RING_CAP: usize = 1000;

/// Shared, capped ring buffer of log lines from the child process. Used by
/// the GUI to show recent output and by the report module for failure dumps.
#[derive(Clone, Default)]
pub struct LogRing {
    inner: Arc<Mutex<VecDeque<String>>>,
}

impl LogRing {
    pub fn new() -> Self {
        Self {
            inner: Arc::new(Mutex::new(VecDeque::with_capacity(LOG_RING_CAP))),
        }
    }

    pub fn push(&self, line: String) {
        let mut g = self.inner.lock().unwrap();
        if g.len() == LOG_RING_CAP {
            g.pop_front();
        }
        g.push_back(line);
    }

    pub fn snapshot(&self) -> Vec<String> {
        self.inner.lock().unwrap().iter().cloned().collect()
    }
}

/// Handle to a running `python main.py` child. Drop it to terminate.
pub struct Runner {
    child: Child,
    pub logs: LogRing,
}

impl Runner {
    pub fn spawn(py: &PythonInfo, logs: LogRing) -> Result<Self> {
        let main_py = paths::main_script_path();
        let mut child = util::no_console(
            Command::new(&py.exe)
                .arg(&main_py)
                .current_dir(paths::project_dir())
                .stdout(Stdio::piped())
                .stderr(Stdio::piped())
                .stdin(Stdio::null()),
        )
        .spawn()
        .context("spawning python main.py")?;

        if let Some(stdout) = child.stdout.take() {
            let logs = logs.clone();
            thread::spawn(move || pipe_lines(stdout, logs, "out"));
        }
        if let Some(stderr) = child.stderr.take() {
            let logs = logs.clone();
            thread::spawn(move || pipe_lines(stderr, logs, "err"));
        }

        Ok(Self { child, logs })
    }

    /// Returns Some(exit) if the child has exited, None if still running.
    pub fn poll(&mut self) -> Result<Option<std::process::ExitStatus>> {
        Ok(self.child.try_wait().context("polling python child")?)
    }

    pub fn kill(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

impl Drop for Runner {
    fn drop(&mut self) {
        self.kill();
    }
}

fn pipe_lines<R: std::io::Read>(reader: R, logs: LogRing, channel: &str) {
    let buf = BufReader::new(reader);
    for line in buf.lines().flatten() {
        logs.push(format!("[{channel}] {line}"));
    }
}
