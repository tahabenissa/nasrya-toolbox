//! Projects: a chosen working folder, like a VS Code or Ghidra project. When
//! one is open, tools run inside it (so you type `scan.txt`, not a full path),
//! logs and outputs land under it, and the args token `{proj}` expands to its
//! path. Recent projects persist and the last one reopens on startup.

use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

/// The active project: a name (its folder name) and its path.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Project {
    pub name: String,
    pub path: PathBuf,
}

impl Project {
    /// Open an existing folder as a project (name = the folder's name).
    pub fn open(path: PathBuf) -> Self {
        let name = path
            .file_name()
            .map(|s| s.to_string_lossy().into_owned())
            .unwrap_or_else(|| path.display().to_string());
        Project { name, path }
    }

    /// Where this project's run logs go: `<project>\.winsploit\logs`.
    pub fn log_dir(&self) -> PathBuf {
        self.path.join(".winsploit").join("logs")
    }
}

/// Persisted list of recent projects and the last one opened.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct Store {
    #[serde(default)]
    pub recents: Vec<PathBuf>,
    #[serde(default)]
    pub last: Option<PathBuf>,
}

impl Store {
    fn file() -> Option<PathBuf> {
        std::env::var_os("APPDATA")
            .map(|a| PathBuf::from(a).join("winsploit").join("projects.json"))
    }

    pub fn load() -> Self {
        let Some(f) = Self::file() else {
            return Store::default();
        };
        std::fs::read_to_string(&f)
            .ok()
            .and_then(|t| serde_json::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> std::io::Result<()> {
        let Some(f) = Self::file() else {
            return Ok(());
        };
        if let Some(dir) = f.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let json = serde_json::to_string_pretty(self).unwrap_or_default();
        std::fs::write(&f, json)
    }

    /// Record `path` as the most-recent project (dedup, cap at 10) and mark it
    /// as the one to reopen next time.
    pub fn touch(&mut self, path: &Path) {
        self.recents.retain(|p| p != path);
        self.recents.insert(0, path.to_path_buf());
        self.recents.truncate(10);
        self.last = Some(path.to_path_buf());
    }

    /// Recent projects whose folder still exists, most-recent first.
    pub fn existing_recents(&self) -> Vec<PathBuf> {
        self.recents.iter().filter(|p| p.is_dir()).cloned().collect()
    }
}

/// A sensible folder to start the browser in: the user's home, else the
/// current directory.
pub fn home_dir() -> PathBuf {
    std::env::var_os("USERPROFILE")
        .map(PathBuf::from)
        .filter(|p| p.is_dir())
        .or_else(|| std::env::current_dir().ok())
        .unwrap_or_else(|| PathBuf::from("."))
}

/// Immediate sub-folders of `dir`, sorted by name (hidden/system ones skipped).
pub fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(dir)
        .into_iter()
        .flatten()
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.is_dir())
        .filter(|p| {
            p.file_name()
                .and_then(|n| n.to_str())
                .map(|n| !n.starts_with('.') && !n.starts_with('$'))
                .unwrap_or(false)
        })
        .collect();
    v.sort();
    v
}

/// Expand the `{proj}` token in a tool argument to the project path.
pub fn expand(arg: &str, project: Option<&Project>) -> String {
    match project {
        Some(p) if arg.contains("{proj}") => {
            arg.replace("{proj}", &p.path.to_string_lossy())
        }
        _ => arg.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn touch_dedups_and_caps_and_sets_last() {
        let mut s = Store::default();
        for i in 0..12 {
            s.touch(Path::new(&format!("C:/p{i}")));
        }
        assert_eq!(s.recents.len(), 10);
        assert_eq!(s.recents[0], PathBuf::from("C:/p11"));
        assert_eq!(s.last, Some(PathBuf::from("C:/p11")));
        // touching an existing one moves it to the front, no duplicate
        s.touch(Path::new("C:/p5"));
        assert_eq!(s.recents[0], PathBuf::from("C:/p5"));
        assert_eq!(s.recents.iter().filter(|p| **p == PathBuf::from("C:/p5")).count(), 1);
    }

    #[test]
    fn open_takes_the_folder_name() {
        let p = Project::open(PathBuf::from("C:/ctf/hackthebox"));
        assert_eq!(p.name, "hackthebox");
    }

    #[test]
    fn expand_replaces_only_with_a_project() {
        let p = Project::open(PathBuf::from("C:/ctf/box"));
        assert_eq!(expand("-oN {proj}/scan.txt", Some(&p)), "-oN C:/ctf/box/scan.txt");
        assert_eq!(expand("{proj}", None), "{proj}");
        assert_eq!(expand("plain", Some(&p)), "plain");
    }
}
