//! Tool detection: is a tool's executable actually runnable from PATH?
//!
//! This resolves names in-process, as Windows itself would (PATH x PATHEXT),
//! instead of spawning `where.exe` once per tool (~40 ms each, which froze
//! the UI as the catalog grew). Each PATH directory is listed once per pass
//! into a name set, so checking 40 tools costs ~40 directory listings, not
//! thousands of stat calls.
//!
//! Two deliberate differences from `where`:
//! - the current directory is never searched, so a stray `nmap.exe` in the
//!   folder you started winsploit from can't pose as the real tool;
//! - only extensions we can actually launch count (`.exe .com .bat .cmd`), so
//!   Scoop's extensionless sh shims and `.ps1` shims don't produce a false
//!   "installed".

use std::collections::{BTreeMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Mutex;

use serde::{Deserialize, Serialize};

pub use crate::registry::Verify;

/// Extensions `std::process::Command` can launch, in Windows' default order.
const RUNNABLE: &[&str] = &[".com", ".exe", ".bat", ".cmd"];

/// A snapshot of what's on PATH: each directory with its (lowercased) file
/// names, plus the launchable extensions in PATHEXT order.
pub struct PathIndex {
    dirs: Vec<(PathBuf, HashSet<String>)>,
    exts: Vec<String>,
}

impl PathIndex {
    /// Snapshot the current process's PATH / PATHEXT.
    pub fn from_env() -> Self {
        let path = std::env::var("PATH").unwrap_or_default();
        let pathext = std::env::var("PATHEXT").unwrap_or_default();
        Self::build(&path, &pathext, list_dir)
    }

    /// Build from explicit values; `list` returns a directory's file names.
    fn build(
        path_var: &str,
        pathext_var: &str,
        list: impl Fn(&Path) -> Option<Vec<String>>,
    ) -> Self {
        // PATHEXT order wins (that's how Windows picks between x.exe and
        // x.cmd), restricted to what we can launch.
        let mut exts: Vec<String> = pathext_var
            .split(';')
            .map(|e| e.trim().to_ascii_lowercase())
            .filter(|e| RUNNABLE.contains(&e.as_str()))
            .collect();
        if exts.is_empty() {
            exts = RUNNABLE.iter().map(|e| e.to_string()).collect();
        }

        let mut seen = HashSet::new();
        let mut dirs = Vec::new();
        for d in path_var.split(';') {
            let d = d.trim().trim_matches('"');
            if d.is_empty() || !seen.insert(d.to_ascii_lowercase()) {
                continue;
            }
            let dir = PathBuf::from(d);
            if let Some(names) = list(&dir) {
                let names = names.into_iter().map(|n| n.to_ascii_lowercase()).collect();
                dirs.push((dir, names));
            }
        }
        PathIndex { dirs, exts }
    }

    /// Full path of `exe` as it would be launched, or `None`.
    pub fn resolve(&self, exe: &str) -> Option<PathBuf> {
        let given = Path::new(exe);
        if given.components().count() > 1 {
            // An explicit path: no PATH search.
            return given.is_file().then(|| given.to_path_buf());
        }
        let lower = exe.to_ascii_lowercase();
        let has_runnable_ext = RUNNABLE.iter().any(|e| lower.ends_with(e));

        for (dir, names) in &self.dirs {
            if has_runnable_ext {
                if names.contains(&lower) {
                    return Some(dir.join(exe));
                }
                continue;
            }
            for ext in &self.exts {
                let file = format!("{lower}{ext}");
                if names.contains(&file) {
                    return Some(dir.join(format!("{exe}{ext}")));
                }
            }
        }
        None
    }
}

/// File names in `dir`, or `None` if it can't be read.
fn list_dir(dir: &Path) -> Option<Vec<String>> {
    let rd = std::fs::read_dir(dir).ok()?;
    Some(
        rd.filter_map(|e| e.ok())
            .filter(|e| e.file_type().map(|t| !t.is_dir()).unwrap_or(false))
            .filter_map(|e| e.file_name().into_string().ok())
            .collect(),
    )
}

/// Resolve one name against the current PATH (builds a fresh snapshot).
pub fn resolve(exe: &str) -> Option<PathBuf> {
    PathIndex::from_env().resolve(exe)
}

/// What detection found for one tool.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub enum Found {
    Missing,
    Installed(PathBuf),
    /// Something with the right name is on PATH, but its `verify` check
    /// says it's a different program (e.g. Python's httpx instead of
    /// ProjectDiscovery's).
    Conflict(PathBuf),
}

/// Cap on concurrent `verify` subprocesses, so a catalog with many verified
/// tools doesn't spawn dozens of processes at once (#108).
const MAX_VERIFY_PARALLEL: usize = 8;

/// Probe a batch of tools with one PATH snapshot, running `verify` checks
/// where the catalog asks for them.
pub fn probe_all(tools: &[(String, Option<Verify>)]) -> Vec<Found> {
    probe_with(&PathIndex::from_env(), tools)
}

/// Same as `probe_all`, with an explicit PATH index (so tests can inject one).
/// PATH resolution is in-memory and cheap, so it's done up front; only the
/// `verify` checks spawn a subprocess, and those run concurrently (#108).
fn probe_with(idx: &PathIndex, tools: &[(String, Option<Verify>)]) -> Vec<Found> {
    let mut found: Vec<Found> = Vec::with_capacity(tools.len());
    let mut jobs: Vec<(usize, PathBuf, Verify)> = Vec::new();
    for (i, (exe, verify)) in tools.iter().enumerate() {
        match idx.resolve(exe) {
            None => found.push(Found::Missing),
            Some(path) => {
                if let Some(v) = verify {
                    jobs.push((i, path.clone(), v.clone()));
                }
                found.push(Found::Installed(path));
            }
        }
    }
    // Downgrade Installed -> Conflict wherever the (parallel) verify check fails.
    for (i, matched) in run_verifications(&jobs) {
        if !matched {
            if let Found::Installed(p) = &found[i] {
                found[i] = Found::Conflict(p.clone());
            }
        }
    }
    found
}

/// Run the `verify` checks on a bounded thread pool. Returns `(tool index,
/// matched)` for each job; order doesn't matter since results carry their index.
fn run_verifications(jobs: &[(usize, PathBuf, Verify)]) -> Vec<(usize, bool)> {
    if jobs.is_empty() {
        return Vec::new();
    }
    let workers = jobs.len().min(MAX_VERIFY_PARALLEL);
    let next = AtomicUsize::new(0);
    let out: Mutex<Vec<(usize, bool)>> = Mutex::new(Vec::with_capacity(jobs.len()));
    std::thread::scope(|s| {
        for _ in 0..workers {
            s.spawn(|| loop {
                let k = next.fetch_add(1, Ordering::Relaxed);
                let Some((i, path, v)) = jobs.get(k) else {
                    break;
                };
                let matched = verify_matches(path, v);
                out.lock().unwrap().push((*i, matched));
            });
        }
    });
    out.into_inner().unwrap()
}

// ---- detection cache (#106): remember the last probe so the dashboard shows
// install status instantly, then reconciles after the background probe. --------

/// How long cached results are trusted before a cold probe is required.
const CACHE_MAX_AGE_SECS: u64 = 14 * 24 * 60 * 60;

#[derive(Default, Serialize, Deserialize)]
struct DetectCache {
    /// Fingerprint of PATH + PATHEXT; a change invalidates the whole cache.
    path_fp: u64,
    /// When it was written (unix seconds).
    saved: u64,
    /// exe name -> last result.
    tools: BTreeMap<String, Found>,
}

fn now_secs() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

fn path_fingerprint() -> u64 {
    use std::hash::{Hash, Hasher};
    let mut h = std::collections::hash_map::DefaultHasher::new();
    std::env::var("PATH").unwrap_or_default().hash(&mut h);
    std::env::var("PATHEXT").unwrap_or_default().hash(&mut h);
    h.finish()
}

fn cache_file() -> Option<PathBuf> {
    std::env::var_os("APPDATA")
        .map(|a| PathBuf::from(a).join("winsploit").join("detect-cache.json"))
}

/// Pick cached results for `exes` out of `cache`, honouring the PATH
/// fingerprint and age. Returns one entry per exe (`None` = not usable).
fn select_from_cache(
    cache: &DetectCache,
    exes: &[String],
    current_fp: u64,
    now: u64,
) -> Vec<Option<Found>> {
    if cache.path_fp != current_fp || now.saturating_sub(cache.saved) > CACHE_MAX_AGE_SECS {
        return vec![None; exes.len()];
    }
    exes.iter().map(|e| cache.tools.get(e).cloned()).collect()
}

/// Load the last probe's results for `exes`, if the cache matches the current
/// PATH and is fresh enough. One entry per exe; `None` means "re-probe".
pub fn load_cache(exes: &[String]) -> Vec<Option<Found>> {
    let blank = || vec![None; exes.len()];
    let Some(f) = cache_file() else {
        return blank();
    };
    let Ok(text) = std::fs::read_to_string(&f) else {
        return blank();
    };
    match serde_json::from_str::<DetectCache>(&text) {
        Ok(cache) => select_from_cache(&cache, exes, path_fingerprint(), now_secs()),
        Err(_) => blank(),
    }
}

/// Write a fresh probe to the cache (best-effort; errors are ignored).
pub fn save_cache(exes: &[String], found: &[Found]) {
    let Some(f) = cache_file() else {
        return;
    };
    let cache = DetectCache {
        path_fp: path_fingerprint(),
        saved: now_secs(),
        tools: exes.iter().cloned().zip(found.iter().cloned()).collect(),
    };
    if let Some(dir) = f.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(json) = serde_json::to_string(&cache) {
        let _ = std::fs::write(&f, json);
    }
}

/// Run `path <verify.args>` (3 s timeout) and check that its combined
/// output contains `verify.contains`, case-insensitively.
fn verify_matches(path: &Path, v: &Verify) -> bool {
    use std::io::Read;
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let Ok(args) = crate::args::split(&v.args) else {
        return false;
    };
    let Ok(mut child) = Command::new(path)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
    else {
        return false;
    };
    // Read both pipes on threads so a chatty tool can't block on a full pipe.
    let mut out = child.stdout.take();
    let mut err = child.stderr.take();
    let t_out = std::thread::spawn(move || {
        let mut s = Vec::new();
        if let Some(o) = out.as_mut() {
            let _ = o.read_to_end(&mut s);
        }
        s
    });
    let t_err = std::thread::spawn(move || {
        let mut s = Vec::new();
        if let Some(e) = err.as_mut() {
            let _ = e.read_to_end(&mut s);
        }
        s
    });
    let deadline = Instant::now() + Duration::from_secs(3);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                // Kill the whole tree: for a .cmd shim, killing only cmd.exe
                // would leave the real program holding our pipes open, and
                // the reader threads below would never finish.
                let _ = Command::new("taskkill")
                    .args(["/T", "/F", "/PID", &child.id().to_string()])
                    .stdout(Stdio::null())
                    .stderr(Stdio::null())
                    .status();
                let _ = child.kill();
                let _ = child.wait();
                break;
            }
        }
    }
    let mut text = t_out.join().unwrap_or_default();
    text.extend(t_err.join().unwrap_or_default());
    String::from_utf8_lossy(&text)
        .to_lowercase()
        .contains(&v.contains.to_lowercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    const PATHEXT: &str = ".COM;.EXE;.BAT;.CMD;.VBS;.JS;.PS1";

    /// A fake filesystem: dir -> file names.
    fn idx(path_var: &str, pathext: &str, files: &[(&str, &[&str])]) -> PathIndex {
        let map: HashMap<String, Vec<String>> = files
            .iter()
            .map(|(d, f)| {
                (
                    d.to_ascii_lowercase(),
                    f.iter().map(|s| s.to_string()).collect(),
                )
            })
            .collect();
        PathIndex::build(path_var, pathext, move |p| {
            map.get(&p.to_string_lossy().to_ascii_lowercase()).cloned()
        })
    }

    #[test]
    fn finds_exe_on_path() {
        let i = idx(r"C:\x;C:\a", PATHEXT, &[(r"C:\a", &["nmap.exe"])]);
        assert_eq!(i.resolve("nmap"), Some(PathBuf::from(r"C:\a\nmap.exe")));
    }

    #[test]
    fn finds_cmd_shims_and_skips_sh_and_ps1_shims() {
        // Scoop puts all three next to each other for a .bat-based app.
        let i = idx(
            r"C:\s",
            PATHEXT,
            &[(
                r"C:\s",
                &["jadx", "jadx.ps1", "jadx.cmd", "only", "only.ps1"],
            )],
        );
        assert_eq!(i.resolve("jadx"), Some(PathBuf::from(r"C:\s\jadx.cmd")));
        // a lone sh/ps1 shim does not count as installed
        assert_eq!(i.resolve("only"), None);
    }

    #[test]
    fn earlier_path_dir_wins_then_pathext_order() {
        let i = idx(
            r"C:\a;C:\b",
            PATHEXT,
            &[(r"C:\a", &["t.cmd", "t.exe"]), (r"C:\b", &["t.exe"])],
        );
        assert_eq!(i.resolve("t"), Some(PathBuf::from(r"C:\a\t.exe")));
    }

    #[test]
    fn case_insensitive() {
        let i = idx(r"C:\a", PATHEXT, &[(r"C:\a", &["NMap.EXE"])]);
        assert!(i.resolve("nmap").is_some());
        assert!(i.resolve("NMAP").is_some());
    }

    #[test]
    fn current_directory_is_not_searched() {
        // Nothing is searched implicitly: an empty PATH finds nothing, even
        // though the fake cwd has nmap.exe.
        let i = idx("", PATHEXT, &[(".", &["nmap.exe"])]);
        assert_eq!(i.resolve("nmap"), None);
    }

    #[test]
    fn explicit_runnable_extension() {
        let i = idx(r"C:\a", PATHEXT, &[(r"C:\a", &["x.exe", "y.ps1"])]);
        assert_eq!(i.resolve("x.exe"), Some(PathBuf::from(r"C:\a\x.exe")));
        assert_eq!(i.resolve("y.ps1"), None);
    }

    #[test]
    fn quoted_duplicate_and_empty_path_entries() {
        let i = idx(
            r#";;"C:\Program Files\T";C:\PROGRAM FILES\T;"#,
            PATHEXT,
            &[(r"C:\Program Files\T", &["t.exe"])],
        );
        assert_eq!(
            i.resolve("t"),
            Some(PathBuf::from(r"C:\Program Files\T\t.exe"))
        );
        assert_eq!(i.dirs.len(), 1, "duplicate PATH entry listed once");
    }

    #[test]
    fn missing_pathext_falls_back_to_defaults() {
        let i = idx(r"C:\a", "", &[(r"C:\a", &["t.bat"])]);
        assert_eq!(i.resolve("t"), Some(PathBuf::from(r"C:\a\t.bat")));
    }

    #[test]
    fn unreadable_dirs_are_skipped() {
        let i = idx(r"C:\gone;C:\a", PATHEXT, &[(r"C:\a", &["t.exe"])]);
        assert_eq!(i.resolve("t"), Some(PathBuf::from(r"C:\a\t.exe")));
    }

    #[test]
    fn probe_resolves_installed_and_missing() {
        let i = idx(r"C:\a", PATHEXT, &[(r"C:\a", &["nmap.exe"])]);
        let tools = [("nmap".to_string(), None), ("ghost".to_string(), None)];
        let found = probe_with(&i, &tools);
        assert_eq!(found[0], Found::Installed(PathBuf::from(r"C:\a\nmap.exe")));
        assert_eq!(found[1], Found::Missing);
    }

    #[test]
    fn cache_selects_only_when_fingerprint_and_age_match() {
        let mut tools = BTreeMap::new();
        tools.insert("nmap".into(), Found::Installed(PathBuf::from(r"C:\a\nmap.exe")));
        tools.insert("sqlmap".into(), Found::Missing);
        let cache = DetectCache { path_fp: 42, saved: 1_000, tools };
        let exes = vec!["nmap".to_string(), "sqlmap".to_string(), "new".to_string()];

        // matching fingerprint, fresh: known tools come back, unknown is None
        let got = select_from_cache(&cache, &exes, 42, 1_500);
        assert_eq!(got[0], Some(Found::Installed(PathBuf::from(r"C:\a\nmap.exe"))));
        assert_eq!(got[1], Some(Found::Missing));
        assert_eq!(got[2], None);

        // PATH changed -> whole cache ignored
        assert!(select_from_cache(&cache, &exes, 99, 1_500).iter().all(Option::is_none));
        // too old -> ignored
        let stale = 1_000 + CACHE_MAX_AGE_SECS + 1;
        assert!(select_from_cache(&cache, &exes, 42, stale).iter().all(Option::is_none));
    }

    #[test]
    fn cache_round_trips_through_json() {
        let mut tools = BTreeMap::new();
        tools.insert("x".into(), Found::Conflict(PathBuf::from(r"C:\b\x.exe")));
        let cache = DetectCache { path_fp: 7, saved: 123, tools };
        let json = serde_json::to_string(&cache).unwrap();
        let back: DetectCache = serde_json::from_str(&json).unwrap();
        assert_eq!(back.path_fp, 7);
        assert_eq!(back.tools.get("x"), Some(&Found::Conflict(PathBuf::from(r"C:\b\x.exe"))));
    }
}
