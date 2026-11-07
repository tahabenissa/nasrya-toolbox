//! Tool registry: loads the config-driven catalog from `tools.yaml`.
//! Adding a tool is a YAML edit, never a code change.

use std::collections::BTreeMap;
use std::fs;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use serde::Deserialize;

/// How a tool gets installed.
#[derive(Debug, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Install {
    /// `scoop install [bucket/]<scoop>` (the default)
    #[default]
    Scoop,
    /// `pipx install <pipx>`: Python tools in their own venv
    Pipx,
}

/// How a tool is launched.
#[derive(Debug, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
#[serde(rename_all = "lowercase")]
pub enum Launch {
    /// CLI: output streams into the output pane (the default)
    #[default]
    Stream,
    /// Desktop app: started detached; nothing to stream or wait for
    Gui,
    /// Local web app: optionally start `run_cmd` (a server, streamed), then
    /// open `url` in the default browser
    Web,
    /// Data, not a program (wordlists): `exe` is a file inside the dataset;
    /// Enter lists the folder and copies its path instead of running anything
    Dataset,
}

#[derive(Debug, Deserialize, Clone)]
pub struct Tool {
    pub name: String,
    #[serde(default)]
    pub category: String,
    #[serde(default)]
    pub description: String,
    /// Executable name used to detect installation (PATH probe).
    pub exe: String,
    /// Install backend (default: scoop).
    #[serde(default)]
    pub install: Install,
    /// Scoop package name (install: scoop).
    #[serde(default)]
    pub scoop: Option<String>,
    /// Optional scoop bucket the package lives in (main-bucket tools omit this).
    #[serde(default)]
    pub bucket: Option<String>,
    /// pipx package spec, e.g. `arjun` or `git+https://github.com/o/r`
    /// (install: pipx).
    #[serde(default)]
    pub pipx: Option<String>,
    /// Command actually launched. May be empty for `launch: web` tools that
    /// are only a URL.
    #[serde(default)]
    pub run_cmd: String,
    #[serde(default)]
    pub args_hint: Option<String>,
    /// Launch mode (default: stream).
    #[serde(default)]
    pub launch: Launch,
    /// URL opened by `launch: web`.
    #[serde(default)]
    pub url: Option<String>,
    /// Caveat shown when the tool is run (e.g. Windows limitations).
    #[serde(default)]
    pub note: Option<String>,
    /// Check that the `exe` found on PATH is really this tool, not another
    /// program with the same name.
    #[serde(default)]
    pub verify: Option<Verify>,
}

/// `exe <args>` must print something containing `contains` (any case, on
/// stdout or stderr) within a few seconds.
#[derive(Debug, Deserialize, Clone)]
pub struct Verify {
    pub args: String,
    pub contains: String,
}

impl Tool {
    /// Problems that make this entry unusable, if any.
    fn problems(&self) -> Vec<String> {
        let mut p = Vec::new();
        let missing = |f: &Option<String>| f.as_deref().is_none_or(|s| s.trim().is_empty());
        match self.install {
            Install::Scoop if missing(&self.scoop) => {
                p.push("install: scoop needs a `scoop:` package".into())
            }
            Install::Pipx if missing(&self.pipx) => {
                p.push("install: pipx needs a `pipx:` package spec".into())
            }
            _ => {}
        }
        match self.launch {
            Launch::Dataset => {}
            Launch::Web => match self.url.as_deref() {
                None => p.push("launch: web needs a `url:`".into()),
                Some(u)
                    if !(u.starts_with("http://")
                        || u.starts_with("https://")
                        || is_local_page(u)) =>
                {
                    p.push(format!(
                        "url must be http(s):// or a {SCOOP_APP}\\...html page (got {u:?})"
                    ))
                }
                _ => {}
            },
            _ if self.run_cmd.trim().is_empty() => p.push("`run_cmd:` is required".into()),
            _ => {}
        }
        let uses_app = [&self.exe, &self.run_cmd]
            .into_iter()
            .chain(self.url.as_ref())
            .any(|s| s.contains(SCOOP_APP));
        if uses_app && (self.install != Install::Scoop || self.scoop.is_none()) {
            p.push(format!(
                "{SCOOP_APP} needs install: scoop with a `scoop:` package"
            ));
        }
        p
    }

    /// Replace `{scoop_app}` with this tool's Scoop app directory
    /// (`<scoop root>\apps\<scoop>\current`) in exe, run_cmd and url.
    ///
    /// Also expands `{app:NAME}` to *another* Scoop app's directory, so an
    /// args_hint can point at an installed dataset:
    /// `-w "{app:seclists-lite}\common.txt"`. args_hint gets both.
    pub fn expand_scoop_app(&mut self, scoop_root: &Path) {
        let own = self
            .scoop
            .as_deref()
            .map(|pkg| scoop_root.join("apps").join(pkg).join("current"));
        let expand = |s: &str| -> String {
            let mut s = match &own {
                Some(dir) => s.replace(SCOOP_APP, &dir.to_string_lossy()),
                None => s.to_string(),
            };
            let mut from = 0;
            while let Some(rel) = s[from..].find("{app:") {
                let start = from + rel;
                let Some(len) = s[start..].find('}') else {
                    break;
                };
                let name = &s[start + 5..start + len];
                let valid = !name.is_empty()
                    && name
                        .chars()
                        .all(|c| c.is_ascii_alphanumeric() || matches!(c, '-' | '_' | '.'));
                if !valid {
                    from = start + 5; // leave it as-is, keep scanning
                    continue;
                }
                let dir = scoop_root.join("apps").join(name).join("current");
                let dir = dir.to_string_lossy();
                s.replace_range(start..start + len + 1, &dir);
                from = start + dir.len();
            }
            s
        };
        self.exe = expand(&self.exe);
        self.run_cmd = expand(&self.run_cmd);
        self.url = self.url.as_deref().map(expand);
        self.args_hint = self.args_hint.as_deref().map(expand);
    }
}

/// Placeholder for a Scoop app's install directory, for files Scoop doesn't
/// put on PATH (dnSpy.exe, CyberChef.html, ...).
pub const SCOOP_APP: &str = "{scoop_app}";

/// A `launch: web` url may also be a local HTML page inside the app dir.
fn is_local_page(u: &str) -> bool {
    u.starts_with(SCOOP_APP) && (u.ends_with(".html") || u.ends_with(".htm"))
}

/// The whole catalog file.
#[derive(Debug, Deserialize)]
pub struct Catalog {
    pub tools: Vec<Tool>,
    /// Extra Scoop buckets: name -> git URL. Buckets Scoop already knows by
    /// name (extras, versions, ...) don't need an entry.
    #[serde(default)]
    pub buckets: BTreeMap<String, String>,
}

/// Where to look for `tools.yaml`, in priority order.
///
/// The catalog decides which commands Enter runs, so it is **never** taken
/// implicitly from the current directory: winsploit is routinely started inside
/// untrusted folders (challenge files, loot, cloned repos), and a planted
/// `tools.yaml` there could point `nmap` at anything. To use one on purpose,
/// pass `--catalog .\tools.yaml` or set `WINSPLOIT_CATALOG`.
pub fn catalog_candidates(explicit: Option<&Path>) -> Vec<PathBuf> {
    if let Some(p) = explicit {
        return vec![p.to_path_buf()];
    }
    if let Some(p) = std::env::var_os("WINSPLOIT_CATALOG") {
        return vec![PathBuf::from(p)];
    }
    let mut out = Vec::new();
    if let Some(dir) = std::env::current_exe()
        .ok()
        .and_then(|e| e.parent().map(Path::to_path_buf))
    {
        // shipped layout: tools.yaml next to winsploit.exe
        out.push(dir.join("tools.yaml"));
        // dev layout: target\{debug,release}\winsploit.exe inside the winsploit
        // checkout. That's the binary's own source tree, not wherever the
        // user happens to be.
        for root in dir.ancestors().skip(1).take(3) {
            if is_winsploit_checkout(root) {
                out.push(root.join("tools.yaml"));
                break;
            }
        }
    }
    if let Some(appdata) = std::env::var_os("APPDATA") {
        out.push(PathBuf::from(appdata).join("winsploit").join("tools.yaml"));
    }
    out
}

fn is_winsploit_checkout(dir: &Path) -> bool {
    fs::read_to_string(dir.join("Cargo.toml"))
        .map(|t| t.lines().any(|l| l.trim() == r#"name = "winsploit""#))
        .unwrap_or(false)
}

/// Resolve the catalog path: the first candidate that exists.
pub fn find_catalog(explicit: Option<&Path>) -> Result<PathBuf> {
    let cands = catalog_candidates(explicit);
    if let Some(p) = cands.iter().find(|p| p.is_file()) {
        return Ok(p.clone());
    }
    let tried: Vec<String> = cands.iter().map(|p| format!("  {}", p.display())).collect();
    anyhow::bail!(
        "no tools.yaml found. Looked in:\n{}\n(use --catalog <path> to point at one)",
        tried.join("\n")
    )
}

/// If there is a `tools.yaml` in the current directory that we are *not*
/// using, return its path so the UI can say it was ignored.
pub fn ignored_cwd_catalog(active: &Path) -> Option<PathBuf> {
    let cwd = std::env::current_dir().ok()?.join("tools.yaml");
    if !cwd.is_file() {
        return None;
    }
    let same = match (fs::canonicalize(&cwd), fs::canonicalize(active)) {
        (Ok(a), Ok(b)) => a == b,
        _ => false,
    };
    (!same).then_some(cwd)
}

pub fn load(path: impl AsRef<Path>) -> Result<Catalog> {
    let path = path.as_ref();
    let text = fs::read_to_string(path)
        .with_context(|| format!("reading tool registry {}", path.display()))?;
    let mut cat = parse(&text).with_context(|| format!("in {}", path.display()))?;
    if let Some(root) = crate::scoop::scoop_root() {
        for t in &mut cat.tools {
            t.expand_scoop_app(&root);
        }
    }
    Ok(cat)
}

/// Parse and validate catalog text.
pub fn parse(text: &str) -> Result<Catalog> {
    let cat: Catalog = serde_yaml::from_str(text).context("parsing tools.yaml")?;
    let errors: Vec<String> = cat
        .tools
        .iter()
        .flat_map(|t| {
            t.problems()
                .into_iter()
                .map(move |e| format!("  {}: {e}", t.name))
        })
        .collect();
    if !errors.is_empty() {
        anyhow::bail!("invalid tool entries:\n{}", errors.join("\n"));
    }
    Ok(cat)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_are_scoop_and_stream() {
        let c = parse("tools:\n  - {name: nmap, exe: nmap, scoop: nmap, run_cmd: nmap}\n").unwrap();
        assert_eq!(c.tools[0].install, Install::Scoop);
        assert_eq!(c.tools[0].launch, Launch::Stream);
    }

    #[test]
    fn pipx_and_web_entries() {
        let c = parse(
            "tools:\n\
             \x20 - {name: arjun, exe: arjun, install: pipx, pipx: arjun, run_cmd: arjun}\n\
             \x20 - {name: chef, exe: x, scoop: x, launch: web, url: 'http://127.0.0.1:1'}\n",
        )
        .unwrap();
        assert_eq!(c.tools[0].install, Install::Pipx);
        assert_eq!(c.tools[1].launch, Launch::Web);
    }

    #[test]
    fn invalid_entries_are_reported_together() {
        let e = parse(
            "tools:\n\
             \x20 - {name: a, exe: a, run_cmd: a}\n\
             \x20 - {name: b, exe: b, install: pipx, run_cmd: b}\n\
             \x20 - {name: c, exe: c, scoop: c, launch: web, url: 'file:///x'}\n\
             \x20 - {name: d, exe: d, scoop: d}\n",
        )
        .unwrap_err();
        let msg = format!("{e:#}");
        for needle in [
            "a: install: scoop",
            "b: install: pipx",
            "c: url must",
            "d: `run_cmd:`",
        ] {
            assert!(msg.contains(needle), "{needle} missing from:\n{msg}");
        }
    }

    #[test]
    fn scoop_app_placeholder() {
        let mut c = parse(
            "tools:\n\
             \x20 - {name: dnspy, exe: '{scoop_app}\\dnSpy.exe', scoop: dnspyex, bucket: extras, run_cmd: '{scoop_app}\\dnSpy.exe', launch: gui}\n\
             \x20 - {name: chef, exe: '{scoop_app}\\CyberChef.html', scoop: cyberchef, launch: web, url: '{scoop_app}\\CyberChef.html'}\n",
        )
        .unwrap();
        let root = Path::new(r"C:\Users\me\scoop");
        for t in &mut c.tools {
            t.expand_scoop_app(root);
        }
        assert_eq!(
            c.tools[0].exe,
            r"C:\Users\me\scoop\apps\dnspyex\current\dnSpy.exe"
        );
        assert_eq!(c.tools[0].run_cmd, c.tools[0].exe);
        assert_eq!(
            c.tools[1].url.as_deref(),
            Some(r"C:\Users\me\scoop\apps\cyberchef\current\CyberChef.html")
        );
    }

    #[test]
    fn app_placeholder_in_args_hint() {
        let mut c = parse(
            "tools:\n\
             \x20 - {name: gobuster, exe: gobuster, scoop: gobuster, run_cmd: gobuster, args_hint: 'dir -w \"{app:seclists-lite}\\common.txt\" -u x'}\n\
             \x20 - {name: wl, exe: '{scoop_app}\\common.txt', scoop: seclists-lite, bucket: winsploit, launch: dataset}\n\
             \x20 - {name: odd, exe: x, scoop: x, run_cmd: x, args_hint: '{app:} {app:a b} {app:ok} {app:unclosed'}\n",
        )
        .unwrap();
        let root = Path::new(r"C:\s");
        for t in &mut c.tools {
            t.expand_scoop_app(root);
        }
        assert_eq!(
            c.tools[0].args_hint.as_deref(),
            Some(r#"dir -w "C:\s\apps\seclists-lite\current\common.txt" -u x"#)
        );
        assert_eq!(
            c.tools[1].exe,
            r"C:\s\apps\seclists-lite\current\common.txt"
        );
        assert_eq!(c.tools[1].launch, Launch::Dataset);
        // malformed placeholders are left alone (and don't loop forever);
        // a valid one after them is still expanded
        assert_eq!(
            c.tools[2].args_hint.as_deref(),
            Some(r"{app:} {app:a b} C:\s\apps\ok\current {app:unclosed")
        );
    }

    #[test]
    fn scoop_app_needs_a_scoop_package() {
        assert!(parse(
            "tools:\n  - {name: x, exe: '{scoop_app}\\x.exe', install: pipx, pipx: x, run_cmd: x}\n"
        )
        .is_err());
        // web urls must be http(s) or a local page under {scoop_app}
        assert!(
            parse("tools:\n  - {name: x, exe: x, scoop: x, launch: web, url: 'C:\\x.html'}\n")
                .is_err()
        );
    }

    #[test]
    fn unknown_install_backend_is_an_error() {
        assert!(parse("tools:\n  - {name: a, exe: a, install: npm, run_cmd: a}\n").is_err());
    }

    #[test]
    fn shipped_catalog_has_no_escape_accidents() {
        // In YAML double quotes, "\raft" is a carriage return and "\x.." a hex
        // escape, so Windows paths must be single-quoted. Catch slips.
        let mut c = parse(include_str!("../tools.yaml")).unwrap();
        for t in &mut c.tools {
            t.expand_scoop_app(Path::new(r"C:\s"));
            let fields = [
                Some(&t.exe),
                Some(&t.run_cmd),
                t.args_hint.as_ref(),
                t.url.as_ref(),
                t.note.as_ref(),
            ];
            for f in fields.into_iter().flatten() {
                assert!(
                    !f.chars().any(char::is_control),
                    "{}: control char in {f:?}",
                    t.name
                );
            }
        }
        let gob = c.tools.iter().find(|t| t.name == "gobuster").unwrap();
        let argv = crate::args::split(gob.args_hint.as_deref().unwrap()).unwrap();
        assert!(
            argv.contains(&r"C:\s\apps\seclists-lite\current\common.txt".to_string()),
            "{argv:?}"
        );
    }

    #[test]
    fn shipped_catalog_is_valid() {
        let text = include_str!("../tools.yaml");
        let c = parse(text).unwrap();
        assert!(!c.tools.is_empty());
    }
}
