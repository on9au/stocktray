//! Thin wrappers over the Win32 calls stocktray needs.

use std::io;
use std::path::Path;
use std::ptr;

use windows_sys::Win32::Foundation::{ERROR_ALREADY_EXISTS, GetLastError};
use windows_sys::Win32::System::Threading::CreateMutexW;
use windows_sys::Win32::UI::HiDpi::{GetDpiForSystem, GetDpiForWindow, GetSystemMetricsForDpi};
use windows_sys::Win32::UI::Shell::ShellExecuteW;
use windows_sys::Win32::UI::WindowsAndMessaging::{
    FindWindowW, MB_ICONINFORMATION, MB_OK, MessageBoxW, SM_CXSMICON, SW_SHOWNORMAL,
};
use winreg::RegKey;
use winreg::enums::HKEY_CURRENT_USER;

const RUN_KEY: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const RUN_VALUE: &str = "stocktray";

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(Some(0)).collect()
}

/// False if another stocktray is already running in this session.
pub fn claim_single_instance() -> bool {
    let name = wide(r"Local\stocktray");
    unsafe {
        // Deliberately never closed: the mutex has to live as long as the process.
        let handle = CreateMutexW(ptr::null(), 0, name.as_ptr());
        !handle.is_null() && GetLastError() != ERROR_ALREADY_EXISTS
    }
}

/// Pixel size of a tray icon at the taskbar's current DPI.
pub fn tray_icon_size() -> u32 {
    let class = wide("Shell_TrayWnd");
    unsafe {
        let taskbar = FindWindowW(class.as_ptr(), ptr::null());
        let dpi = if taskbar.is_null() { 0 } else { GetDpiForWindow(taskbar) };
        let dpi = if dpi == 0 { GetDpiForSystem() } else { dpi };
        GetSystemMetricsForDpi(SM_CXSMICON, dpi).clamp(16, 256) as u32
    }
}

pub fn open_url(url: &str) {
    let (verb, url) = (wide("open"), wide(url));
    unsafe {
        ShellExecuteW(ptr::null_mut(), verb.as_ptr(), url.as_ptr(), ptr::null(), ptr::null(), SW_SHOWNORMAL);
    }
}

pub fn edit_file(path: &Path) -> io::Result<()> {
    std::process::Command::new("notepad.exe").arg(path).spawn().map(drop)
}

pub fn message_box(text: &str) {
    let (text, caption) = (wide(text), wide("stocktray"));
    unsafe {
        MessageBoxW(ptr::null_mut(), text.as_ptr(), caption.as_ptr(), MB_OK | MB_ICONINFORMATION);
    }
}

/// Starts an installer built by `package.ps1` without its wizard. It replaces this exe (which must
/// exit straight after) and starts the new one.
pub fn run_installer(path: &Path) -> io::Result<()> {
    std::process::Command::new(path)
        .args(["/SILENT", "/SUPPRESSMSGBOXES", "/NORESTART", "/CLOSEAPPLICATIONS"])
        .spawn()
        .map(drop)
}

fn run_command() -> io::Result<String> {
    Ok(format!("\"{}\"", std::env::current_exe()?.display()))
}

/// Whether the Run key starts this exe. An entry for an exe elsewhere doesn't count, so ticking
/// the menu item after moving the exe repoints it.
pub fn autostart_enabled() -> bool {
    let Ok(command) = run_command() else {
        return false;
    };
    RegKey::predef(HKEY_CURRENT_USER)
        .open_subkey(RUN_KEY)
        .and_then(|key| key.get_value::<String, _>(RUN_VALUE))
        .is_ok_and(|value| value.eq_ignore_ascii_case(&command))
}

pub fn set_autostart(enabled: bool) -> io::Result<()> {
    let (key, _) = RegKey::predef(HKEY_CURRENT_USER).create_subkey(RUN_KEY)?;
    if enabled {
        key.set_value(RUN_VALUE, &run_command()?)
    } else {
        match key.delete_value(RUN_VALUE) {
            Err(e) if e.kind() == io::ErrorKind::NotFound => Ok(()),
            result => result,
        }
    }
}
