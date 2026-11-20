//! Diagnostics for a GUI-subsystem exe that otherwise fails silently.
//!
//! * `log()` appends breadcrumbs to `%TEMP%\gotop-blackbox.log`. If that file never appears,
//!   the process did not reach `main()` (blocked by AppLocker/SRP/antivirus/SmartScreen).
//! * `install_panic_hook()` records panics there and shows a dialog (release builds abort on panic).
//! * `notify()` shows a message box on Windows release builds (no console), stdout otherwise.

use std::io::Write;
use std::path::PathBuf;

const MAX_LOG_BYTES: u64 = 200 * 1024;

pub fn log_path() -> PathBuf {
    std::env::temp_dir().join("gotop-blackbox.log")
}

/// Append a timestamped line to the diagnostics log. Never fails, never panics.
pub fn log(msg: &str) {
    let path = log_path();
    if std::fs::metadata(&path).map(|m| m.len() > MAX_LOG_BYTES).unwrap_or(false) {
        let _ = std::fs::remove_file(&path);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(path) {
        let ts = chrono::Local::now().format("%Y-%m-%d %H:%M:%S%.3f");
        let _ = writeln!(f, "{ts} [pid {}] {msg}", std::process::id());
    }
}

pub fn install_panic_hook() {
    std::panic::set_hook(Box::new(|info| {
        let text = format!("panic: {info}");
        log(&text);
        notify(
            "gotop-blackbox crashed",
            &format!("{text}\n\nDetails: {}", log_path().display()),
        );
    }));
}

#[cfg(all(windows, not(debug_assertions)))]
pub fn notify(title: &str, text: &str) {
    use windows::core::PCWSTR;
    use windows::Win32::UI::WindowsAndMessaging::{MessageBoxW, MB_ICONINFORMATION, MB_OK};

    let wide = |s: &str| -> Vec<u16> { s.encode_utf16().chain(std::iter::once(0)).collect() };
    let (t, m) = (wide(title), wide(text));
    unsafe {
        MessageBoxW(None, PCWSTR(m.as_ptr()), PCWSTR(t.as_ptr()), MB_OK | MB_ICONINFORMATION);
    }
}

#[cfg(not(all(windows, not(debug_assertions))))]
pub fn notify(title: &str, text: &str) {
    println!("[{title}] {text}");
}
