//! Picking up PATH changes made while winsploit is running.
//!
//! A process gets a copy of PATH when it starts. When a Scoop install adds a
//! directory to the *user* PATH (`env_add_path`, e.g. openssl), or Scoop
//! itself gets installed mid-session, winsploit's copy doesn't change, so the
//! tool keeps showing as missing until winsploit restarts. `refresh()` re-reads
//! the persistent PATH from the registry (as Explorer does on
//! WM_SETTINGCHANGE) and appends any new directories to our own PATH.

use std::process::{Command, Stdio};

const USER_KEY: &str = r"HKCU\Environment";
const MACHINE_KEY: &str = r"HKLM\SYSTEM\CurrentControlSet\Control\Session Manager\Environment";

/// Merge the registry PATH into this process's PATH. Returns the
/// directories that were newly added (empty if nothing changed).
pub fn refresh() -> Vec<String> {
    let mut fresh = String::new();
    for key in [MACHINE_KEY, USER_KEY] {
        if let Some(v) = reg_path(key) {
            fresh.push_str(&expand(&v, |k| std::env::var(k).ok()));
            fresh.push(';');
        }
    }
    let current = std::env::var("PATH").unwrap_or_default();
    let (merged, added) = merge(&current, &fresh);
    if !added.is_empty() {
        // Edition 2021: set_var is safe; std serialises env access.
        std::env::set_var("PATH", merged);
    }
    added
}

/// `reg query <key> /v Path`, returning the raw (unexpanded) value.
fn reg_path(key: &str) -> Option<String> {
    let out = Command::new("reg")
        .args(["query", key, "/v", "Path"])
        .stderr(Stdio::null())
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    parse_reg_value(&String::from_utf8_lossy(&out.stdout))
}

/// Pull the value out of `reg query` output:
/// `    Path    REG_EXPAND_SZ    C:\a;%SystemRoot%\b`
fn parse_reg_value(out: &str) -> Option<String> {
    out.lines().find_map(|l| {
        let mut parts = l.trim().splitn(3, "    ");
        let name = parts.next()?;
        let ty = parts.next()?;
        if name.eq_ignore_ascii_case("path") && ty.trim().starts_with("REG_") {
            Some(parts.next().unwrap_or("").trim().to_string())
        } else {
            None
        }
    })
}

/// Expand `%NAME%` references with `lookup`; unknown names are left as-is.
fn expand(s: &str, lookup: impl Fn(&str) -> Option<String>) -> String {
    let mut out = String::new();
    let mut rest = s;
    while let Some(start) = rest.find('%') {
        out.push_str(&rest[..start]);
        let after = &rest[start + 1..];
        match after.find('%') {
            Some(end) if end > 0 => {
                let name = &after[..end];
                match lookup(name) {
                    Some(v) => out.push_str(&v),
                    None => {
                        out.push('%');
                        out.push_str(name);
                        out.push('%');
                    }
                }
                rest = &after[end + 1..];
            }
            _ => {
                out.push('%');
                rest = after;
            }
        }
    }
    out.push_str(rest);
    out
}

fn norm(dir: &str) -> String {
    dir.trim()
        .trim_matches('"')
        .trim_end_matches(['\\', '/'])
        .to_ascii_lowercase()
}

/// Keep `current` as-is (it may hold session-only entries) and append every
/// directory from `fresh` it doesn't already contain.
fn merge(current: &str, fresh: &str) -> (String, Vec<String>) {
    let mut have: Vec<String> = current.split(';').map(norm).collect();
    let mut merged = current.trim_end_matches(';').to_string();
    let mut added = Vec::new();
    for d in fresh.split(';') {
        let n = norm(d);
        if n.is_empty() || have.contains(&n) {
            continue;
        }
        have.push(n);
        let d = d.trim();
        if !merged.is_empty() {
            merged.push(';');
        }
        merged.push_str(d);
        added.push(d.to_string());
    }
    (merged, added)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_reg_query_output() {
        let out = "\r\nHKEY_CURRENT_USER\\Environment\r\n    Path    REG_EXPAND_SZ    %USERPROFILE%\\x;C:\\Program Files\\y\r\n\r\n";
        assert_eq!(
            parse_reg_value(out).as_deref(),
            Some(r"%USERPROFILE%\x;C:\Program Files\y")
        );
        assert_eq!(parse_reg_value("ERROR: not found"), None);
    }

    #[test]
    fn expands_known_vars_and_keeps_unknown() {
        let lookup = |k: &str| match k.to_ascii_uppercase().as_str() {
            "SYSTEMROOT" => Some(r"C:\WINDOWS".to_string()),
            _ => None,
        };
        assert_eq!(
            expand(r"%SystemRoot%\system32;%NOPE%\x;100%", lookup),
            r"C:\WINDOWS\system32;%NOPE%\x;100%"
        );
    }

    #[test]
    fn merge_appends_only_new_dirs() {
        let (m, added) = merge(
            r"C:\a;C:\B\;D:\session-only",
            r"c:\a\;C:\b;C:\Users\me\scoop\apps\openssl\current\bin;",
        );
        assert_eq!(added, [r"C:\Users\me\scoop\apps\openssl\current\bin"]);
        assert_eq!(
            m,
            r"C:\a;C:\B\;D:\session-only;C:\Users\me\scoop\apps\openssl\current\bin"
        );
    }

    #[test]
    fn merge_nothing_new() {
        let (m, added) = merge(r"C:\a;C:\b", r"C:\b;C:\a");
        assert!(added.is_empty());
        assert_eq!(m, r"C:\a;C:\b");
    }
}
