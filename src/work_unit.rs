//! The GOTOP work unit (WU) the user is working on, taken from today's logs.
//!
//! Log lines seen in real sessions (the full id is `<product>_<release cycle>_<name>`):
//! * arcgispro: `Initialized Work Unit saved in project: Tessin_Marc`
//! * arcgispro: `APRX already exists for work unit 'Tessin_Marc': Y:\GoTop\Projects\...\toto.aprx`
//! * navigator: `Opening work unit GC_2030-12-31_Tessin_Marc` / `Work unit opened: Tessin_Marc`
//! * editing: `Detected and packed work unit Tessin_Marc`
//! * editing: `The work unit has an existing daily version (U80795753.DV_GC_2030-12-31_Tessin_Marc).`
//!
//! The last occurrence in time wins (files are read oldest first, rotated files included).

use crate::config::Config;
use crate::gotop_info::search_order;
use crate::logs;
use serde::Serialize;
use std::path::Path;

/// Read at most this much from the end of each log file.
const MAX_READ_BYTES: u64 = 4 * 1024 * 1024;
const WU_LOG_TYPES: [&str; 3] =
    ["gotop.arcgispro.debug", "gotop.navigator.debug", "gotop.microservice.editing.debug"];

#[derive(Debug, Default, Serialize)]
pub struct WorkUnitInfo {
    /// Name as shown in GOTOP, e.g. `Tessin_Marc`.
    pub name: Option<String>,
    /// `<product>_<release cycle>_<name>`, e.g. `GC_2030-12-31_Tessin_Marc`.
    pub full_id: Option<String>,
    /// e.g. `U80795753.DV_GC_2030-12-31_Tessin_Marc`.
    pub daily_version: Option<String>,
    /// The work unit's ArcGIS Pro project file.
    pub aprx: Option<String>,
    /// Which log the name came from.
    pub source: Option<String>,
    /// Names of work units opened, oldest first, de-duplicated.
    pub opened: Vec<String>,
}

/// `GC_2030-12-31_Tessin_Marc` -> `Some("Tessin_Marc")`; a short name (`Tessin_Marc`) -> `None`.
fn name_of_full_id(id: &str) -> Option<&str> {
    let mut parts = id.splitn(3, '_');
    let (_product, date, name) = (parts.next()?, parts.next()?, parts.next()?);
    let is_date = date.len() == 10 && date.bytes().all(|b| b.is_ascii_digit() || b == b'-');
    (is_date && !name.is_empty()).then_some(name)
}

fn after<'a>(line: &'a str, marker: &str) -> Option<&'a str> {
    Some(line.split_once(marker)?.1)
}

/// First whitespace-delimited word, without surrounding quotes/brackets/commas.
fn word(s: &str) -> Option<String> {
    let w = s.split_whitespace().next()?.trim_matches(|c| matches!(c, '\'' | '"' | '(' | ')' | ',' | ';'));
    (!w.is_empty()).then(|| w.to_string())
}

pub fn parse_project_wu(line: &str) -> Option<String> {
    word(after(line, "Initialized Work Unit saved in project: ")?)
}
pub fn parse_navigator_opened(line: &str) -> Option<String> {
    word(after(line, "Work unit opened: ")?)
}
pub fn parse_opening_full(line: &str) -> Option<String> {
    word(after(line, "Opening work unit ")?)
}
pub fn parse_packed(line: &str) -> Option<String> {
    word(after(line, "Detected and packed work unit ")?)
}
/// `... existing daily version (U1.DV_X)` or `... Daily version created (X)`; the first has the user prefix.
pub fn parse_daily_version(line: &str) -> Option<(String, bool)> {
    for (marker, existing) in [("existing daily version (", true), ("Daily version created (", false)] {
        if let Some(rest) = after(line, marker) {
            let v = rest.split(')').next()?.trim();
            if !v.is_empty() {
                return Some((v.to_string(), existing));
            }
        }
    }
    None
}
/// `APRX already exists for work unit 'Tessin_Marc': Y:\...\toto.aprx` -> path
pub fn parse_aprx(line: &str) -> Option<String> {
    let rest = after(line, "APRX already exists for work unit '")?;
    let path = rest.split_once("': ")?.1.trim();
    (!path.is_empty()).then(|| path.to_string())
}

#[derive(Default)]
struct Scan {
    pro: Option<String>,
    navigator: Option<String>,
    packed: Option<String>,
    full_id: Option<String>,
    dv_existing: Option<String>,
    dv_created: Option<String>,
    aprx: Option<String>,
    opened: Vec<String>,
}

impl Scan {
    fn line(&mut self, line: &str) {
        if let Some(n) = parse_project_wu(line) {
            self.pro = Some(n);
        } else if let Some(n) = parse_navigator_opened(line) {
            self.navigator = Some(n);
        } else if let Some(n) = parse_packed(line) {
            self.packed = Some(n);
        } else if let Some(f) = parse_opening_full(line) {
            // Some logs print the full id, others only the short name: de-duplicate by name.
            let name = name_of_full_id(&f).unwrap_or(&f).to_string();
            if !self.opened.contains(&name) {
                self.opened.push(name);
            }
            if name_of_full_id(&f).is_some() {
                self.full_id = Some(f);
            }
        } else if let Some((v, existing)) = parse_daily_version(line) {
            if existing {
                self.dv_existing = Some(v);
            } else {
                self.dv_created = Some(v);
            }
        } else if let Some(p) = parse_aprx(line) {
            self.aprx = Some(p);
        }
    }

    fn finish(self) -> WorkUnitInfo {
        let (name, source) = match (self.pro, self.navigator, self.packed) {
            (Some(n), _, _) => (Some(n), Some("arcgispro log (project)")),
            (None, Some(n), _) => (Some(n), Some("navigator log")),
            (None, None, Some(n)) => (Some(n), Some("editing microservice log")),
            _ => (None, None),
        };
        WorkUnitInfo {
            name,
            full_id: self.full_id,
            daily_version: self.dv_existing.or(self.dv_created),
            aprx: self.aprx,
            source: source.map(str::to_string),
            opened: self.opened,
        }
    }
}

/// Scan the newest ArcGIS Pro / Navigator / editing logs in `dir` (only today's if configured).
pub fn collect(cfg: &Config, dir: &Path) -> WorkUnitInfo {
    let types = logs::latest_per_type(dir).unwrap_or_default();
    let since = cfg.logs_today_only.then(logs::local_midnight);
    let mut scan = Scan::default();
    for ty in WU_LOG_TYPES {
        let Some(t) = types.iter().find(|t| t.log_type == ty) else { continue };
        let (Some(file), Some(modified)) = (&t.file, t.modified) else { continue };
        if since.is_some_and(|s| modified < s) {
            continue; // an older session: not "the work unit being used"
        }
        for path in search_order(file) {
            if let Ok(text) = logs::read_tail(&path, MAX_READ_BYTES) {
                text.lines().for_each(|l| scan.line(l));
            }
        }
    }
    scan.finish()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_real_log_lines() {
        assert_eq!(
            parse_project_wu("20.11 17:27:43.683 [2 ] INFO  GoTopContext : Initialized Work Unit saved in project: Tessin_Marc").as_deref(),
            Some("Tessin_Marc")
        );
        assert_eq!(parse_navigator_opened("INFO  NavigatorAppContext : Work unit opened: Tessin_Marc").as_deref(), Some("Tessin_Marc"));
        assert_eq!(
            parse_opening_full("INFO  NavigatorAppContext : Opening work unit GC_2030-12-31_Tessin_Marc").as_deref(),
            Some("GC_2030-12-31_Tessin_Marc")
        );
        assert_eq!(parse_packed("DEBUG EditingDdxGrpcImpl : Detected and packed work unit Tessin_Marc").as_deref(), Some("Tessin_Marc"));
        assert_eq!(
            parse_daily_version("INFO  DailyVersionManagementService :   The work unit has an existing daily version (U80795753.DV_GC_2030-12-31_Tessin_Marc)."),
            Some(("U80795753.DV_GC_2030-12-31_Tessin_Marc".into(), true))
        );
        assert_eq!(
            parse_daily_version("INFO  WorkUnit :   Daily version created (GC_2030-12-31_Tessin_Marc)"),
            Some(("GC_2030-12-31_Tessin_Marc".into(), false))
        );
        assert_eq!(
            parse_aprx(r"INFO  AprxManager :   APRX already exists for work unit 'Tessin_Marc': Y:\GoTop\Projects\Production\GC\2030-12-31\Tessin_Marc\APRX\toto.aprx").as_deref(),
            Some(r"Y:\GoTop\Projects\Production\GC\2030-12-31\Tessin_Marc\APRX\toto.aprx")
        );
        assert_eq!(parse_project_wu("unrelated line"), None);
    }

    #[test]
    fn last_work_unit_wins_and_priority_applies() {
        let mut s = Scan::default();
        for l in [
            "INFO NavigatorAppContext : Opening work unit GC_2030-12-31_Mesocco",
            "INFO EditingX : Opening work unit Mesocco",
            "INFO NavigatorAppContext : Work unit opened: Mesocco",
            "INFO NavigatorAppContext : Opening work unit GC_2030-12-31_Tessin_Marc",
            "INFO NavigatorAppContext : Work unit opened: Tessin_Marc",
            "INFO NavigatorAppContext : Opening work unit GC_2030-12-31_Mesocco",
            "INFO WorkUnit : Daily version created (GC_2030-12-31_Tessin_Marc)",
        ] {
            s.line(l);
        }
        let w = s.finish();
        assert_eq!(w.name.as_deref(), Some("Tessin_Marc"));
        assert_eq!(w.source.as_deref(), Some("navigator log"));
        assert_eq!(w.full_id.as_deref(), Some("GC_2030-12-31_Mesocco")); // last "Opening" line
        assert_eq!(w.opened, vec!["Mesocco", "Tessin_Marc"]);
        assert_eq!(w.daily_version.as_deref(), Some("GC_2030-12-31_Tessin_Marc"));
    }

    #[test]
    fn scans_files_in_a_log_dir() {
        let d = std::env::temp_dir().join(format!("bb_wu_{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        std::fs::write(d.join("gotop.navigator.debug.1.log"), "x Opening work unit GC_2030-12-31_Tessin_Marc\n").unwrap();
        std::fs::write(
            d.join("gotop.arcgispro.debug.2.log"),
            "x Initialized Work Unit saved in project: Tessin_Marc\n",
        )
        .unwrap();
        let w = collect(&Config::default(), &d);
        assert_eq!(w.name.as_deref(), Some("Tessin_Marc"));
        assert_eq!(w.source.as_deref(), Some("arcgispro log (project)"));
        assert_eq!(w.full_id.as_deref(), Some("GC_2030-12-31_Tessin_Marc"));
        std::fs::remove_dir_all(&d).ok();
    }
}
