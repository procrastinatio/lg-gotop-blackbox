use crate::config::{expand_env, Config};
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
        let src_dir = PathBuf::from(expand_env(&src.dir));
        let out_dir = dir.join("logs").join(sanitize(&src.label));
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
    }

    if cfg.collect_event_log {
        match events::recent_application_events(cfg.event_log_count) {
            Ok(text) => std::fs::write(dir.join("events-application.txt"), text)?,
            Err(e) => warnings.push(format!("event log: {e:#}")),
        }
    }

    let report = Report { message, snapshot: snap, warnings: &warnings };
    std::fs::write(dir.join("report.json"), serde_json::to_vec_pretty(&report)?)?;

    Ok(BundleResult { path: dir, used_fallback, warnings })
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
