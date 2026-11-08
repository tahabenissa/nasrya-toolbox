//! #118: a throttled, best-effort check for a newer Winsploit release.
//!
//! Runs on a background thread at startup, at most once a day, and never blocks
//! or fails loudly offline. It only *informs* (a toast) — it never downloads
//! anything. Disable it with the `update-check: false` setting or by setting
//! the `WINSPLOIT_NO_UPDATE_CHECK` environment variable.

use std::path::PathBuf;
use std::process::{Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;
use std::time::{SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::detect;
use crate::runner::AppEvent;

const REPO: &str = "tahabenissa/nasrya-toolbox";
const DAY: u64 = 24 * 60 * 60;

#[derive(Deserialize)]
struct Release {
    #[serde(default)]
    tag_name: String,
}

fn state_file() -> Option<PathBuf> {
    std::env::var_os("APPDATA").map(|a| PathBuf::from(a).join("winsploit").join("update-check"))
}

fn now() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

/// Due for a check? (> a day since the last one, or never checked.)
fn due() -> bool {
    let Some(f) = state_file() else {
        return false;
    };
    match std::fs::read_to_string(&f)
        .ok()
        .and_then(|s| s.trim().parse::<u64>().ok())
    {
        Some(last) => now().saturating_sub(last) >= DAY,
        None => true,
    }
}

fn mark_checked() {
    if let Some(f) = state_file() {
        if let Some(dir) = f.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let _ = std::fs::write(&f, now().to_string());
    }
}

/// Parse "v1.2.3" / "1.2.3" into (major, minor, patch).
fn parse_ver(s: &str) -> Option<(u64, u64, u64)> {
    let s = s.trim().trim_start_matches('v');
    let mut it = s
        .split('.')
        .map(|p| p.trim_matches(|c: char| !c.is_ascii_digit()));
    let a = it.next()?.parse().ok()?;
    let b = it.next().unwrap_or("0").parse().unwrap_or(0);
    let c = it.next().unwrap_or("0").parse().unwrap_or(0);
    Some((a, b, c))
}

/// Is release `latest` newer than `current`?
pub fn is_newer(latest: &str, current: &str) -> bool {
    match (parse_ver(latest), parse_ver(current)) {
        (Some(l), Some(c)) => l > c,
        _ => false,
    }
}

/// The running build's version (compile-time).
pub fn current_version() -> &'static str {
    env!("CARGO_PKG_VERSION")
}

/// Spawn the check on a background thread. On a newer release it sends
/// `AppEvent::Update(tag)`; otherwise it sends nothing. Best-effort: any error
/// (offline, no curl, bad JSON) is swallowed silently.
pub fn check(tx: Sender<AppEvent>) {
    thread::spawn(move || {
        if std::env::var_os("WINSPLOIT_NO_UPDATE_CHECK").is_some() || !due() {
            return;
        }
        // mark now, so a flaky/offline run doesn't re-hit the network next launch
        mark_checked();
        let Some(curl) = detect::resolve("curl") else {
            return;
        };
        let url = format!("https://api.github.com/repos/{REPO}/releases/latest");
        let out = Command::new(curl)
            .args([
                "-sSL",
                "--max-time",
                "6",
                "-H",
                "Accept: application/vnd.github+json",
                "-H",
                "User-Agent: winsploit-update-check",
                &url,
            ])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .output();
        let Ok(out) = out else {
            return;
        };
        if !out.status.success() {
            return;
        }
        let body = String::from_utf8_lossy(&out.stdout);
        let Ok(rel) = serde_json::from_str::<Release>(body.trim()) else {
            return;
        };
        if !rel.tag_name.is_empty() && is_newer(&rel.tag_name, current_version()) {
            let _ = tx.send(AppEvent::Update(rel.tag_name));
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_tags_and_bare_versions() {
        assert_eq!(parse_ver("v0.3.0"), Some((0, 3, 0)));
        assert_eq!(parse_ver("1.2"), Some((1, 2, 0)));
        assert_eq!(parse_ver("v2"), Some((2, 0, 0)));
        assert_eq!(parse_ver("not-a-version"), None);
    }

    #[test]
    fn newer_compares_correctly() {
        assert!(is_newer("v0.3.0", "0.2.0"));
        assert!(is_newer("v0.2.1", "0.2.0"));
        assert!(is_newer("v1.0.0", "0.9.9"));
        assert!(!is_newer("v0.2.0", "0.2.0"));
        assert!(!is_newer("v0.1.0", "0.2.0"));
        assert!(!is_newer("garbage", "0.2.0"));
    }
}
