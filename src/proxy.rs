use anyhow::{Context, Result};

#[cfg(windows)]
use winreg::enums::*;
#[cfg(windows)]
use winreg::RegKey;

const INTERNET_SETTINGS: &str =
    r"Software\Microsoft\Windows\CurrentVersion\Internet Settings";

pub fn enable(host_port: &str) -> Result<()> {
    #[cfg(windows)]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu
            .create_subkey(INTERNET_SETTINGS)
            .context("opening Internet Settings registry key")?;
        key.set_value("ProxyEnable", &1u32)
            .context("setting ProxyEnable")?;
        key.set_value("ProxyServer", &host_port.to_string())
            .context("setting ProxyServer")?;
        // Bypass localhost-style hosts; matches Windows default plus our needs.
        key.set_value(
            "ProxyOverride",
            &"<local>;localhost;127.*;10.*;172.16.*;172.17.*;172.18.*;172.19.*;172.20.*;172.21.*;172.22.*;172.23.*;172.24.*;172.25.*;172.26.*;172.27.*;172.28.*;172.29.*;172.30.*;172.31.*;192.168.*"
                .to_string(),
        )
        .context("setting ProxyOverride")?;
        broadcast_settings_change();
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        let _ = host_port;
        anyhow::bail!("proxy::enable is only supported on Windows");
    }
}

pub fn disable() -> Result<()> {
    #[cfg(windows)]
    {
        let hkcu = RegKey::predef(HKEY_CURRENT_USER);
        let (key, _) = hkcu
            .create_subkey(INTERNET_SETTINGS)
            .context("opening Internet Settings registry key")?;
        key.set_value("ProxyEnable", &0u32)
            .context("clearing ProxyEnable")?;
        // Leave ProxyServer/ProxyOverride alone — disabling is enough and
        // some users may want to re-enable a different proxy later.
        broadcast_settings_change();
        return Ok(());
    }
    #[cfg(not(windows))]
    {
        anyhow::bail!("proxy::disable is only supported on Windows");
    }
}

#[cfg(windows)]
fn broadcast_settings_change() {
    use windows::Win32::Foundation::{HWND, LPARAM, WPARAM};
    use windows::Win32::UI::WindowsAndMessaging::{
        SendMessageTimeoutW, HWND_BROADCAST, SMTO_ABORTIFHUNG, WM_SETTINGCHANGE,
    };

    // WinINet wants two notifications: settings-change + a refresh. The
    // canonical sequence is INTERNET_OPTION_SETTINGS_CHANGED (39) and
    // INTERNET_OPTION_REFRESH (37) via InternetSetOptionW, but for our scope
    // (browsers/system proxy clients re-reading the registry) WM_SETTINGCHANGE
    // is enough on modern Windows.
    unsafe {
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(0),
            SMTO_ABORTIFHUNG,
            5000,
            None,
        );
        // Also poke an empty string lParam variant some apps listen for.
        let internet_str: Vec<u16> = "Internet\0".encode_utf16().collect();
        let _ = SendMessageTimeoutW(
            HWND_BROADCAST,
            WM_SETTINGCHANGE,
            WPARAM(0),
            LPARAM(internet_str.as_ptr() as isize),
            SMTO_ABORTIFHUNG,
            5000,
            None,
        );
        let _ = HWND::default();
    }
}
