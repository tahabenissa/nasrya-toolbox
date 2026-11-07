//! Install planning: the command lines that install a tool with its
//! backend, bootstrapping missing prerequisites first.
//!
//! Planning is pure (prerequisites are passed in), so every path is
//! unit-tested; the app runs the steps joined with `&&` under `cmd /D /C`.

use std::collections::BTreeMap;

use crate::registry::{Install, Tool};
use crate::scoop;

/// What's already on this machine.
#[derive(Debug, Clone, Copy, Default)]
pub struct Prereqs {
    pub scoop: bool,
    /// A working `python` (not the Microsoft Store stub).
    pub python: bool,
    pub pipx: bool,
    /// The tool's Scoop bucket is already added.
    pub bucket_added: bool,
}

const NO_SCOOP: &str = "scoop isn't installed (or not on PATH). Install it first, in \
    PowerShell: irm get.scoop.sh | iex   (see https://scoop.sh)";

/// pipx specs end up on a `cmd /C` line: allow package names, extras,
/// `==` pins and `git+https://` URLs, but nothing cmd treats specially.
fn safe_pipx_spec(s: &str) -> bool {
    !s.is_empty()
        && s.chars().all(|c| {
            c.is_ascii_alphanumeric()
                || matches!(
                    c,
                    '.' | '_' | '-' | '+' | ':' | '/' | '@' | '=' | '[' | ']' | ',' | '~' | '#'
                )
        })
}

pub fn plan(
    tool: &Tool,
    pre: Prereqs,
    bucket_urls: &BTreeMap<String, String>,
) -> Result<Vec<String>, String> {
    match tool.install {
        Install::Scoop => {
            if !pre.scoop {
                return Err(NO_SCOOP.into());
            }
            let pkg = tool.scoop.as_deref().unwrap_or_default();
            scoop::plan_install(pkg, tool.bucket.as_deref(), pre.bucket_added, bucket_urls)
        }
        Install::Pipx => {
            let spec = tool.pipx.as_deref().unwrap_or_default();
            if !safe_pipx_spec(spec) {
                return Err(format!("refusing unsafe pipx spec {spec:?}"));
            }
            let mut steps = Vec::new();
            if !pre.pipx {
                if !pre.scoop {
                    return Err(format!(
                        "{} needs pipx, which isn't installed. Either install Scoop (winsploit \
                         then sets up Python + pipx for you) or run: python -m pip install \
                         --user pipx && python -m pipx ensurepath",
                        tool.name
                    ));
                }
                if !pre.python {
                    steps.push("scoop install python".to_string());
                }
                steps.push("scoop install pipx".to_string());
            }
            // Puts pipx's bin dir (~\.local\bin) on the user PATH; idempotent.
            // winsploit re-reads PATH after the install, so the tool shows up
            // without a restart.
            steps.push("pipx ensurepath".to_string());
            steps.push(format!("pipx install {spec}"));
            Ok(steps)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry::parse;

    fn tool(yaml: &str) -> Tool {
        parse(&format!("tools:\n  - {yaml}\n"))
            .unwrap()
            .tools
            .remove(0)
    }

    fn all() -> Prereqs {
        Prereqs {
            scoop: true,
            python: true,
            pipx: true,
            bucket_added: true,
        }
    }

    #[test]
    fn scoop_tool_needs_scoop() {
        let t = tool("{name: nmap, exe: nmap, scoop: nmap, run_cmd: nmap}");
        assert_eq!(
            plan(&t, all(), &BTreeMap::new()).unwrap(),
            ["scoop install nmap"]
        );
        let pre = Prereqs {
            scoop: false,
            ..all()
        };
        assert!(plan(&t, pre, &BTreeMap::new())
            .unwrap_err()
            .contains("scoop isn't installed"));
    }

    #[test]
    fn pipx_tool_with_pipx_present() {
        let t = tool("{name: arjun, exe: arjun, install: pipx, pipx: arjun, run_cmd: arjun}");
        assert_eq!(
            plan(&t, all(), &BTreeMap::new()).unwrap(),
            ["pipx ensurepath", "pipx install arjun"]
        );
    }

    #[test]
    fn pipx_bootstraps_python_and_pipx_via_scoop() {
        let t = tool("{name: arjun, exe: arjun, install: pipx, pipx: arjun, run_cmd: arjun}");
        let pre = Prereqs {
            python: false,
            pipx: false,
            ..all()
        };
        assert_eq!(
            plan(&t, pre, &BTreeMap::new()).unwrap(),
            [
                "scoop install python",
                "scoop install pipx",
                "pipx ensurepath",
                "pipx install arjun"
            ]
        );
        // python present: only pipx is added
        let pre = Prereqs {
            pipx: false,
            ..all()
        };
        assert_eq!(
            plan(&t, pre, &BTreeMap::new()).unwrap()[0],
            "scoop install pipx"
        );
    }

    #[test]
    fn pipx_without_pipx_or_scoop_explains_both_ways() {
        let t = tool("{name: arjun, exe: arjun, install: pipx, pipx: arjun, run_cmd: arjun}");
        let pre = Prereqs {
            scoop: false,
            pipx: false,
            ..all()
        };
        let e = plan(&t, pre, &BTreeMap::new()).unwrap_err();
        assert!(e.contains("install Scoop") && e.contains("pip install --user pipx"));
    }

    #[test]
    fn pipx_git_specs_are_allowed_but_metacharacters_are_not() {
        let ok = tool(
            "{name: nxc, exe: nxc, install: pipx, pipx: 'git+https://github.com/Pennyw0rth/NetExec', run_cmd: nxc}",
        );
        assert_eq!(
            plan(&ok, all(), &BTreeMap::new()).unwrap()[1],
            "pipx install git+https://github.com/Pennyw0rth/NetExec"
        );
        for bad in ["'arjun & calc'", "'a|b'", "'a>b'", "'%PATH%'", "'a^b'"] {
            let t = tool(&format!(
                "{{name: x, exe: x, install: pipx, pipx: {bad}, run_cmd: x}}"
            ));
            assert!(plan(&t, all(), &BTreeMap::new()).is_err(), "{bad} accepted");
        }
    }
}
