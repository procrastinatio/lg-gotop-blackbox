use anyhow::{bail, Result};

/// Recent Error/Warning entries from the Windows Application log, via the built-in `wevtutil`.
#[cfg(windows)]
pub fn recent_application_events(count: u32) -> Result<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    let out = std::process::Command::new("wevtutil")
        .args([
            "qe",
            "Application",
            &format!("/c:{count}"),
            "/rd:true",
            "/f:text",
            "/q:*[System[(Level=1 or Level=2 or Level=3)]]",
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    if !out.status.success() {
        bail!("wevtutil failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(not(windows))]
pub fn recent_application_events(_count: u32) -> Result<String> {
    bail!("Windows event log is only available on Windows")
}
