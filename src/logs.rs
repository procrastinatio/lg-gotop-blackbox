use std::collections::BTreeMap;
use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Never read more than this from the end of a file (logs can be huge).
const MAX_TAIL_BYTES: u64 = 2 * 1024 * 1024;

/// Local midnight of the current day (start of "today").
pub fn local_midnight() -> SystemTime {
    use chrono::TimeZone;
    let now = chrono::Local::now();
    now.date_naive()
        .and_hms_opt(0, 0, 0)
        .and_then(|n| chrono::Local.from_local_datetime(&n).earliest())
        .map(SystemTime::from)
        .unwrap_or(SystemTime::UNIX_EPOCH)
}

/// Most recently modified files in `dir` whose extension is in `extensions`
/// (and, if `since` is given, modified at or after it).
pub fn newest_files(
    dir: &Path,
    extensions: &[String],
    max: usize,
    since: Option<SystemTime>,
) -> std::io::Result<Vec<PathBuf>> {
    let mut files: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter(|e| {
            let p = e.path();
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
            extensions.iter().any(|x| x.trim_start_matches('.').eq_ignore_ascii_case(&ext))
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .filter(|(m, _)| since.is_none_or(|s| *m >= s))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    Ok(files.into_iter().take(max).map(|(_, p)| p).collect())
}

/// Split a GOTOP-style log name `<type>.<pid>.log[.<n>]` into `(type, rotation)`.
///
/// `gotop.navigator.debug.7716.log`   -> ("gotop.navigator.debug", 0)  (current file)
/// `gotop.navigator.debug.7716.log.3` -> ("gotop.navigator.debug", 3)  (rotated, higher = older)
pub fn parse_name(name: &str) -> Option<(&str, u32)> {
    let is_digits = |s: &str| !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit());
    let (stem_log, rotation) = match name.rsplit_once('.') {
        Some((head, last)) if is_digits(last) && head.ends_with(".log") => (head, last.parse().ok()?),
        _ => (name, 0),
    };
    let stem = stem_log.strip_suffix(".log")?;
    let (log_type, pid) = stem.rsplit_once('.')?;
    is_digits(pid).then_some((log_type, rotation))
}

/// The newest log of one type (one process instance).
#[derive(Debug)]
pub struct TypeLatest {
    pub log_type: String,
    /// Newest non-empty current file, if any.
    pub file: Option<PathBuf>,
    pub modified: Option<SystemTime>,
    pub size: u64,
    /// `<file>.1`, the most recent rotated file, if present.
    pub previous: Option<PathBuf>,
    /// How many current files of this type exist (empty ones included).
    pub candidates: usize,
}

/// For every log type found in `dir`, pick the newest non-empty current (non-rotated) file.
/// Types whose files are all empty are still listed (with `file: None`): "no errors logged" is evidence.
pub fn latest_per_type(dir: &Path) -> std::io::Result<Vec<TypeLatest>> {
    let mut by_type: BTreeMap<String, TypeLatest> = BTreeMap::new();
    for entry in std::fs::read_dir(dir)?.filter_map(|e| e.ok()) {
        let Ok(meta) = entry.metadata() else { continue };
        if !meta.is_file() {
            continue;
        }
        let name = entry.file_name().to_string_lossy().into_owned();
        let Some((log_type, 0)) = parse_name(&name) else { continue };
        let slot = by_type.entry(log_type.to_string()).or_insert_with(|| TypeLatest {
            log_type: log_type.to_string(),
            file: None,
            modified: None,
            size: 0,
            previous: None,
            candidates: 0,
        });
        slot.candidates += 1;
        let Ok(modified) = meta.modified() else { continue };
        if meta.len() > 0 && slot.modified.is_none_or(|m| modified > m) {
            slot.file = Some(entry.path());
            slot.modified = Some(modified);
            slot.size = meta.len();
        }
    }
    let mut out: Vec<TypeLatest> = by_type.into_values().collect();
    for t in &mut out {
        if let Some(f) = &t.file {
            let mut prev = f.clone().into_os_string();
            prev.push(".1");
            let prev = PathBuf::from(prev);
            t.previous = prev.is_file().then_some(prev);
        }
    }
    Ok(out)
}

/// Last `n` lines of a file, decoded lossily as UTF-8.
pub fn tail_lines(path: &Path, n: usize) -> std::io::Result<String> {
    let mut f = File::open(path)?;
    let len = f.metadata()?.len();
    let start = len.saturating_sub(MAX_TAIL_BYTES);
    f.seek(SeekFrom::Start(start))?;
    let mut buf = Vec::new();
    f.read_to_end(&mut buf)?;
    let text = String::from_utf8_lossy(&buf);
    let lines: Vec<&str> = text.lines().collect();
    // If we started mid-file the first line is probably partial; it falls outside `n` anyway
    // unless the file is tiny, in which case start == 0 and nothing is cut.
    let from = lines.len().saturating_sub(n);
    Ok(lines[from..].join("\n"))
}

/// Like [`tail_lines`], but if the file has fewer than `n` lines (e.g. it was just rotated),
/// the missing lines are taken from the end of `previous`.
pub fn tail_with_rotation(path: &Path, previous: Option<&Path>, n: usize) -> std::io::Result<String> {
    let text = tail_lines(path, n)?;
    let have = text.lines().count();
    if have >= n {
        return Ok(text);
    }
    match previous.and_then(|p| tail_lines(p, n - have).ok()) {
        Some(older) if !older.is_empty() => {
            Ok(format!("{older}\n----- rotation boundary -----\n{text}"))
        }
        _ => Ok(text),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tmp(name: &str) -> PathBuf {
        let d = std::env::temp_dir().join(format!("bb_{name}_{}", std::process::id()));
        std::fs::remove_dir_all(&d).ok();
        std::fs::create_dir_all(&d).unwrap();
        d
    }

    #[test]
    fn tails_last_lines() {
        let d = tmp("tail");
        let p = d.join("a.log");
        std::fs::write(&p, "a\nb\nc\nd\n").unwrap();
        assert_eq!(tail_lines(&p, 2).unwrap(), "c\nd");
        assert_eq!(tail_lines(&p, 99).unwrap(), "a\nb\nc\nd");
        std::fs::remove_dir_all(&d).ok();
    }

    #[test]
    fn local_midnight_is_within_the_last_day() {
        let m = local_midnight();
        let age = SystemTime::now().duration_since(m).unwrap();
        assert!(age < std::time::Duration::from_secs(25 * 3600), "{age:?}");
    }

    #[test]
    fn parses_gotop_names() {
        assert_eq!(parse_name("gotop.navigator.debug.7716.log"), Some(("gotop.navigator.debug", 0)));
        assert_eq!(parse_name("gotop.arcgispro.debug.12480.log.14"), Some(("gotop.arcgispro.debug", 14)));
        assert_eq!(
            parse_name("gotop.microservice.qa.error.7236.log"),
            Some(("gotop.microservice.qa.error", 0))
        );
        assert_eq!(parse_name("notes.txt"), None);
        assert_eq!(parse_name("gotop.navigator.debug.log"), None);
        assert_eq!(parse_name("gotop.x.abc.log"), None);
    }

    #[test]
    fn picks_newest_nonempty_per_type_and_rotation() {
        let d = tmp("pertype");
        std::fs::write(d.join("gotop.nav.debug.100.log"), "old\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(d.join("gotop.nav.debug.200.log"), "new1\nnew2\n").unwrap();
        std::fs::write(d.join("gotop.nav.debug.200.log.1"), "r1\nr2\nr3\n").unwrap();
        std::thread::sleep(std::time::Duration::from_millis(30));
        std::fs::write(d.join("gotop.nav.debug.300.log"), "").unwrap(); // newest but empty
        std::fs::write(d.join("gotop.nav.error.300.log"), "").unwrap(); // only empty files
        std::fs::write(d.join("unrelated.log"), "x").unwrap();

        let got = latest_per_type(&d).unwrap();
        assert_eq!(got.len(), 2);
        let debug = &got[0];
        assert_eq!(debug.log_type, "gotop.nav.debug");
        assert_eq!(debug.candidates, 3);
        assert!(debug.file.as_ref().unwrap().ends_with("gotop.nav.debug.200.log"));
        assert!(debug.previous.is_some());
        assert!(got[1].file.is_none());

        let text = tail_with_rotation(debug.file.as_ref().unwrap(), debug.previous.as_deref(), 4).unwrap();
        assert_eq!(text, "r2\nr3\n----- rotation boundary -----\nnew1\nnew2");
        std::fs::remove_dir_all(&d).ok();
    }
}
