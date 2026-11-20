use anyhow::{bail, Result};

/// Recent Error/crash entries from the Windows Application log, via the built-in `wevtutil`.
#[cfg(windows)]
pub fn recent_application_events(count: u32, hours: u32) -> Result<String> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;

    // Errors/criticals plus crash/hang sources (some are level 4), within the time window.
    // Plain warnings are left out: they are dominated by unrelated noise (e.g. Perflib).
    let query = format!(
        "*[System[(Level=1 or Level=2 or Provider[@Name='Application Hang'] or \
         Provider[@Name='Windows Error Reporting'] or Provider[@Name='.NET Runtime']) \
         and TimeCreated[timediff(@SystemTime) <= {}]]]",
        u64::from(hours) * 3_600_000
    );
    let out = std::process::Command::new("wevtutil")
        .args([
            "qe",
            "Application",
            &format!("/c:{count}"),
            "/rd:true",
            "/f:text",
            &format!("/q:{query}"),
        ])
        .creation_flags(CREATE_NO_WINDOW)
        .output()?;
    if !out.status.success() {
        bail!("wevtutil failed: {}", String::from_utf8_lossy(&out.stderr).trim());
    }
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

#[cfg(not(windows))]
pub fn recent_application_events(_count: u32, _hours: u32) -> Result<String> {
    bail!("Windows event log is only available on Windows")
}
