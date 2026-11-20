use anyhow::{Context, Result};
use serde::Deserialize;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Deserialize)]
#[serde(default)]
pub struct Config {
    pub share_path: String,
    pub fallback_dir: String,
    pub tail_lines: usize,
    pub max_files_per_source: usize,
    pub processes: Vec<String>,
    pub collect_event_log: bool,
    pub event_log_count: u32,
    pub logs: Vec<LogSource>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogSource {
    pub label: String,
    pub dir: String,
    #[serde(default = "default_extensions")]
    pub extensions: Vec<String>,
}

fn default_extensions() -> Vec<String> {
    vec!["log".into(), "txt".into()]
}

impl Default for Config {
    fn default() -> Self {
        Self {
            share_path: String::new(),
            fallback_dir: String::new(),
            tail_lines: 200,
            max_files_per_source: 3,
            processes: vec!["ArcGISPro".into(), "GOTOP".into(), "ArcGISIndexing".into()],
            collect_event_log: true,
            event_log_count: 50,
            logs: vec![LogSource {
                label: "esri-error-reports".into(),
                dir: r"%LOCALAPPDATA%\ESRI\ErrorReports".into(),
                extensions: default_extensions(),
            }],
        }
    }
}

impl Config {
    /// Explicit path > `gotop-blackbox.toml` next to the executable > built-in defaults.
    pub fn load(explicit: Option<&Path>) -> Result<Self> {
        let path = match explicit {
            Some(p) => Some(p.to_path_buf()),
            None => std::env::current_exe()
                .ok()
                .and_then(|e| e.parent().map(|d| d.join("gotop-blackbox.toml")))
                .filter(|p| p.is_file()),
        };
        match path {
            Some(p) => {
                let text = std::fs::read_to_string(&p)
                    .with_context(|| format!("reading config {}", p.display()))?;
                toml::from_str(&text).with_context(|| format!("parsing config {}", p.display()))
            }
            None => Ok(Self::default()),
        }
    }

    /// Directory used when the share is unset or unreachable.
    pub fn fallback_path(&self) -> PathBuf {
        if self.fallback_dir.trim().is_empty() {
            std::env::temp_dir().join("gotop-blackbox")
        } else {
            PathBuf::from(expand_env(&self.fallback_dir))
        }
    }
}

/// Expand `%VAR%` references. Unknown variables are left untouched.
pub fn expand_env(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) => {
                let name = &after[..end];
                match (!name.is_empty()).then(|| std::env::var(name).ok()).flatten() {
                    Some(val) => out.push_str(&val),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_known_and_keeps_unknown() {
        std::env::set_var("BB_TEST_VAR", "abc");
        assert_eq!(expand_env(r"x\%BB_TEST_VAR%\y"), r"x\abc\y");
        assert_eq!(expand_env("%BB_NOPE_XYZ%"), "%BB_NOPE_XYZ%");
        assert_eq!(expand_env("100%"), "100%");
    }

    #[test]
    fn parses_partial_config() {
        let c: Config = toml::from_str("tail_lines = 5\n[[logs]]\nlabel='a'\ndir='b'").unwrap();
        assert_eq!(c.tail_lines, 5);
        assert_eq!(c.logs.len(), 1);
        assert_eq!(c.logs[0].extensions, vec!["log", "txt"]);
        assert_eq!(c.max_files_per_source, 3);
    }
}
