//! Per-run log files: every tool run and install is written to
//! `%LOCALAPPDATA%\winsploit\logs\<UTC timestamp>-<name>.log`, so output
//! survives the 2000-line in-memory buffer and winsploit itself (evidence for a
//! report, or just "what did that scan say yesterday").

use std::fs::File;
use std::io::{LineWriter, Write};
use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

/// Where logs go: `%LOCALAPPDATA%\winsploit\logs` (or the temp dir).
pub fn log_dir() -> PathBuf {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .unwrap_or_else(std::env::temp_dir)
        .join("winsploit")
        .join("logs")
}

/// An open log for one run. Lines are flushed as they're written, so a
/// crash or kill loses nothing.
pub struct RunLog {
    path: PathBuf,
    file: LineWriter<File>,
}

impl RunLog {
    /// Create `<dir>/<timestamp>-<name>.log` and write the header.
    pub fn create(dir: &Path, name: &str, command: &str) -> std::io::Result<Self> {
        std::fs::create_dir_all(dir)?;
        let now = unix_now();
        let path = dir.join(format!("{}-{}.log", stamp_compact(now), sanitize(name)));
        let mut file = LineWriter::new(File::create(&path)?);
        writeln!(file, "# winsploit run log")?;
        writeln!(file, "# tool:    {name}")?;
        writeln!(file, "# command: {command}")?;
        writeln!(file, "# started: {}", stamp_iso(now))?;
        writeln!(file)?;
        Ok(RunLog { path, file })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    pub fn line(&mut self, line: &str) {
        let _ = writeln!(self.file, "{line}");
    }

    /// Footer with the exit status; the log is closed when dropped.
    pub fn finish(mut self, code: Option<i32>) {
        let status = code.map_or("killed / unknown".to_string(), |c| c.to_string());
        let _ = writeln!(self.file);
        let _ = writeln!(self.file, "# exit:    {status}");
        let _ = writeln!(self.file, "# ended:   {}", stamp_iso(unix_now()));
        let _ = self.file.flush();
    }
}

fn unix_now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Keep file names boring: letters, digits, `-`, `_`, `.`.
fn sanitize(name: &str) -> String {
    let s: String = name
        .chars()
        .map(|c| {
            if c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.') {
                c
            } else {
                '_'
            }
        })
        .collect();
    if s.is_empty() {
        "run".into()
    } else {
        s
    }
}

/// (year, month, day, hour, minute, second) in UTC.
fn civil(secs: u64) -> (i64, u32, u32, u32, u32, u32) {
    let days = (secs / 86_400) as i64;
    let rem = secs % 86_400;
    // Howard Hinnant's days_from_civil, inverted
    let z = days + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z.rem_euclid(146_097);
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = yoe + era * 400 + i64::from(m <= 2);
    (
        y,
        m,
        d,
        (rem / 3600) as u32,
        (rem % 3600 / 60) as u32,
        (rem % 60) as u32,
    )
}

/// `20261107-031502Z`, sortable and filename-safe.
fn stamp_compact(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(secs);
    format!("{y:04}{mo:02}{d:02}-{h:02}{mi:02}{s:02}Z")
}

/// `2026-11-07 03:15:02 UTC`
fn stamp_iso(secs: u64) -> String {
    let (y, mo, d, h, mi, s) = civil(secs);
    format!("{y:04}-{mo:02}-{d:02} {h:02}:{mi:02}:{s:02} UTC")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn civil_dates() {
        assert_eq!(civil(0), (1970, 1, 1, 0, 0, 0));
        assert_eq!(civil(951_782_400), (2000, 2, 29, 0, 0, 0)); // leap day
        assert_eq!(civil(1_798_686_302), (2026, 12, 31, 3, 5, 2));
        assert_eq!(stamp_compact(1_798_686_302), "20261231-030502Z");
        assert_eq!(stamp_iso(1_798_686_302), "2026-12-31 03:05:02 UTC");
    }

    #[test]
    fn sanitized_names() {
        assert_eq!(sanitize("nmap"), "nmap");
        assert_eq!(sanitize("seclists-lite"), "seclists-lite");
        assert_eq!(sanitize(r"..\evil/name:x"), ".._evil_name_x");
        assert_eq!(sanitize(""), "run");
    }

    #[test]
    fn writes_header_lines_and_footer() {
        let dir = std::env::temp_dir().join(format!("winsploit-log-test-{}", std::process::id()));
        let mut log = RunLog::create(&dir, "nmap", "nmap -sV x").unwrap();
        let path = log.path().to_path_buf();
        log.line("PORT STATE");
        log.line("[err] warning");
        log.finish(Some(0));
        let text = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(path
            .file_name()
            .unwrap()
            .to_string_lossy()
            .ends_with("Z-nmap.log"));
        assert!(text.contains("# command: nmap -sV x"));
        assert!(text.contains("\nPORT STATE\n[err] warning\n"));
        assert!(text.contains("# exit:    0"));
    }
}
