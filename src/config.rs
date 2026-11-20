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
    /// Only events from the last N hours.
    pub event_log_hours: u32,
    pub logs: Vec<LogSource>,
    /// Directory holding `gotop.server.config.yml` (only its `EnvironmentName` key is read).
    pub gotop_config_dir: String,
    /// Environment variables recorded in the report; a trailing `*` is a prefix wildcard.
    pub env_vars: Vec<String>,
    /// Hostname prefixes (case-insensitive) used to classify the machine in report.json.
    pub vdi_prefixes: Vec<String>,
    pub laptop_prefixes: Vec<String>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct LogSource {
    pub label: String,
    /// A directory, and/or `dirs` for several candidates (VDI and laptops keep logs in different
    /// places). Candidates are tried in order; the first that exists is used.
    #[serde(default)]
    pub dir: String,
    #[serde(default)]
    pub dirs: Vec<String>,
    #[serde(default = "default_extensions")]
    pub extensions: Vec<String>,
    /// GOTOP naming (`<type>.<pid>.log[.n]`): take the newest non-empty file of *each* type
    /// instead of the N newest files overall. `extensions` is ignored in this mode.
    #[serde(default)]
    pub per_type: bool,
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
            event_log_hours: 72,
            gotop_config_dir: r"%LOCALAPPDATA%\swisstopo\GoTop\Config".into(),
            env_vars: vec!["GOTOP_*".into(), "TNS_ADMIN".into()],
            vdi_prefixes: vec!["cvt".into()],
            laptop_prefixes: vec!["cm".into()],
            logs: vec![
                LogSource {
                    label: "gotop".into(),
                    dir: String::new(),
                    dirs: vec![r"Y:\Gotop\Logs".into(), r"D:\Gotop\Logs".into(), r"C:\Gotop\Logs".into()],
                    extensions: default_extensions(),
                    per_type: true,
                },
                LogSource {
                    label: "esri-error-reports".into(),
                    dir: r"%LOCALAPPDATA%\ESRI\ErrorReports".into(),
                    dirs: vec![],
                    extensions: default_extensions(),
                    per_type: false,
                },
            ],
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

impl LogSource {
    /// All candidate directories, env-expanded, in order.
    pub fn candidates(&self) -> Vec<PathBuf> {
        std::iter::once(&self.dir)
            .chain(self.dirs.iter())
            .filter(|d| !d.trim().is_empty())
            .map(|d| PathBuf::from(expand_env(d.trim())))
            .collect()
    }
}

/// `vdi`, `laptop` or `other`, from the hostname prefix.
pub fn classify_host(cfg: &Config, host: &str) -> &'static str {
    let h = host.to_lowercase();
    let matches = |prefixes: &[String]| prefixes.iter().any(|p| !p.is_empty() && h.starts_with(&p.to_lowercase()));
    if matches(&cfg.vdi_prefixes) {
        "vdi"
    } else if matches(&cfg.laptop_prefixes) {
        "laptop"
    } else {
        "other"
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
    fn classifies_hosts() {
        let c = Config::default();
        assert_eq!(classify_host(&c, "CVT03236"), "vdi");
        assert_eq!(classify_host(&c, "CM123456"), "laptop");
        assert_eq!(classify_host(&c, "zbook"), "other");
    }

    #[test]
    fn candidates_combine_dir_and_dirs() {
        let s = LogSource {
            label: "x".into(),
            dir: "a".into(),
            dirs: vec!["b".into(), " ".into()],
            extensions: vec![],
            per_type: false,
        };
        assert_eq!(s.candidates(), vec![PathBuf::from("a"), PathBuf::from("b")]);
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
