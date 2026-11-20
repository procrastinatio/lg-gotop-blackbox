use crate::config::{expand_env, Config, LogSource};
use crate::gotop_info::{self, GotopInfo};
use crate::{events, logs, snapshot::Snapshot};
use anyhow::{bail, Context, Result};
use serde::Serialize;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct BundleResult {
    pub path: PathBuf,
    /// True when the configured share could not be used and the local fallback was used instead.
    pub used_fallback: bool,
    pub warnings: Vec<String>,
}

#[derive(Serialize)]
struct Report<'a> {
    message: &'a str,
    #[serde(flatten)]
    snapshot: &'a Snapshot,
    gotop: &'a GotopInfo,
    warnings: &'a [String],
}

/// `2026-11-20_1130_HOST_user`, sanitised for use as a directory name.
pub fn folder_name(snap: &Snapshot) -> String {
    let ts = chrono::DateTime::parse_from_rfc3339(&snap.timestamp)
        .map(|t| t.format("%Y-%m-%d_%H%M").to_string())
        .unwrap_or_else(|_| "unknown-time".into());
    sanitize(&format!("{ts}_{}_{}", snap.host, snap.user))
}

fn sanitize(s: &str) -> String {
    s.chars()
        .map(|c| if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') { c } else { '_' })
        .collect()
}

/// Create a unique bundle directory under `root`.
fn create_unique_dir(root: &Path, name: &str) -> Result<PathBuf> {
    std::fs::create_dir_all(root).with_context(|| format!("creating {}", root.display()))?;
    for i in 0..100 {
        let candidate = if i == 0 { root.join(name) } else { root.join(format!("{name}_{}", i + 1)) };
        match std::fs::create_dir(&candidate) {
            Ok(()) => return Ok(candidate),
            Err(e) if e.kind() == std::io::ErrorKind::AlreadyExists => continue,
            Err(e) => return Err(e).with_context(|| format!("creating {}", candidate.display())),
        }
    }
    bail!("could not find a free bundle name under {}", root.display())
}

/// Write the support bundle. Prefers the network share; falls back to a local directory.
pub fn write(cfg: &Config, snap: &Snapshot, message: &str, include_screenshot: bool) -> Result<BundleResult> {
    let name = folder_name(snap);
    let mut warnings = Vec::new();

    let share = expand_env(cfg.share_path.trim());
    let (dir, used_fallback) = if share.is_empty() {
        warnings.push("share_path is not configured; bundle saved locally".into());
        (create_unique_dir(&cfg.fallback_path(), &name)?, true)
    } else {
        match create_unique_dir(Path::new(&share), &name) {
            Ok(d) => (d, false),
            Err(e) => {
                warnings.push(format!("share unavailable ({e:#}); bundle saved locally"));
                (create_unique_dir(&cfg.fallback_path(), &name)?, true)
            }
        }
    };

    std::fs::write(dir.join("description.txt"), message)?;

    if include_screenshot {
        match &snap.screenshot_png {
            Some(png) => std::fs::write(dir.join("screenshot.png"), png)?,
            None => warnings.push(format!(
                "no screenshot: {}",
                snap.screenshot_error.as_deref().unwrap_or("not captured")
            )),
        }
    }

    for src in &cfg.logs {
        if let Err(e) = collect_log_source(cfg, src, &dir, &mut warnings) {
            warnings.push(format!("log source '{}': {e:#}", src.label));
        }
    }

    if cfg.collect_event_log {
        match events::recent_application_events(cfg.event_log_count) {
            Ok(text) => std::fs::write(dir.join("events-application.txt"), text)?,
            Err(e) => warnings.push(format!("event log: {e:#}")),
        }
    }

    let (gotop, gotop_warnings) = gotop_info::collect(cfg);
    warnings.extend(gotop_warnings);

    std::fs::write(dir.join("summary.txt"), summary(snap, &gotop, message, &warnings))?;
    let report = Report { message, snapshot: snap, gotop: &gotop, warnings: &warnings };
    std::fs::write(dir.join("report.json"), serde_json::to_vec_pretty(&report)?)?;

    Ok(BundleResult { path: dir, used_fallback, warnings })
}

/// One-glance overview for whoever opens the bundle first.
fn summary(snap: &Snapshot, g: &GotopInfo, message: &str, warnings: &[String]) -> String {
    let or_unknown = |o: &Option<String>| o.clone().unwrap_or_else(|| "unknown".into());
    let ddx = g.env_vars.iter().find(|(k, _)| k.eq_ignore_ascii_case("GOTOP_DDX_ENVIRONMENT")).map(|(_, v)| v.as_str());
    let mut s = String::new();
    s.push_str(&format!("gotop-blackbox {}   {}\n", snap.tool_version, snap.timestamp));
    s.push_str(&format!("Host: {} ({})   User: {}\n", snap.host, snap.machine_kind, snap.user));
    s.push_str(&format!("OS: {}   RAM used: {} / {} MB\n", snap.os, snap.used_mem_mb, snap.total_mem_mb));
    s.push_str(&format!(
        "GOTOP add-in: {}   ArcGIS Pro: {}   {}\n",
        or_unknown(&g.addin_version),
        or_unknown(&g.arcgis_pro_version),
        or_unknown(&g.dotnet_runtime)
    ));
    s.push_str(&format!("GOTOP server: {}\n", or_unknown(&g.server_version)));
    s.push_str(&format!(
        "Environment: {}   (GOTOP_DDX_ENVIRONMENT: {})\n",
        or_unknown(&g.environment_name),
        ddx.unwrap_or("unset")
    ));
    for p in snap.processes.iter().take(5) {
        s.push_str(&format!("  {} (pid {}): {} MB RSS\n", p.name, p.pid, p.rss_mb));
    }
    s.push_str(&format!("\nUser message:\n{message}\n"));
    if !warnings.is_empty() {
        s.push_str("\nWarnings:\n");
        for w in warnings {
            s.push_str(&format!("- {w}\n"));
        }
    }
    s
}

fn collect_log_source(cfg: &Config, src: &LogSource, bundle: &Path, warnings: &mut Vec<String>) -> Result<()> {
    let candidates = src.candidates();
    let Some(src_dir) = candidates.iter().find(|d| d.is_dir()).cloned() else {
        warnings.push(format!(
            "log source '{}': none of these directories exist: {}",
            src.label,
            candidates.iter().map(|d| d.display().to_string()).collect::<Vec<_>>().join(", ")
        ));
        return Ok(());
    };
    let out_dir = bundle.join("logs").join(sanitize(&src.label));

    if src.per_type {
        let types = match logs::latest_per_type(&src_dir) {
            Ok(t) if t.is_empty() => {
                warnings.push(format!("log source '{}': no GOTOP logs in {}", src.label, src_dir.display()));
                return Ok(());
            }
            Ok(t) => t,
            Err(e) => {
                warnings.push(format!("log source '{}' ({}): {e}", src.label, src_dir.display()));
                return Ok(());
            }
        };
        std::fs::create_dir_all(&out_dir)?;
        // index.txt records every type seen, including ones whose files are all empty
        // ("the error log is empty" is itself useful evidence).
        let mut index = String::from("type | chosen file | modified | size (bytes) | files of this type\n");
        for t in &types {
            match &t.file {
                Some(f) => {
                    let fname = f.file_name().and_then(|n| n.to_str()).unwrap_or("file");
                    let modified = t
                        .modified
                        .map(|m| chrono::DateTime::<chrono::Local>::from(m).format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_default();
                    index.push_str(&format!("{} | {fname} | {modified} | {} | {}\n", t.log_type, t.size, t.candidates));
                    match logs::tail_with_rotation(f, t.previous.as_deref(), cfg.tail_lines) {
                        Ok(text) => std::fs::write(out_dir.join(format!("{}.tail.txt", sanitize(fname))), text)?,
                        Err(e) => warnings.push(format!("cannot read {}: {e}", f.display())),
                    }
                }
                None => index.push_str(&format!("{} | (all empty) | | 0 | {}\n", t.log_type, t.candidates)),
            }
        }
        std::fs::write(out_dir.join("index.txt"), index)?;
        return Ok(());
    }

    match logs::newest_files(&src_dir, &src.extensions, cfg.max_files_per_source) {
        Ok(files) if files.is_empty() => {
            warnings.push(format!("log source '{}': no matching files in {}", src.label, src_dir.display()))
        }
        Ok(files) => {
            std::fs::create_dir_all(&out_dir)?;
            for f in files {
                let fname = f.file_name().and_then(|n| n.to_str()).unwrap_or("file");
                match logs::tail_lines(&f, cfg.tail_lines) {
                    Ok(text) => std::fs::write(out_dir.join(format!("{}.tail.txt", sanitize(fname))), text)?,
                    Err(e) => warnings.push(format!("cannot read {}: {e}", f.display())),
                }
            }
        }
        Err(e) => warnings.push(format!("log source '{}' ({}): {e}", src.label, src_dir.display())),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn writes_bundle_with_fallback() {
        let root = std::env::temp_dir().join(format!("bb_test_{}", std::process::id()));
        let cfg = Config {
            share_path: String::new(),
            fallback_dir: root.to_string_lossy().into_owned(),
            logs: vec![],
            collect_event_log: false,
            ..Config::default()
        };
        let snap = Snapshot::capture(&cfg, false);
        let res = write(&cfg, &snap, "it broke", true).unwrap();
        assert!(res.used_fallback);
        assert_eq!(std::fs::read_to_string(res.path.join("description.txt")).unwrap(), "it broke");
        assert!(res.path.join("report.json").is_file());
        // Second bundle in the same minute must not collide.
        let res2 = write(&cfg, &snap, "again", false).unwrap();
        assert_ne!(res.path, res2.path);
        std::fs::remove_dir_all(&root).ok();
    }
}
