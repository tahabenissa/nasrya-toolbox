//! One terminal in the dashboard: a process's output, its input pipe, and the
//! terminal's own scroll and search position.
//!
//! The dashboard has an always-open console (installs, hash ids, messages)
//! plus up to `SLOTS` tool terminals, each running one tool independently.

use std::cell::Cell;
use std::io::Write;
use std::path::PathBuf;
use std::process::ChildStdin;
use std::time::{Duration, Instant};

use crate::redact::{self, Level};
use crate::runlog::RunLog;
use crate::runner;

/// Session id of the console; tool terminals get ids from 1 up.
pub const CONSOLE: u64 = 0;
/// How many tools can run side by side.
pub const SLOTS: usize = 4;
/// Output lines kept in memory per terminal (the run log keeps everything).
const MAX_LINES: usize = 2000;

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum RunState {
    /// nothing has run here yet
    Idle,
    Running,
    /// finished on its own, with this exit code
    Exited(Option<i32>),
    /// stopped with `c` (or replaced)
    Killed,
}

pub struct Session {
    /// Routes runner events here; never reused, so a replaced terminal's
    /// late output can't land in its successor.
    pub id: u64,
    pub title: String,
    pub subtitle: String,
    pub output: Vec<String>,
    /// Output rows scrolled up from the bottom (0 = follow new output).
    pub scroll: usize,
    /// Set by the renderer: (visible rows, max useful scroll, row width).
    pub view: Cell<(usize, usize, usize)>,
    /// Output line the last search matched.
    pub search_hit: Option<usize>,
    pub pid: Option<u32>,
    pub state: RunState,
    stdin: Option<ChildStdin>,
    log: Option<RunLog>,
    kill_requested: bool,
    /// New output arrived while another terminal was in front.
    pub unread: bool,
    /// When it finished (a counter, not a clock): the oldest finished
    /// terminal is the one reused when all slots are taken.
    pub ended: u64,
    /// Lines ever pushed (unlike `output.len()`, never trimmed): the web
    /// view sends what it hasn't seen yet.
    pub seq: u64,
    /// The run log of the last run here (kept after it's closed, so the web
    /// view can offer it for download).
    pub log_path: Option<PathBuf>,
    /// When the current run started (for the finish notification, #113).
    started_at: Option<Instant>,
    /// How long the last finished run took.
    pub last_duration: Option<Duration>,
    /// What to keep out of the pane and the log (#70); set from the settings
    /// when a tool starts.
    pub redact: Level,
    /// The last line the tool printed asked for a secret, so the next line
    /// typed is masked.
    awaiting_secret: bool,
}

impl Session {
    pub fn new(id: u64, title: impl Into<String>, subtitle: impl Into<String>) -> Self {
        Session {
            id,
            title: title.into(),
            subtitle: subtitle.into(),
            output: Vec::new(),
            scroll: 0,
            view: Cell::new((10, 0, 80)),
            search_hit: None,
            pid: None,
            state: RunState::Idle,
            stdin: None,
            log: None,
            kill_requested: false,
            unread: false,
            ended: 0,
            seq: 0,
            log_path: None,
            started_at: None,
            last_duration: None,
            redact: Level::default(),
            awaiting_secret: false,
        }
    }

    pub fn running(&self) -> bool {
        self.state == RunState::Running
    }

    pub fn has_stdin(&self) -> bool {
        self.stdin.is_some()
    }

    /// Did the last process here fail (non-zero exit, or killed)?
    pub fn failed(&self) -> bool {
        matches!(self.state, RunState::Killed | RunState::Exited(Some(1..)))
            || matches!(self.state, RunState::Exited(Some(c)) if c < 0)
    }

    pub fn push(&mut self, line: impl Into<String>) {
        let line = line.into();
        if line.starts_with("$ ") {
            // a new command: jump back to the live tail
            self.scroll = 0;
        } else if self.scroll > 0 {
            // scrolled up: keep the view still while output keeps coming
            self.scroll += 1;
        }
        self.output.push(line);
        self.seq += 1;
        if self.output.len() > MAX_LINES {
            let excess = self.output.len() - MAX_LINES;
            self.output.drain(0..excess);
            self.search_hit = self.search_hit.and_then(|h| h.checked_sub(excess));
        }
    }

    /// A line from the process: logged, then shown. The log gets secret
    /// patterns masked when `redact` is `all`; the pane shows the line as is.
    pub fn output_line(&mut self, line: String) {
        self.awaiting_secret = redact::is_secret_prompt(&line);
        if let Some(log) = self.log.as_mut() {
            if self.redact == Level::All {
                log.line(&redact::text(&line));
            } else {
                log.line(&line);
            }
        }
        self.push(line);
    }

    /// What the pane and the log show for a line the user typed: the line,
    /// or `********` when the tool just asked for a password.
    fn input_echo(&self, line: &str) -> String {
        if self.awaiting_secret && self.redact != Level::Off {
            format!("> {}", redact::MASK)
        } else {
            format!("> {line}")
        }
    }

    /// Start this run's log file and say where it is. `source` records
    /// where the run was started from (`terminal` or `web`).
    /// `dir` is the active project's / challenge's `logs\` folder, or the default.
    pub fn open_log(&mut self, dir: &std::path::Path, name: &str, command: &str, source: &str) {
        match RunLog::create(dir, name, command, source) {
            Ok(log) => {
                self.push(format!(":: log: {}", log.path().display()));
                self.log_path = Some(log.path().to_path_buf());
                self.log = Some(log);
            }
            Err(e) => self.push(format!("[err] couldn't create a log file: {e}")),
        }
    }

    /// A process was started in this terminal.
    pub fn started(&mut self, pid: u32, stdin: Option<ChildStdin>) {
        self.pid = Some(pid);
        self.stdin = stdin;
        self.state = RunState::Running;
        self.kill_requested = false;
        self.started_at = Some(Instant::now());
    }

    /// The process finished; `seq` orders finished terminals by age.
    pub fn exited(&mut self, code: Option<i32>, seq: u64) {
        self.pid = None;
        self.stdin = None;
        self.last_duration = self.started_at.take().map(|t| t.elapsed());
        if let Some(log) = self.log.take() {
            log.finish(code);
        }
        self.ended = seq;
        if self.kill_requested {
            self.state = RunState::Killed;
            self.push("── process killed ──");
        } else {
            self.state = RunState::Exited(code);
            match code {
                Some(c) => self.push(format!("── process exited (code {c}) ──")),
                None => self.push("── process ended ──"),
            }
        }
    }

    /// Kill the running process tree, if any. Returns its pid.
    pub fn kill(&mut self) -> Option<u32> {
        self.stdin = None;
        let pid = self.pid?;
        self.kill_requested = true;
        runner::kill_tree(pid);
        Some(pid)
    }

    /// Send one line to the process's stdin (echoed in the terminal).
    pub fn send(&mut self, line: &str) {
        let Some(stdin) = self.stdin.as_mut() else {
            self.push("[err] the tool's input is closed");
            return;
        };
        let res = stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush());
        match res {
            Ok(()) => {
                let echo = self.input_echo(line);
                self.output_line(echo);
            }
            Err(e) => {
                self.stdin = None;
                self.push(format!("[err] couldn't send input: {e}"));
            }
        }
    }

    /// Close the process's stdin so it sees end-of-input.
    pub fn close_stdin(&mut self) {
        self.stdin = None;
        self.push(":: closed the tool's input (EOF)");
    }

    pub fn page_up(&mut self) {
        let (page, max, _) = self.view.get();
        self.scroll = (self.scroll + page.saturating_sub(1).max(1)).min(max);
    }

    pub fn page_down(&mut self) {
        let (page, _, _) = self.view.get();
        self.scroll = self.scroll.saturating_sub(page.saturating_sub(1).max(1));
    }

    pub fn scroll_top(&mut self) {
        self.scroll = self.view.get().1;
    }

    pub fn follow(&mut self) {
        self.scroll = 0;
    }

    /// Jump to the next line containing `needle` (case-insensitive):
    /// `older` = further up, otherwise further down; wraps around. Returns
    /// the number of matching lines, or None when there is no match.
    pub fn find_next(&mut self, needle: &str, older: bool) -> Option<usize> {
        let needle = needle.trim().to_lowercase();
        let n = self.output.len();
        if needle.is_empty() || n == 0 {
            return None;
        }
        let start = self.search_hit.unwrap_or(n); // first search: from the bottom
        let hit = (1..=n)
            .map(|k| {
                if older {
                    (start + n - k) % n
                } else {
                    (start + k) % n
                }
            })
            .find(|&i| self.output[i].to_lowercase().contains(&needle));
        match hit {
            Some(i) => {
                self.search_hit = Some(i);
                self.scroll_to_line(i);
                Some(
                    self.output
                        .iter()
                        .filter(|l| l.to_lowercase().contains(&needle))
                        .count(),
                )
            }
            None => {
                self.search_hit = None;
                None
            }
        }
    }

    /// Scroll so output line `i` sits in the middle of the pane.
    pub fn scroll_to_line(&mut self, i: usize) {
        let (height, _, width) = self.view.get();
        let width = width.max(1);
        let rows_after: usize = self.output[i + 1..]
            .iter()
            .map(|l| l.chars().count().div_ceil(width).max(1))
            .sum();
        let rows_of_hit = self.output[i].chars().count().div_ceil(width).max(1);
        // put the hit's first row about mid-pane
        self.scroll = (rows_after + rows_of_hit).saturating_sub(height / 2 + 1);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn with(lines: &[&str]) -> Session {
        let mut s = Session::new(1, "t", "");
        s.output = lines.iter().map(|s| s.to_string()).collect();
        s.view.set((4, 0, 80));
        s
    }

    #[test]
    fn find_starts_at_the_newest_match_and_wraps() {
        let mut s = with(&["open 22", "x", "OPEN 80", "y", "open 443", "z"]);
        assert_eq!(s.find_next("open", true), Some(3));
        assert_eq!(s.search_hit, Some(4)); // newest first
        s.find_next("open", true);
        assert_eq!(s.search_hit, Some(2)); // older, case-insensitive
        s.find_next("open", true);
        assert_eq!(s.search_hit, Some(0));
        s.find_next("open", true);
        assert_eq!(s.search_hit, Some(4)); // wraps to the newest
        s.find_next("open", false);
        assert_eq!(s.search_hit, Some(0)); // newer, wraps the other way
    }

    /// A session with a real log in a temp dir; returns it and the log text reader.
    fn logged(level: Level, tag: &str) -> (Session, std::path::PathBuf, std::path::PathBuf) {
        let dir =
            std::env::temp_dir().join(format!("winsploit-redact-{tag}-{}", std::process::id()));
        let mut s = Session::new(1, "t", "");
        s.redact = level;
        s.open_log(&dir, "tool", "tool -x", "terminal");
        let path = s.log_path.clone().unwrap();
        (s, dir, path)
    }

    #[test]
    fn what_is_typed_at_a_password_prompt_is_masked() {
        let mut s = Session::new(1, "t", "");
        assert_eq!(s.input_echo("hunter2"), "> hunter2"); // no prompt yet
        s.output_line("Password:".into());
        assert_eq!(s.input_echo("hunter2"), "> ********");
        // any later line (the echo itself, or output) ends the prompt
        s.output_line("> ********".into());
        assert_eq!(s.input_echo("ls"), "> ls");
        s.output_line("Enter passphrase for key:".into());
        s.output_line("Permission denied".into());
        assert_eq!(s.input_echo("x"), "> x");
        // off means off
        s.redact = Level::Off;
        s.output_line("Password:".into());
        assert_eq!(s.input_echo("hunter2"), "> hunter2");
    }

    #[test]
    fn the_log_follows_the_redaction_level() {
        let line = "Authorization: Bearer abcdefgh12345 and password=hunter2";
        for (level, masked) in [
            (Level::Off, false),
            (Level::Commands, false),
            (Level::All, true),
        ] {
            let (mut s, dir, path) = logged(level, &format!("{level:?}"));
            s.output_line(line.into());
            let masked_log = {
                let text = std::fs::read_to_string(&path).unwrap();
                let _ = std::fs::remove_dir_all(&dir);
                text
            };
            assert_eq!(
                !masked_log.contains("hunter2"),
                masked,
                "{level:?}: {masked_log}"
            );
            assert_eq!(!masked_log.contains("abcdefgh12345"), masked, "{level:?}");
            // the pane always keeps the tool's output as printed
            assert_eq!(s.output.last().map(String::as_str), Some(line));
        }
    }

    #[test]
    fn find_reports_no_match() {
        let mut s = with(&["a", "b"]);
        assert_eq!(s.find_next("zzz", true), None);
        assert_eq!(s.search_hit, None);
    }

    #[test]
    fn hit_is_scrolled_into_the_middle() {
        let lines: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let mut s = with(&refs);
        s.scroll_to_line(10);
        // 9 rows after line 10, plus its own row, minus half a 4-row page
        assert_eq!(s.scroll, 10 - 3);
        let (rows, _) = crate::ui::visible_rows(&s.output, 80, 4, s.scroll);
        assert!(rows.iter().any(|(i, _)| *i == 10));
        // the newest line needs no scrolling
        s.scroll_to_line(19);
        assert_eq!(s.scroll, 0);
    }

    #[test]
    fn exit_states() {
        let mut s = Session::new(1, "t", "");
        assert!(!s.failed());
        s.started(42, None);
        assert!(s.running());
        s.exited(Some(0), 5);
        assert_eq!(s.state, RunState::Exited(Some(0)));
        assert!(!s.failed());
        assert_eq!(s.ended, 5);
        s.started(43, None);
        s.exited(Some(2), 6);
        assert!(s.failed());
        assert!(s.output.last().unwrap().contains("code 2"));
    }
}
