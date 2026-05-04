use std::path::PathBuf;

pub const PROJECT_DIR: &str = r"C:\mhr-cfw";
pub const PROJECT_REPO_ZIP: &str =
    "https://github.com/denuitt1/mhr-cfw/archive/refs/heads/main.zip";
pub const PROXY_HOST_PORT: &str = "127.0.0.1:8085";

pub fn project_dir() -> PathBuf {
    PathBuf::from(PROJECT_DIR)
}

pub fn config_path() -> PathBuf {
    project_dir().join("config.json")
}

pub fn config_example_path() -> PathBuf {
    project_dir().join("config.example.json")
}

pub fn requirements_path() -> PathBuf {
    project_dir().join("requirements.txt")
}

pub fn main_script_path() -> PathBuf {
    project_dir().join("main.py")
}

pub fn install_marker_path() -> PathBuf {
    project_dir().join(".launcher-installed")
}

pub fn log_dir() -> PathBuf {
    project_dir().join("logs")
}
