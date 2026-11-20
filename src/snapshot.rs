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

/// State captured at the instant the tool is launched (before any window appears),
/// so the screenshot and memory figures show what the user was looking at.
#[derive(Debug, Serialize)]
pub struct Snapshot {
    pub timestamp: String,
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

        let host = System::host_name()
            .or_else(|| std::env::var("COMPUTERNAME").ok())
            .unwrap_or_else(|| "unknown-host".into());

        Snapshot {
            machine_kind: crate::config::classify_host(cfg, &host),
            host,
            timestamp: chrono::Local::now().to_rfc3339(),
            tool_version: env!("CARGO_PKG_VERSION"),
            user: std::env::var("USERNAME")
                .or_else(|_| std::env::var("USER"))
                .unwrap_or_else(|_| "unknown-user".into()),
            os: System::long_os_version().unwrap_or_else(|| "unknown-os".into()),
            total_mem_mb: sys.total_memory() / MB,
            used_mem_mb: sys.used_memory() / MB,
            processes,
            screenshot_error,
            screenshot_png,
        }
    }
}
