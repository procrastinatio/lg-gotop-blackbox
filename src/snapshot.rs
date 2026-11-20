use crate::config::Config;
use crate::screenshot;
use serde::Serialize;
use sysinfo::System;

#[derive(Debug, Serialize)]
pub struct ProcInfo {
    pub pid: u32,
    pub name: String,
    pub rss_mb: u64,
    pub virtual_mb: u64,
}

/// Machine/user context plus a memory sample. The sample is taken at launch and refreshed
/// when the bundle is saved (`refresh`), because the user usually starts GOTOP/ArcGIS Pro
/// *after* launching this tool or keeps working while it is open.
#[derive(Debug, Serialize)]
pub struct Snapshot {
    /// When the report was made (refreshed on save); used for the bundle folder name.
    pub timestamp: String,
    /// When the tool was launched.
    pub started_at: String,
    /// When the screenshot was taken, if any.
    pub screenshot_taken_at: Option<String>,
    pub tool_version: &'static str,
    pub host: String,
    /// `vdi`, `laptop` or `other` (from hostname prefix, see config).
    pub machine_kind: &'static str,
    pub user: String,
    pub os: String,
    pub total_mem_mb: u64,
    pub used_mem_mb: u64,
    pub processes: Vec<ProcInfo>,
    pub screenshot_error: Option<String>,
    #[serde(skip)]
    pub screenshot_png: Option<Vec<u8>>,
}

const MB: u64 = 1024 * 1024;

/// (total RAM MB, used RAM MB, matching processes sorted by memory).
fn sample(cfg: &Config) -> (u64, u64, Vec<ProcInfo>) {
    let mut sys = System::new();
    sys.refresh_memory();
    sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);

    let own_pid = sysinfo::get_current_pid().ok();
    let needles: Vec<String> = cfg.processes.iter().map(|p| p.to_lowercase()).collect();
    let mut processes: Vec<ProcInfo> = sys
        .processes()
        .values()
        .filter(|p| Some(p.pid()) != own_pid) // our own exe name contains "gotop"
        .filter_map(|p| {
            let name = p.name().to_string_lossy().into_owned();
            let lower = name.to_lowercase();
            needles.iter().any(|n| lower.contains(n)).then(|| ProcInfo {
                pid: p.pid().as_u32(),
                name,
                rss_mb: p.memory() / MB,
                virtual_mb: p.virtual_memory() / MB,
            })
        })
        .collect();
    processes.sort_by_key(|p| std::cmp::Reverse(p.rss_mb));
    (sys.total_memory() / MB, sys.used_memory() / MB, processes)
}

impl Snapshot {
    pub fn capture(cfg: &Config, with_screenshot: bool) -> Self {
        // Screenshot first: nothing of ours should be on screen yet.
        let (screenshot_png, screenshot_error) = if with_screenshot {
            match screenshot::capture_png() {
                Ok(png) => (Some(png), None),
                Err(e) => (None, Some(format!("{e:#}"))),
            }
        } else {
            (None, None)
        };

        let now = chrono::Local::now().to_rfc3339();
        let (total_mem_mb, used_mem_mb, processes) = sample(cfg);
        let host = System::host_name()
            .or_else(|| std::env::var("COMPUTERNAME").ok())
            .unwrap_or_else(|| "unknown-host".into());

        Snapshot {
            machine_kind: crate::config::classify_host(cfg, &host),
            host,
            started_at: now.clone(),
            screenshot_taken_at: screenshot_png.is_some().then(|| now.clone()),
            timestamp: now,
            tool_version: env!("CARGO_PKG_VERSION"),
            user: std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_else(|_| "unknown-user".into()),
            os: System::long_os_version().unwrap_or_else(|| "unknown-os".into()),
            total_mem_mb,
            used_mem_mb,
            processes,
            screenshot_error,
            screenshot_png,
        }
    }

    #[cfg_attr(not(windows), allow(dead_code))] // only the Windows GUI refreshes before saving
    /// Re-sample memory/processes and stamp the report time (call right before saving a bundle).
    pub fn refresh(&mut self, cfg: &Config) {
        let (total, used, processes) = sample(cfg);
        self.total_mem_mb = total;
        self.used_mem_mb = used;
        self.processes = processes;
        self.timestamp = chrono::Local::now().to_rfc3339();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn refresh_updates_report_time_but_not_launch_time() {
        let cfg = Config::default();
        let mut snap = Snapshot::capture(&cfg, false);
        let (started, first) = (snap.started_at.clone(), snap.timestamp.clone());
        std::thread::sleep(std::time::Duration::from_millis(20));
        snap.refresh(&cfg);
        assert_eq!(snap.started_at, started);
        assert_ne!(snap.timestamp, first);
    }
}
