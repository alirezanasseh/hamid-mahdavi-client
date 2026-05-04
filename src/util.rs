use std::process::Command;

/// Suppress the brief console window that Windows would otherwise flash when
/// spawning a console subprocess (python.exe, pip.exe, the installer's helper
/// console, etc.). No-op on non-Windows targets so cross-host `cargo check`
/// stays green.
#[cfg(windows)]
pub fn no_console(cmd: &mut Command) -> &mut Command {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW)
}

#[cfg(not(windows))]
pub fn no_console(cmd: &mut Command) -> &mut Command {
    cmd
}

/// Open `path` with its default Windows shell association (e.g. .txt → Notepad).
#[cfg(windows)]
pub fn open_path(path: &std::path::Path) -> anyhow::Result<()> {
    use std::os::windows::ffi::OsStrExt;
    use windows::core::PCWSTR;
    use windows::Win32::UI::Shell::ShellExecuteW;
    use windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL;

    let path_w: Vec<u16> = path
        .as_os_str()
        .encode_wide()
        .chain(std::iter::once(0))
        .collect();
    let verb: Vec<u16> = "open\0".encode_utf16().collect();

    unsafe {
        let h = ShellExecuteW(
            None,
            PCWSTR(verb.as_ptr()),
            PCWSTR(path_w.as_ptr()),
            PCWSTR::null(),
            PCWSTR::null(),
            SW_SHOWNORMAL,
        );
        // ShellExecuteW returns an HINSTANCE-shaped handle; values <= 32 are errors.
        if (h.0 as isize) <= 32 {
            return Err(anyhow::anyhow!(
                "ShellExecuteW failed (code {}) opening {}",
                h.0 as isize,
                path.display()
            ));
        }
    }
    Ok(())
}
