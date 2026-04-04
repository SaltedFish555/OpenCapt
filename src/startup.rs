use anyhow::{Context, Result, anyhow};
use windows::{
    Win32::{
        Foundation::{ERROR_FILE_NOT_FOUND, WIN32_ERROR},
        System::Registry::{
            HKEY, HKEY_CURRENT_USER, REG_SZ, RegCloseKey, RegCreateKeyW, RegDeleteValueW,
            RegSetValueExW,
        },
    },
    core::PCWSTR,
};

const RUN_KEY_PATH: &str = "Software\\Microsoft\\Windows\\CurrentVersion\\Run";
const RUN_VALUE_NAME: &str = "OpenCapt";

pub fn sync_launch_at_startup(enabled: bool) -> Result<()> {
    if enabled {
        enable_launch_at_startup()
    } else {
        disable_launch_at_startup()
    }
}

fn enable_launch_at_startup() -> Result<()> {
    let exe = std::env::current_exe().context("failed to resolve current executable")?;
    let command = format!("\"{}\"", exe.display());
    let key = create_run_key().context("failed to open startup registry key")?;
    let name = to_wide(RUN_VALUE_NAME);
    let value = to_wide(&command);
    let value_bytes = unsafe {
        std::slice::from_raw_parts(
            value.as_ptr() as *const u8,
            value.len() * std::mem::size_of::<u16>(),
        )
    };

    let status = unsafe {
        RegSetValueExW(
            key,
            PCWSTR(name.as_ptr()),
            Some(0),
            REG_SZ,
            Some(value_bytes),
        )
    };
    close_key(key);
    win32_ok(status).with_context(|| format!("failed to register startup command {}", command))
}

fn disable_launch_at_startup() -> Result<()> {
    let key = create_run_key().context("failed to open startup registry key")?;
    let name = to_wide(RUN_VALUE_NAME);
    let status = unsafe { RegDeleteValueW(key, PCWSTR(name.as_ptr())) };
    close_key(key);
    if status == ERROR_FILE_NOT_FOUND {
        return Ok(());
    }
    win32_ok(status).context("failed to remove startup registry value")
}

fn create_run_key() -> Result<HKEY> {
    let subkey = to_wide(RUN_KEY_PATH);
    let mut key = HKEY::default();
    let status = unsafe { RegCreateKeyW(HKEY_CURRENT_USER, PCWSTR(subkey.as_ptr()), &mut key) };
    win32_ok(status)?;
    Ok(key)
}

fn close_key(key: HKEY) {
    unsafe {
        let _ = RegCloseKey(key);
    }
}

fn to_wide(value: &str) -> Vec<u16> {
    value.encode_utf16().chain(std::iter::once(0)).collect()
}

fn win32_ok(status: WIN32_ERROR) -> Result<()> {
    if status == WIN32_ERROR(0) {
        Ok(())
    } else {
        Err(anyhow!("win32 error: 0x{:08X}", status.0))
    }
}
