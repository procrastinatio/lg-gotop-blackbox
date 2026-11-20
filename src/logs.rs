use std::fs::File;
use std::io::{Read, Seek, SeekFrom};
use std::path::{Path, PathBuf};
use std::time::SystemTime;

/// Never read more than this from the end of a file (logs can be huge).
const MAX_TAIL_BYTES: u64 = 2 * 1024 * 1024;

/// Most recently modified files in `dir` whose extension is in `extensions`.
pub fn newest_files(dir: &Path, extensions: &[String], max: usize) -> std::io::Result<Vec<PathBuf>> {
    let mut files: Vec<(SystemTime, PathBuf)> = std::fs::read_dir(dir)?
        .filter_map(|e| e.ok())
        .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
        .filter(|e| {
            let p = e.path();
            let ext = p.extension().and_then(|x| x.to_str()).unwrap_or("").to_lowercase();
            extensions.iter().any(|x| x.trim_start_matches('.').eq_ignore_ascii_case(&ext))
        })
        .filter_map(|e| Some((e.metadata().ok()?.modified().ok()?, e.path())))
        .collect();
    files.sort_by_key(|f| std::cmp::Reverse(f.0));
    Ok(files.into_iter().take(max).map(|(_, p)| p).collect())
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tails_last_lines() {
        let p = std::env::temp_dir().join(format!("bb_tail_{}.log", std::process::id()));
        std::fs::write(&p, "a\nb\nc\nd\n").unwrap();
        assert_eq!(tail_lines(&p, 2).unwrap(), "c\nd");
        assert_eq!(tail_lines(&p, 99).unwrap(), "a\nb\nc\nd");
        std::fs::remove_file(&p).ok();
    }
}
