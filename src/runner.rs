//! Process runner: spawns an external tool with piped stdout/stderr and
//! streams every line back to the UI over a channel, without blocking the
//! TUI event loop. Also supports killing the process tree (tools spawn
//! grandchildren, so a plain child.kill() is not enough on Windows).

use std::io::{BufRead, BufReader, Read};
use std::path::PathBuf;
use std::process::{ChildStdin, Command, Stdio};
use std::sync::mpsc::Sender;
use std::thread;

use crate::detect;

/// Messages sent from runner threads into the app event loop. `sid` is the
/// terminal (session) the process belongs to, so up to four tools can stream
/// at once without their output mixing.
pub enum AppEvent {
    /// One line of tool output (stderr lines are prefixed with "[err] ").
    Output { sid: u64, line: String },
    /// The process finished with this exit code (None if it was killed/unknown).
    Exited { sid: u64, code: Option<i32> },
    /// Background detection finished: one result per tool.
    Detected(Vec<detect::Found>),
    /// An action sent from the web view (see web.rs), and who sent it.
    Web(crate::web::WebAction, crate::web::Caller),
    /// LAN view news: a guest waits to be let in, a join key was burned.
    Lan(crate::web::LanEvent),
    /// The agent finished analysing the output (Ok analysis, or an error message).
    Agent(Result<crate::agent::Analysis, String>),
    /// The side chat's AI answered request `req` (the model's raw text, or an
    /// error message); an answer to an older request is dropped.
    Chat {
        req: u64,
        result: Result<String, String>,
    },
    /// A newer Winsploit release is available (its tag, e.g. "v0.3.0").
    Update(String),
}

/// Build the command that runs a tool: `program` resolved the way Windows
/// would (see `resolve_program`) plus its arguments.
pub fn tool_command(program: &str, args: &[String]) -> Command {
    let mut cmd = Command::new(resolve_program(program));
    cmd.args(args);
    cmd
}

/// A started streaming process.
pub struct Spawned {
    pub pid: u32,
    /// Write end of the child's stdin (when it was asked for).
    pub stdin: Option<ChildStdin>,
}

/// Spawn `cmd`, streaming its output to `tx`, tagged with session `sid`.
///
/// The child never shares winsploit's console input: with `with_stdin` it gets
/// a pipe the UI can type into, otherwise stdin is closed. (Inheriting the
/// console made a prompting tool race the dashboard for keystrokes.)
pub fn spawn_streaming(
    mut cmd: Command,
    tx: Sender<AppEvent>,
    with_stdin: bool,
    sid: u64,
) -> std::io::Result<Spawned> {
    let stdin = if with_stdin {
        Stdio::piped()
    } else {
        Stdio::null()
    };
    let mut child = cmd
        .stdin(stdin)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()?;

    let pid = child.id();
    let stdin = child.stdin.take();

    let mut readers = Vec::new();
    if let Some(out) = child.stdout.take() {
        let tx = tx.clone();
        readers.push(thread::spawn(move || pump(out, &tx, sid, "")));
    }
    // stderr lines are tagged so the UI can colour them
    if let Some(err) = child.stderr.take() {
        let tx = tx.clone();
        readers.push(thread::spawn(move || pump(err, &tx, sid, "[err] ")));
    }

    // waiter thread: owns the child so we can reap it; we keep only the PID.
    // It reports the exit only after both pipes are drained, so the
    // "process exited" line never lands above the tool's last output.
    thread::spawn(move || {
        let code = child.wait().ok().and_then(|s| s.code());
        for r in readers {
            let _ = r.join();
        }
        let _ = tx.send(AppEvent::Exited { sid, code });
    });

    Ok(Spawned { pid, stdin })
}

/// Forward every line from `src` to the UI until EOF.
///
/// Reads raw bytes rather than `BufRead::lines()`: that errors on the first
/// line that isn't valid UTF-8 (common: Windows tools print in the console
/// codepage), and stopping there would leave the child blocked on a full pipe.
fn pump(src: impl Read, tx: &Sender<AppEvent>, sid: u64, prefix: &str) {
    let mut reader = BufReader::new(src);
    let mut buf = Vec::new();
    loop {
        buf.clear();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) | Err(_) => break,
            Ok(_) => {
                let line = clean_line(&buf);
                let line = format!("{prefix}{line}");
                if tx.send(AppEvent::Output { sid, line }).is_err() {
                    break;
                }
            }
        }
    }
}

/// Make one line of raw tool output safe to draw:
/// - decode as UTF-8, replacing invalid bytes instead of failing;
/// - a carriage-return progress bar (`10%\r20%\r30%`) keeps only what the
///   terminal would finally show (the last segment);
/// - ANSI escape sequences (colours, cursor moves) are removed;
/// - tabs become spaces, other control characters are dropped.
pub fn clean_line(raw: &[u8]) -> String {
    let text = String::from_utf8_lossy(raw);
    let text = text.trim_end_matches(['\n', '\r']);
    let last = text.rsplit('\r').find(|seg| !seg.is_empty()).unwrap_or("");
    let mut out = String::with_capacity(last.len());
    let mut chars = last.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '\u{1b}' => match chars.peek() {
                // CSI: ESC [ params... final byte in @..~
                Some('[') => {
                    chars.next();
                    for c in chars.by_ref() {
                        if ('@'..='~').contains(&c) {
                            break;
                        }
                    }
                }
                // OSC: ESC ] ... BEL or ESC \
                Some(']') => {
                    chars.next();
                    while let Some(c) = chars.next() {
                        if c == '\u{7}' || (c == '\u{1b}' && chars.peek() == Some(&'\\')) {
                            if c == '\u{1b}' {
                                chars.next();
                            }
                            break;
                        }
                    }
                }
                // charset designation: ESC ( B and friends (3 chars)
                Some('(' | ')' | '*' | '+') => {
                    chars.next();
                    chars.next();
                }
                // other two-char escapes: ESC 7, ESC =, ...
                Some(_) => {
                    chars.next();
                }
                None => {}
            },
            '\t' => out.push_str("    "),
            c if c.is_control() => {}
            c => out.push(c),
        }
    }
    out
}

/// Resolve a bare command name to the file Windows would actually run.
///
/// `Command::new("jadx")` only finds `jadx.exe`; it never tries `.cmd`/`.bat`,
/// and many Scoop shims (jadx, apktool, Python/Java wrappers) are exactly that.
/// So we resolve it with the same PATH x PATHEXT search detection uses
/// (`detect::resolve`), which also never looks in the current directory, and
/// launch the full path. std's `Command` runs a resolved `.bat`/`.cmd` through
/// cmd.exe with safe argument escaping. Falls back to the bare name.
fn resolve_program(program: &str) -> PathBuf {
    detect::resolve(program).unwrap_or_else(|| PathBuf::from(program))
}

/// Start a desktop app and let it go: no pipes, nothing to wait on.
/// CREATE_NO_WINDOW keeps `.bat` launchers (Ghidra, jadx-gui, ...) from
/// leaving an empty console window behind; GUI windows still appear.
pub fn spawn_detached(mut cmd: Command) -> std::io::Result<u32> {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    let child = cmd
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .creation_flags(CREATE_NO_WINDOW)
        .spawn()?;
    Ok(child.id())
}

/// Open an http(s) URL, or an existing local .html page, in the default
/// browser. Goes through `url.dll,FileProtocolHandler` rather than
/// `cmd /C start`, so characters like `&` are never interpreted by a shell.
pub fn open_url(url: &str) -> std::io::Result<()> {
    let web = url.starts_with("http://") || url.starts_with("https://");
    let lower = url.to_ascii_lowercase();
    let page = (lower.ends_with(".html") || lower.ends_with(".htm"))
        && std::path::Path::new(url).is_absolute()
        && std::path::Path::new(url).is_file();
    if !(web || page) {
        return Err(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            "only http(s) URLs and existing local .html pages are opened",
        ));
    }
    Command::new("rundll32")
        .args(["url.dll,FileProtocolHandler", url])
        .spawn()
        .map(|_| ())
}

// The Win32 clipboard calls (user32 / kernel32), declared here rather than
// pulling in a bindings crate for six functions.
mod win32 {
    use std::ffi::c_void;
    #[link(name = "user32")]
    extern "system" {
        pub fn OpenClipboard(owner: *mut c_void) -> i32;
        pub fn CloseClipboard() -> i32;
        pub fn EmptyClipboard() -> i32;
        pub fn SetClipboardData(format: u32, mem: *mut c_void) -> *mut c_void;
        #[cfg(test)]
        pub fn GetClipboardData(format: u32) -> *mut c_void;
    }
    #[link(name = "kernel32")]
    extern "system" {
        pub fn GlobalAlloc(flags: u32, bytes: usize) -> *mut c_void;
        pub fn GlobalLock(mem: *mut c_void) -> *mut c_void;
        pub fn GlobalUnlock(mem: *mut c_void) -> i32;
        pub fn GlobalFree(mem: *mut c_void) -> *mut c_void;
    }
    pub const CF_UNICODETEXT: u32 = 13;
    pub const GMEM_MOVEABLE: u32 = 0x0002;
}

/// Open the clipboard, retrying briefly: another app may hold it for a
/// moment. Pair every success with `CloseClipboard`.
fn open_clipboard() -> std::io::Result<()> {
    for _ in 0..20 {
        // SAFETY: no pointers involved; a null owner is allowed.
        if unsafe { win32::OpenClipboard(std::ptr::null_mut()) } != 0 {
            return Ok(());
        }
        std::thread::sleep(std::time::Duration::from_millis(10));
    }
    Err(std::io::Error::last_os_error())
}

/// Put exactly `text` on the Windows clipboard as Unicode text.
///
/// This used to pipe UTF-16 into clip.exe, which keeps the byte-order mark
/// (an invisible U+FEFF in front of every copy, so a pasted flag no longer
/// matched) and garbles input without one. The clipboard API has neither
/// problem.
pub fn copy_to_clipboard(text: &str) -> std::io::Result<()> {
    use win32::*;
    let wide: Vec<u16> = text.encode_utf16().chain(Some(0)).collect();
    open_clipboard()?;
    // SAFETY: the clipboard is open (closed below on every path). `mem` is a
    // fresh movable block of exactly `wide.len()` u16s; we write through the
    // locked pointer only within that size, unlock it, and hand it to the
    // clipboard, which owns it from then on. If any step fails we free it.
    let result = unsafe {
        if EmptyClipboard() == 0 {
            Err(std::io::Error::last_os_error())
        } else {
            let mem = GlobalAlloc(GMEM_MOVEABLE, wide.len() * 2);
            if mem.is_null() {
                Err(std::io::Error::last_os_error())
            } else {
                let dst = GlobalLock(mem) as *mut u16;
                if dst.is_null() {
                    let e = std::io::Error::last_os_error();
                    GlobalFree(mem);
                    Err(e)
                } else {
                    std::ptr::copy_nonoverlapping(wide.as_ptr(), dst, wide.len());
                    GlobalUnlock(mem);
                    if SetClipboardData(CF_UNICODETEXT, mem).is_null() {
                        let e = std::io::Error::last_os_error();
                        GlobalFree(mem);
                        Err(e)
                    } else {
                        Ok(())
                    }
                }
            }
        }
    };
    // SAFETY: we opened it above.
    unsafe { CloseClipboard() };
    result
}

/// The clipboard's Unicode text, for the round-trip test.
#[cfg(test)]
fn read_clipboard() -> std::io::Result<String> {
    use win32::*;
    open_clipboard()?;
    // SAFETY: the clipboard is open; the handle stays owned by the
    // clipboard; we read the NUL-terminated UTF-16 while it's locked.
    let out = unsafe {
        let h = GetClipboardData(CF_UNICODETEXT);
        let p = if h.is_null() {
            std::ptr::null_mut()
        } else {
            GlobalLock(h) as *const u16
        };
        if p.is_null() {
            Err(std::io::Error::last_os_error())
        } else {
            let mut n = 0;
            while *p.add(n) != 0 {
                n += 1;
            }
            let s = String::from_utf16_lossy(std::slice::from_raw_parts(p, n));
            GlobalUnlock(h);
            Ok(s)
        }
    };
    // SAFETY: opened above.
    unsafe { CloseClipboard() };
    out
}

/// Kill a process and its whole tree (taskkill /T). Best-effort.
pub fn kill_tree(pid: u32) {
    let _ = Command::new("taskkill")
        .args(["/T", "/F", "/PID", &pid.to_string()])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn();
}

#[cfg(test)]
mod tests {
    use super::clean_line;

    /// Replaces the real clipboard, so it only runs on request:
    /// `cargo test clipboard -- --ignored`
    #[test]
    #[ignore]
    fn clipboard_round_trip_is_exact() {
        for text in ["picoCTF{n0_b0m_h3r3}", "C:\\Users\\Zoé\\ctf\\chall 1", ""] {
            super::copy_to_clipboard(text).unwrap();
            let back = super::read_clipboard().unwrap();
            assert_eq!(back, text, "clipboard changed the text");
            assert!(!back.starts_with('\u{feff}'));
        }
    }

    /// Input typed in the UI reaches the tool, and dropping the pipe is EOF:
    /// `sort` only prints once its input ends.
    #[test]
    fn stdin_pipe_and_eof() {
        use super::{spawn_streaming, AppEvent};
        use std::io::Write;
        use std::time::Duration;

        let (tx, rx) = std::sync::mpsc::channel();
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/D", "/C", "sort"]);
        let mut sp = spawn_streaming(cmd, tx, true, 7).unwrap();
        let mut stdin = sp.stdin.take().expect("piped stdin");
        stdin.write_all(b"pear\napple\n").unwrap();
        drop(stdin); // EOF

        let mut lines = Vec::new();
        loop {
            match rx
                .recv_timeout(Duration::from_secs(10))
                .expect("runner stalled")
            {
                AppEvent::Output { sid, line } => {
                    assert_eq!(sid, 7);
                    lines.push(line)
                }
                AppEvent::Exited { sid, .. } => {
                    assert_eq!(sid, 7);
                    break;
                }
                AppEvent::Detected(_)
                | AppEvent::Web(..)
                | AppEvent::Lan(_)
                | AppEvent::Agent(_)
                | AppEvent::Chat { .. }
                | AppEvent::Update(_) => {}
            }
        }
        assert_eq!(lines, ["apple", "pear"]);
    }

    /// Through the real spawn path: a non-UTF-8 line must not stop the
    /// stream, and Exited must come after the last line.
    #[test]
    fn streams_past_invalid_utf8_and_exits_last() {
        use super::{spawn_streaming, AppEvent};
        use std::time::Duration;

        let dir = std::env::temp_dir().join(format!("winsploit-test-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let file = dir.join("out.txt");
        std::fs::write(&file, b"caf\xe9\r\nsecond\r\nthird\r\n").unwrap();

        let (tx, rx) = std::sync::mpsc::channel();
        let mut cmd = std::process::Command::new("cmd");
        cmd.args(["/D", "/C", "type"]).arg(&file);
        spawn_streaming(cmd, tx, false, 0).unwrap();

        let mut lines = Vec::new();
        loop {
            match rx
                .recv_timeout(Duration::from_secs(10))
                .expect("runner stalled")
            {
                AppEvent::Output { line, .. } => lines.push(line),
                AppEvent::Exited { code, .. } => {
                    assert_eq!(code, Some(0));
                    break;
                }
                AppEvent::Detected(_)
                | AppEvent::Web(..)
                | AppEvent::Lan(_)
                | AppEvent::Agent(_)
                | AppEvent::Chat { .. }
                | AppEvent::Update(_) => {}
            }
        }
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(lines, ["caf\u{fffd}", "second", "third"]);
    }

    #[test]
    fn plain_and_crlf() {
        assert_eq!(
            clean_line(b"PORT   STATE SERVICE\r\n"),
            "PORT   STATE SERVICE"
        );
        assert_eq!(clean_line(b"no newline"), "no newline");
        assert_eq!(clean_line(b"\r\n"), "");
    }

    #[test]
    fn invalid_utf8_is_replaced_not_fatal() {
        // "caf\xe9" in cp1252
        assert_eq!(clean_line(b"caf\xe9 ok\n"), "caf\u{fffd} ok");
    }

    #[test]
    fn progress_bar_keeps_last_frame() {
        assert_eq!(clean_line(b"10%\r20%\r30% done\n"), "30% done");
        assert_eq!(clean_line(b"working...\r\r\n"), "working...");
    }

    #[test]
    fn ansi_sequences_are_stripped() {
        assert_eq!(
            clean_line(b"\x1b[1;32m[INF]\x1b[0m found \x1b[34mhttps://x\x1b[0m\n"),
            "[INF] found https://x"
        );
        assert_eq!(clean_line(b"\x1b]0;title\x07after"), "after");
        assert_eq!(
            clean_line(b"\x1b]8;;http://x\x1b\\link\x1b]8;;\x1b\\"),
            "link"
        );
        assert_eq!(clean_line(b"\x1b(Bplain\x1b[K"), "plain");
    }

    #[test]
    fn tabs_and_controls() {
        assert_eq!(clean_line(b"a\tb\x07\x08c"), "a    bc");
    }
}
