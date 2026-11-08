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

/// The user PATH with `dir` appended, or None when it's already on it
/// (compared case-insensitively, ignoring a trailing `\` and expanding
/// `%VARS%` in the existing entries).
pub fn with_dir(existing: &str, dir: &str) -> Option<String> {
    let norm = |s: &str| s.trim().trim_end_matches('\\').to_lowercase();
    let want = norm(dir);
    let present = existing
        .split(';')
        .any(|e| norm(&expand(e, |k| std::env::var(k).ok())) == want || norm(e) == want);
    if present {
        return None;
    }
    let base = existing.trim_end_matches(';');
    Some(if base.trim().is_empty() {
        dir.to_string()
    } else {
        format!("{base};{dir}")
    })
}

/// Put `dir` on the *user* PATH (no admin), so new terminals find
/// winsploit.exe. Returns false when it was already there. The value keeps
/// its REG_EXPAND_SZ type (setx and .NET's SetEnvironmentVariable would turn
/// `%USERPROFILE%\...` entries into plain text), then running programs are
/// told the environment changed, as the System Properties dialog does.
pub fn add_user_dir(dir: &std::path::Path) -> anyhow::Result<bool> {
    let dir = dir.display().to_string();
    let existing = reg_path(USER_KEY).unwrap_or_default();
    let Some(new) = with_dir(&existing, &dir) else {
        return Ok(false);
    };
    // the new value goes in through the environment, never into the script text
    let script = "$k = [Microsoft.Win32.Registry]::CurrentUser.OpenSubKey('Environment', $true); \
        $k.SetValue('Path', $env:WINSPLOIT_NEW_PATH, [Microsoft.Win32.RegistryValueKind]::ExpandString); \
        $k.Close(); \
        [Environment]::SetEnvironmentVariable('WINSPLOIT_PATH_PING', $null, 'User')";
    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("WINSPLOIT_NEW_PATH", &new)
        .stdout(Stdio::null())
        .status()?;
    if !status.success() {
        anyhow::bail!("PowerShell couldn't update the user PATH ({status})");
    }
    Ok(true)
}

#[cfg(test)]
mod with_dir_tests {
    use super::with_dir;

    #[test]
    fn appends_once() {
        let d = r"C:\Users\me\projects\winsploit\target\release";
        assert_eq!(with_dir("", d).as_deref(), Some(d));
        assert_eq!(
            with_dir(r"C:\a;C:\b;", d).unwrap(),
            format!(r"C:\a;C:\b;{d}")
        );
        // already there: any case, with or without a trailing backslash
        assert_eq!(with_dir(&format!(r"C:\a;{}\", d.to_uppercase()), d), None);
        // %VARS% in existing entries are compared expanded
        std::env::set_var("WINSPLOIT_TEST_HOME", r"C:\Users\me");
        assert_eq!(
            with_dir(
                r"%WINSPLOIT_TEST_HOME%\projects\winsploit\target\release",
                d
            ),
            None
        );
    }
}
