//! GOTOP / ArcGIS Pro version and environment information.
//!
//! Sources (all read-only, none of them secret):
//! * the header of the newest `gotop.arcgispro.debug.<pid>.log` (searching rotated `.log.N`
//!   files too, because after rotation the header only lives in the oldest one):
//!   `Initializing Swisstopo.GoTop.AddIn.esriAddinX version 0.3.2.2 for ArcGIS 3.7.3.1904...`
//!   `... version 0.3.2.2, using .NET Runtime .NET 10.0.11`
//! * user environment variables set by the GOTOP environment-changer script
//!   (`GOTOP_*`, `TNS_ADMIN`, ...)
//! * `EnvironmentName` from `gotop.server.config.yml` (only that single key is read; the file
//!   itself is never copied because it may contain connection details)

use crate::config::{expand_env, Config};
use crate::logs;
use serde::Serialize;
use std::collections::BTreeMap;
use std::io::Read;
use std::path::{Path, PathBuf};

const VERSION_LOG_TYPE: &str = "gotop.arcgispro.debug";
/// The version header is among the first lines; no need to read more.
const HEAD_BYTES: u64 = 512 * 1024;

#[derive(Debug, Default, Serialize)]
pub struct GotopInfo {
    /// GOTOP ArcGIS Pro add-in version, e.g. `0.3.2.2`.
    pub addin_version: Option<String>,
    /// ArcGIS Pro version as reported by the add-in, e.g. `3.7.3.1904`.
    pub arcgis_pro_version: Option<String>,
    pub dotnet_runtime: Option<String>,
    /// Version parsed from the `GOTOP_SERVER_DIR` folder name (`GoTop_Server_0.3.2.2_...`).
    pub server_version: Option<String>,
    /// `EnvironmentName` from `gotop.server.config.yml`.
    pub environment_name: Option<String>,
    /// Which log file the versions were read from.
    pub version_source: Option<String>,
    /// Parent of the log directory, e.g. `Y:\Gotop` (drive differs: Y: VDI, D: office laptop, C: field laptop).
    pub gotop_root: Option<String>,
    /// `*.esriAddinX` files found in `<root>\Addins` (or `<root>`), with the version from their `Config.daml`.
    pub installed_addins: Vec<InstalledAddin>,
    /// Matching environment variables (see `env_vars` in the config).
    pub env_vars: BTreeMap<String, String>,
}

#[derive(Debug, Serialize)]
pub struct InstalledAddin {
    pub path: String,
    pub version: Option<String>,
    pub desktop_version: Option<String>,
    pub modified: Option<String>,
    pub size: u64,
}

/// `<AddInInfo id="{..}" version="0.3.2.2" desktopVersion="3.7">` -> `("0.3.2.2", Some("3.7"))`
pub fn parse_addin_info(daml: &str) -> Option<(String, Option<String>)> {
    let start = daml.find("<AddInInfo")?;
    let tag = &daml[start..start + daml[start..].find('>')?];
    let attr = |name: &str| -> Option<String> {
        let key = format!(" {name}=\"");
        let rest = &tag[tag.find(&key)? + key.len()..];
        Some(rest[..rest.find('"')?].to_string())
    };
    Some((attr("version")?, attr("desktopVersion")))
}

fn read_addin(path: &Path) -> InstalledAddin {
    let meta = std::fs::metadata(path).ok();
    let (version, desktop_version) = (|| {
        let mut zip = zip::ZipArchive::new(std::fs::File::open(path).ok()?).ok()?;
        let mut text = String::new();
        zip.by_name("Config.daml").ok()?.read_to_string(&mut text).ok()?;
        parse_addin_info(&text)
    })()
    .map_or((None, None), |(v, d)| (Some(v), d));
    InstalledAddin {
        path: path.display().to_string(),
        version,
        desktop_version,
        modified: meta
            .as_ref()
            .and_then(|m| m.modified().ok())
            .map(|t| chrono::DateTime::<chrono::Local>::from(t).format("%Y-%m-%d %H:%M:%S").to_string()),
        size: meta.map_or(0, |m| m.len()),
    }
}

/// `*.esriAddinX` files directly inside `<root>\Addins` and `<root>`.
fn find_addins(root: &Path) -> Vec<InstalledAddin> {
    let mut out = Vec::new();
    for dir in [root.join("Addins"), root.to_path_buf()] {
        let Ok(rd) = std::fs::read_dir(&dir) else { continue };
        let mut files: Vec<PathBuf> = rd
            .filter_map(|e| e.ok())
            .map(|e| e.path())
            .filter(|p| {
                p.is_file()
                    && p.extension().and_then(|x| x.to_str()).is_some_and(|x| x.eq_ignore_ascii_case("esriAddinX"))
            })
            .collect();
        files.sort();
        out.extend(files.iter().map(|p| read_addin(p)));
    }
    out
}

/// `...Initializing Swisstopo.GoTop.AddIn.esriAddinX version 0.3.2.2 for ArcGIS 3.7.3.1904...`
/// -> `("0.3.2.2", "3.7.3.1904")`
pub fn parse_init_line(line: &str) -> Option<(String, String)> {
    let rest = line.split_once("Initializing Swisstopo.GoTop.AddIn.esriAddinX version ")?.1;
    let (addin, rest) = rest.split_once(' ')?;
    let arcgis = rest.strip_prefix("for ArcGIS ")?.split_whitespace().next()?;
    Some((
        addin.trim_end_matches(',').to_string(),
        arcgis.trim_end_matches('.').to_string(),
    ))
}

/// `..., using .NET Runtime .NET 10.0.11` -> `.NET 10.0.11`
pub fn parse_runtime_line(line: &str) -> Option<String> {
    let v = line.split_once("using .NET Runtime ")?.1.trim();
    (!v.is_empty()).then(|| v.to_string())
}

/// Value of a top-level-looking `key: value` YAML line (quotes and trailing comments stripped).
pub fn yaml_value(text: &str, key: &str) -> Option<String> {
    text.lines().find_map(|line| {
        let (k, v) = line.trim().split_once(':')?;
        if !k.trim().eq_ignore_ascii_case(key) {
            return None;
        }
        let v = v.split(" #").next().unwrap_or("").trim().trim_matches(|c| c == '"' || c == '\'');
        (!v.is_empty()).then(|| v.to_string())
    })
}

/// `\\srv\...\GoTop_Server_0.3.2.2_ArcGISPro_3.7` -> `0.3.2.2`
pub fn server_version_from_dir(dir: &str) -> Option<String> {
    let last = dir.trim_end_matches(['\\', '/']).rsplit(['\\', '/']).next()?;
    last.split('_')
        .find(|t| {
            t.starts_with(|c: char| c.is_ascii_digit())
                && t.matches('.').count() >= 2
                && t.chars().all(|c| c.is_ascii_digit() || c == '.')
        })
        .map(str::to_string)
}

fn env_matches(name: &str, patterns: &[String]) -> bool {
    patterns.iter().any(|p| match p.strip_suffix('*') {
        Some(prefix) => name.to_uppercase().starts_with(&prefix.to_uppercase()),
        None => name.eq_ignore_ascii_case(p),
    })
}

fn read_head(path: &Path) -> std::io::Result<String> {
    let mut buf = Vec::new();
    std::fs::File::open(path)?.take(HEAD_BYTES).read_to_end(&mut buf)?;
    Ok(String::from_utf8_lossy(&buf).into_owned())
}

/// `base` followed by its rotated siblings, oldest first (`.log.14` ... `.log.1`, then `.log`).
fn search_order(base: &Path) -> Vec<PathBuf> {
    let mut rotated: Vec<(u32, PathBuf)> = Vec::new();
    if let (Some(dir), Some(name)) = (base.parent(), base.file_name().and_then(|n| n.to_str())) {
        let prefix = format!("{name}.");
        if let Ok(rd) = std::fs::read_dir(dir) {
            for e in rd.filter_map(|e| e.ok()) {
                let fname = e.file_name().to_string_lossy().into_owned();
                if let Some(n) = fname.strip_prefix(&prefix).and_then(|s| s.parse::<u32>().ok()) {
                    rotated.push((n, e.path()));
                }
            }
        }
    }
    rotated.sort_by_key(|(n, _)| std::cmp::Reverse(*n));
    let mut order: Vec<PathBuf> = rotated.into_iter().map(|(_, p)| p).collect();
    order.push(base.to_path_buf());
    order
}

/// The GOTOP log directory in use (first existing candidate of the first `per_type` source).
fn gotop_log_dir(cfg: &Config) -> Option<PathBuf> {
    cfg.logs
        .iter()
        .filter(|s| s.per_type)
        .find_map(|src| src.candidates().into_iter().find(|d| d.is_dir()))
}

/// Newest non-empty `gotop.arcgispro.debug` log in `dir`.
fn find_version_log(dir: &Path) -> Option<PathBuf> {
    logs::latest_per_type(dir)
        .ok()?
        .into_iter()
        .find(|t| t.log_type == VERSION_LOG_TYPE)?
        .file
}

pub fn collect(cfg: &Config) -> (GotopInfo, Vec<String>) {
    let mut info = GotopInfo::default();
    let mut warnings = Vec::new();

    let log_dir = gotop_log_dir(cfg);
    if let Some(root) = log_dir.as_deref().and_then(Path::parent) {
        info.gotop_root = Some(root.display().to_string());
        info.installed_addins = find_addins(root);
    }

    match log_dir.as_deref().and_then(find_version_log) {
        None => warnings.push("versions: no gotop.arcgispro.debug log found".into()),
        Some(base) => {
            for path in search_order(&base) {
                let Ok(head) = read_head(&path) else { continue };
                for line in head.lines() {
                    if info.addin_version.is_none() {
                        if let Some((addin, arcgis)) = parse_init_line(line) {
                            info.addin_version = Some(addin);
                            info.arcgis_pro_version = Some(arcgis);
                        }
                    }
                    if info.dotnet_runtime.is_none() {
                        info.dotnet_runtime = parse_runtime_line(line);
                    }
                }
                if info.addin_version.is_some() {
                    info.version_source = path.file_name().map(|n| n.to_string_lossy().into_owned());
                    break;
                }
            }
            if info.addin_version.is_none() {
                warnings.push(format!(
                    "versions: no add-in version header found in {} (or its rotated files)",
                    base.display()
                ));
            }
        }
    }

    for (name, value) in std::env::vars() {
        if env_matches(&name, &cfg.env_vars) {
            info.env_vars.insert(name, value);
        }
    }
    info.server_version = info
        .env_vars
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("GOTOP_SERVER_DIR"))
        .and_then(|(_, v)| server_version_from_dir(v));

    if let (Some(a), Some(sv)) = (&info.addin_version, &info.server_version) {
        if a != sv {
            warnings.push(format!("version skew: add-in {a} but GOTOP_SERVER_DIR points to server {sv}"));
        }
    }

    // Installed (on disk) vs loaded (in the log): differs if ArcGIS Pro was not restarted after an install.
    if let (Some(loaded), Some(inst)) = (
        &info.addin_version,
        info.installed_addins
            .iter()
            .find(|a| a.path.contains("Swisstopo.GoTop.AddIn."))
            .and_then(|a| a.version.clone()),
    ) {
        if *loaded != inst {
            warnings.push(format!(
                "add-in installed on disk is {inst} but the last session loaded {loaded} (ArcGIS Pro not restarted after install, or stale log)"
            ));
        }
    }

    let cfg_file = PathBuf::from(expand_env(cfg.gotop_config_dir.trim())).join("gotop.server.config.yml");
    match std::fs::read_to_string(&cfg_file) {
        Ok(text) => info.environment_name = yaml_value(&text, "EnvironmentName"),
        Err(e) => warnings.push(format!("environment: cannot read {}: {e}", cfg_file.display())),
    }

    (info, warnings)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_log_lines() {
        let init = "18.11 13:48:15.698 [2 ] DEBUG ConfigurationUtils           : Initializing Swisstopo.GoTop.AddIn.esriAddinX version 0.3.2.2 for ArcGIS 3.7.3.1904...";
        assert_eq!(parse_init_line(init), Some(("0.3.2.2".into(), "3.7.3.1904".into())));
        let rt = "18.11 13:48:15.683 [2 ] DEBUG ConfigurationUtils : Logging configured for C:\\x\\Swisstopo.GoTop.Configuration.dll version 0.3.2.2, using .NET Runtime .NET 10.0.11";
        assert_eq!(parse_runtime_line(rt).as_deref(), Some(".NET 10.0.11"));
        assert_eq!(parse_init_line("unrelated"), None);
    }

    #[test]
    fn yaml_and_server_version() {
        let y = "# comment\nfoo: 1\n  EnvironmentName: \"GCOVER Production\"  # trailing\nbar: 2\n";
        assert_eq!(yaml_value(y, "environmentname").as_deref(), Some("GCOVER Production"));
        assert_eq!(yaml_value(y, "missing"), None);
        assert_eq!(
            server_version_from_dir(r"\\v0t\prod\GoTop\Server\GoTop_Server_0.3.2.2_ArcGISPro_3.7").as_deref(),
            Some("0.3.2.2")
        );
        assert_eq!(server_version_from_dir("nothing"), None);
    }

    #[test]
    fn parses_addin_info_and_reads_zip() {
        let daml = "\u{feff}<?xml?>\n<ArcGIS>\n  <AddInInfo id=\"{8275}\" version=\"0.2.9.1\" desktopVersion=\"3.7\">\n</AddInInfo>";
        assert_eq!(parse_addin_info(daml), Some(("0.2.9.1".into(), Some("3.7".into()))));
        assert_eq!(parse_addin_info("<nothing/>"), None);

        use std::io::Write;
        let d = std::env::temp_dir().join(format!("bb_zip_{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(d.join("Addins")).unwrap();
        let f = std::fs::File::create(d.join("Addins").join("Swisstopo.GoTop.AddIn.esriAddinX")).unwrap();
        let mut w = zip::ZipWriter::new(f);
        w.start_file("Config.daml", zip::write::SimpleFileOptions::default()).unwrap();
        w.write_all(daml.as_bytes()).unwrap();
        w.finish().unwrap();
        let found = find_addins(&d);
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].version.as_deref(), Some("0.2.9.1"));
        assert_eq!(found[0].desktop_version.as_deref(), Some("3.7"));
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn env_patterns() {
        let p = vec!["GOTOP_*".to_string(), "TNS_ADMIN".to_string()];
        assert!(env_matches("GOTOP_SERVER_DIR", &p));
        assert!(env_matches("tns_admin", &p));
        assert!(!env_matches("PATH", &p));
    }

    #[test]
    fn finds_header_in_oldest_rotated_file() {
        let d = std::env::temp_dir().join(format!("bb_gi_{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        let base = d.join("gotop.arcgispro.debug.1.log");
        std::fs::write(&base, "no header here\n").unwrap();
        std::fs::write(d.join("gotop.arcgispro.debug.1.log.1"), "middle\n").unwrap();
        std::fs::write(
            d.join("gotop.arcgispro.debug.1.log.2"),
            "x Initializing Swisstopo.GoTop.AddIn.esriAddinX version 9.9.9.9 for ArcGIS 3.7.3.1904...\n",
        )
        .unwrap();
        let order = search_order(&base);
        assert!(order[0].ends_with("gotop.arcgispro.debug.1.log.2"));
        assert!(order.last().unwrap().ends_with("gotop.arcgispro.debug.1.log"));

        let mut cfg = Config::default();
        cfg.logs = vec![crate::config::LogSource {
            label: "g".into(),
            dir: d.to_string_lossy().into_owned(),
            dirs: vec![],
            extensions: vec![],
            per_type: true,
        }];
        cfg.gotop_config_dir = d.to_string_lossy().into_owned();
        let (info, _) = collect(&cfg);
        assert_eq!(info.addin_version.as_deref(), Some("9.9.9.9"));
        assert_eq!(info.version_source.as_deref(), Some("gotop.arcgispro.debug.1.log.2"));
        std::fs::remove_dir_all(&d).ok();
    }
}
