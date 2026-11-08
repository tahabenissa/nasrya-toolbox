//! How winsploit looks as a Windows app: the console title while it runs,
//! and `winsploit setup --shortcut`, which adds a Windows Terminal profile
//! (the Iso cube on the tab, Frost colours) and a Start menu entry.
//!
//! The taskbar button of a program running in Windows Terminal is always
//! Terminal's own; the profile is what gives the tab and the Terminal menu
//! the cube, and the Start menu shortcut gives search and pinning the cube.

use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use anyhow::{Context, Result};

/// The app icon (assets/winsploit.ico, also embedded in the exe by build.rs).
const ICON: &[u8] = include_bytes!("../assets/winsploit.ico");

/// Name of the Windows Terminal profile and the Start menu entry.
pub const NAME: &str = "Winsploit";

// ------------------------------------------------------------ console title

#[cfg(windows)]
mod ffi {
    #[link(name = "kernel32")]
    extern "system" {
        pub fn GetConsoleTitleW(buf: *mut u16, size: u32) -> u32;
        pub fn SetConsoleTitleW(title: *const u16) -> i32;
    }
}

/// The console's current title (what the tab shows), if there is a console.
pub fn title() -> Option<String> {
    #[cfg(windows)]
    {
        let mut buf = vec![0u16; 1024];
        // SAFETY: the buffer is valid for `buf.len()` u16s; the call writes at
        // most that many and returns how many it wrote (0 = no title/console).
        let n = unsafe { ffi::GetConsoleTitleW(buf.as_mut_ptr(), buf.len() as u32) } as usize;
        (n > 0).then(|| String::from_utf16_lossy(&buf[..n.min(buf.len())]))
    }
    #[cfg(not(windows))]
    {
        None
    }
}

/// Set the console title. Windows Terminal shows it on the tab (through
/// ConPTY), the classic console in its title bar. Best effort.
pub fn set_title(t: &str) {
    #[cfg(windows)]
    {
        let wide: Vec<u16> = t.encode_utf16().chain(std::iter::once(0)).collect();
        // SAFETY: `wide` is NUL-terminated and outlives the call.
        unsafe {
            ffi::SetConsoleTitleW(wide.as_ptr());
        }
    }
    #[cfg(not(windows))]
    let _ = t;
}

/// The title shown while winsploit runs: the name and the working folder,
/// so several tabs tell apart.
pub fn title_for(cwd: &Path) -> String {
    format!(
        "winsploit · {}",
        crate::folder::fit(&crate::folder::short(cwd), 40)
    )
}

// --------------------------------------------------------- setup --shortcut

/// The Windows Terminal profile fragment: a "Winsploit" profile running
/// `exe` with the cube icon and a Frost colour scheme.
pub fn fragment(exe: &Path, icon: &Path, start_dir: &str) -> serde_json::Value {
    serde_json::json!({
        "profiles": [{
            "name": NAME,
            "commandline": format!("\"{}\"", exe.display()),
            "icon": icon.display().to_string(),
            "startingDirectory": start_dir,
            "colorScheme": "Winsploit Frost",
            "hidden": false
        }],
        // Frost: slate background, icy blue accent (the web view's palette);
        // the 16 ANSI colours serve the ANSI-only themes and the tools' output
        "schemes": [{
            "name": "Winsploit Frost",
            "background": "#1B1F27",
            "foreground": "#E9EAED",
            "cursorColor": "#89C2D2",
            "selectionBackground": "#3D4552",
            "black": "#3B4252",
            "red": "#E06C7A",
            "green": "#8FC99A",
            "yellow": "#EBCB8B",
            "blue": "#81A1C1",
            "purple": "#B48EAD",
            "cyan": "#89C2D2",
            "white": "#E5E9F0",
            "brightBlack": "#5C667A",
            "brightRed": "#FF7A8C",
            "brightGreen": "#A3D9AE",
            "brightYellow": "#F0D399",
            "brightBlue": "#9CB7D6",
            "brightPurple": "#C9A3C2",
            "brightCyan": "#BCDDE6",
            "brightWhite": "#F3F4F7"
        }]
    })
}

fn local_appdata() -> Result<PathBuf> {
    std::env::var_os("LOCALAPPDATA")
        .map(PathBuf::from)
        .context("%LOCALAPPDATA% is not set")
}

/// Where Windows Terminal (Store or not) reads per-user profile fragments.
pub fn fragment_path() -> Result<PathBuf> {
    Ok(local_appdata()?
        .join("Microsoft")
        .join("Windows Terminal")
        .join("Fragments")
        .join(NAME)
        .join("winsploit.json"))
}

/// The Start menu shortcut.
pub fn start_menu_path() -> Result<PathBuf> {
    let roaming = std::env::var_os("APPDATA").context("%APPDATA% is not set")?;
    Ok(PathBuf::from(roaming)
        .join("Microsoft")
        .join("Windows")
        .join("Start Menu")
        .join("Programs")
        .join(format!("{NAME}.lnk")))
}

/// What `setup --shortcut` did, for the message.
pub struct Installed {
    pub icon: PathBuf,
    pub fragment: PathBuf,
    pub start_menu: PathBuf,
    /// the shortcut opens the Windows Terminal profile (else the exe itself)
    pub via_terminal: bool,
}

/// `winsploit setup --shortcut`: the cube icon, the Windows Terminal profile
/// and the Start menu entry, all per user (no admin). Running it again
/// refreshes them (for example after moving winsploit.exe).
pub fn install(exe: &Path) -> Result<Installed> {
    let dir = local_appdata()?.join("winsploit");
    std::fs::create_dir_all(&dir)?;
    let icon = dir.join("winsploit.ico");
    std::fs::write(&icon, ICON).with_context(|| format!("writing {}", icon.display()))?;

    // start where challenges live when there are any, else the profile
    let ctf = crate::workspace::root();
    let start_dir = if ctf.is_dir() {
        ctf.display().to_string()
    } else {
        "%USERPROFILE%".to_string()
    };
    let fragment_file = fragment_path()?;
    if let Some(parent) = fragment_file.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_string_pretty(&fragment(exe, &icon, &start_dir))?;
    std::fs::write(&fragment_file, json)
        .with_context(|| format!("writing {}", fragment_file.display()))?;

    // the shortcut opens the Terminal profile when Terminal is there
    let wt = crate::detect::resolve("wt");
    let (target, args) = match &wt {
        Some(wt) => (wt.clone(), format!("-p \"{NAME}\"")),
        None => (exe.to_path_buf(), String::new()),
    };
    let lnk = start_menu_path()?;
    // values go in through the environment, never into the script text
    let script = "$s = (New-Object -ComObject WScript.Shell).CreateShortcut($env:WS_LNK); \
        $s.TargetPath = $env:WS_TARGET; $s.Arguments = $env:WS_ARGS; \
        $s.IconLocation = $env:WS_ICON + ',0'; $s.WorkingDirectory = $env:USERPROFILE; \
        $s.Description = 'Winsploit: native-Windows security toolbox'; $s.Save()";
    let status = Command::new("powershell")
        .args(["-NoProfile", "-NonInteractive", "-Command", script])
        .env("WS_LNK", &lnk)
        .env("WS_TARGET", &target)
        .env("WS_ARGS", &args)
        .env("WS_ICON", &icon)
        .stdout(Stdio::null())
        .status()
        .context("starting PowerShell")?;
    if !status.success() {
        anyhow::bail!("PowerShell couldn't create {} ({status})", lnk.display());
    }
    Ok(Installed {
        icon,
        fragment: fragment_file,
        start_menu: lnk,
        via_terminal: wt.is_some(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fragment_is_a_terminal_profile_with_the_cube() {
        let exe = Path::new(r"C:\Users\me\projects\winsploit\target\release\winsploit.exe");
        let icon = Path::new(r"C:\Users\me\AppData\Local\winsploit\winsploit.ico");
        let f = fragment(exe, icon, "%USERPROFILE%");
        let p = &f["profiles"][0];
        assert_eq!(p["name"], NAME);
        // quoted, so a path with spaces stays one program
        assert_eq!(p["commandline"], format!("\"{}\"", exe.display()));
        assert_eq!(p["icon"], icon.display().to_string());
        assert_eq!(p["colorScheme"], f["schemes"][0]["name"]);
        // round-trips as JSON (backslashes escaped)
        let text = serde_json::to_string(&f).unwrap();
        let back: serde_json::Value = serde_json::from_str(&text).unwrap();
        assert_eq!(back, f);
        // all 16 ANSI colours are there
        for k in [
            "black", "red", "green", "yellow", "blue", "purple", "cyan", "white",
        ] {
            assert!(f["schemes"][0][k].is_string(), "{k}");
            let bright = format!("bright{}{}", k[..1].to_uppercase(), &k[1..]);
            assert!(f["schemes"][0][bright.as_str()].is_string(), "{bright}");
        }
    }

    #[test]
    fn the_icon_is_a_real_ico() {
        // reserved 0, type 1 (icon), at least one image
        assert_eq!(&ICON[..4], &[0, 0, 1, 0]);
        assert!(u16::from_le_bytes([ICON[4], ICON[5]]) >= 1);
    }

    #[test]
    fn title_names_the_folder() {
        let t = title_for(Path::new(r"C:\ctf\htb\lame"));
        assert_eq!(t, r"winsploit · C:\ctf\htb\lame");
        let long = title_for(Path::new(
            r"C:\some\really\long\folder\structure\that\goes\on\and\on\htb\lame",
        ));
        assert!(long.ends_with(r"\htb\lame") && long.contains('…'), "{long}");
    }
}
