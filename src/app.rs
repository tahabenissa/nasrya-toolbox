//! Application state and the main event loop.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::Duration;

use anyhow::Result;
use ratatui::crossterm::event::{self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers};
use ratatui::DefaultTerminal;

use crate::args;
use crate::banner;
use crate::detect;
use crate::editor::{History, LineEditor};
use crate::envpath;
use crate::hashid;
use crate::install;
use crate::registry::{Catalog, Install, Launch, Tool};
use crate::runlog;
use crate::runner::{self, AppEvent};
use crate::scoop;
use crate::ui;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Unknown,
    Installed,
    Missing,
    /// the exe on PATH failed its `verify` check (a same-named program)
    Conflict,
}

#[derive(PartialEq, Eq)]
pub enum Mode {
    Browsing,
    /// editing the selected tool's arguments
    Input,
    /// typing a hash for the built-in identifier
    HashInput,
    /// typing a line for the running process's stdin
    StdinInput,
    /// typing a filter for the tool list (applied live)
    Filter,
    /// typing text to find in the output
    Search,
}

#[derive(PartialEq, Eq)]
enum Screen {
    Splash,
    Main,
}

pub struct App {
    pub tools: Vec<Tool>,
    buckets: BTreeMap<String, String>,
    pub status: Vec<ToolStatus>,
    /// For tools in Conflict: the path of the program that's actually found.
    conflicts: Vec<Option<PathBuf>>,
    pub selected: usize,
    /// Tool-list filter (empty = show everything).
    pub filter: String,
    pub output: Vec<String>,
    /// Output rows scrolled up from the bottom (0 = follow new output).
    pub scroll: usize,
    /// Set by the renderer: (visible rows, max useful scroll, row width).
    pub view: std::cell::Cell<(usize, usize, usize)>,
    /// Last output search, and the output line it matched.
    search: String,
    pub search_hit: Option<usize>,
    pub mode: Mode,
    pub editor: LineEditor,
    history: History,
    pub running: bool,
    pub running_pid: Option<u32>,
    /// Log file of the current run / install.
    log: Option<runlog::RunLog>,
    /// stdin of the running tool (None for installs, or after Ctrl+D)
    proc_stdin: Option<std::process::ChildStdin>,
    pub status_line: String,
    pub tick: u64,
    /// The '?' key overlay is open.
    pub show_help: bool,
    /// A background detection pass is in flight.
    detecting: bool,
    /// The running process is a scoop install (so re-detect when it exits).
    installing: bool,
    screen: Screen,
    tx: Sender<AppEvent>,
    rx: Receiver<AppEvent>,
    quit: bool,
}

impl App {
    pub fn new(catalog: Catalog, catalog_path: PathBuf, ignored: Option<PathBuf>) -> Self {
        let Catalog { tools, buckets } = catalog;
        let (tx, rx) = mpsc::channel();
        let n = tools.len();
        let mut output = vec![
            ":: winsploit ready. tools above; output appears here.".into(),
            ":: Enter runs the highlighted tool · i installs it (scoop or pipx).".into(),
            format!(":: catalog: {}", catalog_path.display()),
        ];
        if let Some(ignored) = ignored {
            output.push(format!(
                "[err] ignored {} — catalogs aren't loaded from the current directory \
                 unless you pass --catalog",
                ignored.display()
            ));
        }
        App {
            tools,
            buckets,
            status: vec![ToolStatus::Unknown; n],
            conflicts: vec![None; n],
            selected: 0,
            filter: String::new(),
            output,
            scroll: 0,
            view: std::cell::Cell::new((10, 0, 80)),
            search: String::new(),
            search_hit: None,
            mode: Mode::Browsing,
            editor: LineEditor::default(),
            history: History::default(),
            running: false,
            running_pid: None,
            proc_stdin: None,
            log: None,
            status_line: "detecting tools…".into(),
            tick: 0,
            show_help: false,
            detecting: false,
            installing: false,
            screen: Screen::Splash,
            tx,
            rx,
            quit: false,
        }
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        self.detect_all(false);
        while !self.quit {
            terminal.draw(|f| match self.screen {
                Screen::Splash => banner::render_splash(f, self.tick),
                Screen::Main => ui::render(f, &self),
            })?;

            // drain runner events
            while let Ok(ev) = self.rx.try_recv() {
                self.on_runner_event(ev);
            }

            if event::poll(Duration::from_millis(80))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => match self.screen {
                        Screen::Splash => self.screen = Screen::Main,
                        Screen::Main => self.on_key(key),
                    },
                    // bracketed paste: the whole pasted text arrives at once
                    Event::Paste(text) if self.mode != Mode::Browsing => {
                        self.editor.insert_str(&text)
                    }
                    _ => {}
                }
            }
            self.tick = self.tick.wrapping_add(1);
        }
        Ok(())
    }

    /// Re-probe every tool on a background thread; the result comes back as
    /// `AppEvent::Detected`, so the UI keeps drawing meanwhile. With
    /// `refresh_path`, first pick up PATH changes persisted since winsploit
    /// started (installs that add a PATH entry, a freshly installed Scoop).
    fn detect_all(&mut self, refresh_path: bool) {
        if self.detecting {
            return;
        }
        self.detecting = true;
        self.status_line = "detecting tools…".into();
        let probes: Vec<(String, Option<detect::Verify>)> = self
            .tools
            .iter()
            .map(|t| (t.exe.clone(), t.verify.clone()))
            .collect();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            if refresh_path {
                for dir in envpath::refresh() {
                    let _ = tx.send(AppEvent::Output(format!(":: PATH: picked up {dir}")));
                }
            }
            let found = detect::probe_all(&probes);
            let _ = tx.send(AppEvent::Detected(found));
        });
    }

    fn on_detected(&mut self, found: Vec<detect::Found>) {
        self.detecting = false;
        self.conflicts = vec![None; found.len()];
        self.status = found
            .into_iter()
            .enumerate()
            .map(|(i, f)| match f {
                detect::Found::Installed(_) => ToolStatus::Installed,
                detect::Found::Missing => ToolStatus::Missing,
                detect::Found::Conflict(p) => {
                    self.conflicts[i] = Some(p);
                    ToolStatus::Conflict
                }
            })
            .collect();
        let count = |s: ToolStatus| self.status.iter().filter(|x| **x == s).count();
        let installed = count(ToolStatus::Installed);
        let conflicts = count(ToolStatus::Conflict);
        self.status_line = format!("{installed}/{} tools installed", self.tools.len());
        if conflicts > 0 {
            self.status_line
                .push_str(&format!(" · {conflicts} name conflict(s)"));
        }
    }

    fn push(&mut self, line: impl Into<String>) {
        let line = line.into();
        if line.starts_with("$ ") {
            // a new command: jump back to the live tail
            self.scroll = 0;
        } else if self.scroll > 0 {
            // scrolled up: keep the view still while output keeps coming
            self.scroll += 1;
        }
        self.output.push(line);
        const MAX: usize = 2000;
        if self.output.len() > MAX {
            let excess = self.output.len() - MAX;
            self.output.drain(0..excess);
            self.search_hit = self.search_hit.and_then(|h| h.checked_sub(excess));
        }
    }

    fn on_runner_event(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::Output(l) => {
                if let Some(log) = self.log.as_mut() {
                    log.line(&l);
                }
                self.push(l)
            }
            AppEvent::Exited(code) => {
                self.running = false;
                self.running_pid = None;
                self.proc_stdin = None;
                if let Some(log) = self.log.take() {
                    log.finish(code);
                }
                if self.mode == Mode::StdinInput {
                    self.mode = Mode::Browsing;
                    self.editor.set("");
                }
                match code {
                    Some(c) => self.push(format!("── process exited (code {c}) ──")),
                    None => self.push("── process ended ──".to_string()),
                }
                // Only an install can change what's on PATH; a plain tool run
                // doesn't need a re-probe.
                if std::mem::take(&mut self.installing) {
                    self.detect_all(true);
                }
            }
            AppEvent::Detected(found) => self.on_detected(found),
        }
    }

    fn on_key(&mut self, key: KeyEvent) {
        if self.show_help {
            // any key closes the overlay
            self.show_help = false;
            return;
        }
        match self.mode {
            Mode::Browsing => self.on_key_browsing(key.code),
            Mode::Input | Mode::HashInput | Mode::StdinInput | Mode::Filter | Mode::Search => {
                self.on_key_input(key)
            }
        }
    }

    /// History key for the open prompt: the tool's name, or the hash prompt.
    fn history_key(&self) -> String {
        if self.mode == Mode::HashInput {
            "\u{0}hashid".to_string()
        } else if self.mode == Mode::StdinInput {
            "\u{0}stdin".to_string()
        } else if self.mode == Mode::Search {
            "\u{0}search".to_string()
        } else {
            self.tools[self.selected].name.clone()
        }
    }

    fn on_key_browsing(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char('q') | KeyCode::Esc => self.quit = true,
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('f') => {
                self.editor.set(&self.search.clone());
                self.mode = Mode::Search;
            }
            KeyCode::Char('n') => self.find_next(true),
            KeyCode::Char('N') => self.find_next(false),
            KeyCode::Char('/') => {
                self.editor.set(&self.filter.clone());
                self.mode = Mode::Filter;
            }
            KeyCode::Char('r') => self.detect_all(true),
            KeyCode::PageUp => {
                let (page, max, _) = self.view.get();
                self.scroll = (self.scroll + page.saturating_sub(1).max(1)).min(max);
            }
            KeyCode::PageDown => {
                let (page, _, _) = self.view.get();
                self.scroll = self.scroll.saturating_sub(page.saturating_sub(1).max(1));
            }
            KeyCode::End => self.scroll = 0,
            KeyCode::Home => self.scroll = self.view.get().1,
            KeyCode::Char('i') if self.visible().contains(&self.selected) => {
                self.install_selected()
            }
            KeyCode::Char('c') => self.cancel(),
            KeyCode::Char('h') => {
                self.editor.set("");
                self.history.reset();
                self.mode = Mode::HashInput;
            }
            KeyCode::Enter | KeyCode::Char('i') if !self.visible().contains(&self.selected) => {
                self.status_line = "no tool matches the filter (press / to change it)".into();
            }
            KeyCode::Enter => {
                if self.tools[self.selected].launch == Launch::Dataset {
                    // nothing to run, so no args prompt
                    self.show_dataset();
                } else if self.running {
                    if self.proc_stdin.is_some() {
                        // talk to the running tool (answer a prompt, etc.)
                        self.editor.set("");
                        self.history.reset();
                        self.mode = Mode::StdinInput;
                    } else {
                        self.status_line =
                            "a process is already running (press c to cancel)".into();
                    }
                } else {
                    let hint = self.tools[self.selected].args_hint.clone();
                    self.editor.set(&hint.unwrap_or_default());
                    self.history.reset();
                    self.mode = Mode::Input;
                }
            }
            _ => {}
        }
    }

    fn on_key_input(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        let e = &mut self.editor;
        match key.code {
            KeyCode::Esc if self.mode == Mode::Filter => {
                // Esc in the filter prompt clears the filter
                self.editor.set("");
                self.set_filter(String::new());
                self.mode = Mode::Browsing;
            }
            KeyCode::Esc => {
                self.mode = Mode::Browsing;
                self.editor.set("");
                self.history.reset();
            }
            KeyCode::Enter if self.mode == Mode::Filter => {
                // keep the filter, back to the list
                self.editor.set("");
                self.mode = Mode::Browsing;
            }
            KeyCode::Up if self.mode == Mode::Filter => self.move_selection(-1),
            KeyCode::Down if self.mode == Mode::Filter => self.move_selection(1),
            KeyCode::Enter => {
                let text = self.editor.take();
                let hkey = self.history_key();
                self.history.push(&hkey, &text);
                let mode = std::mem::replace(&mut self.mode, Mode::Browsing);
                match mode {
                    Mode::Search => {
                        self.search = text;
                        self.search_hit = None;
                        self.find_next(true);
                    }
                    Mode::HashInput => self.identify_hash(&text),
                    Mode::StdinInput => self.send_stdin(&text),
                    _ => self.launch(&text),
                }
            }
            KeyCode::Up => {
                let hkey = self.history_key();
                let cur = self.editor.text();
                if let Some(line) = self.history.older(&hkey, &cur) {
                    self.editor.set(&line);
                }
            }
            KeyCode::Down => {
                let hkey = self.history_key();
                if let Some(line) = self.history.newer(&hkey) {
                    self.editor.set(&line);
                }
            }
            KeyCode::Left if ctrl => e.word_left(),
            KeyCode::Right if ctrl => e.word_right(),
            KeyCode::Left => e.left(),
            KeyCode::Right => e.right(),
            KeyCode::Home => e.home(),
            KeyCode::End => e.end(),
            KeyCode::Backspace if ctrl => e.delete_word_left(),
            KeyCode::Backspace => e.backspace(),
            KeyCode::Delete => e.delete(),
            KeyCode::Char('d') if ctrl && self.mode == Mode::StdinInput => {
                // end-of-input: close the pipe so the tool sees EOF
                self.proc_stdin = None;
                self.mode = Mode::Browsing;
                self.editor.set("");
                self.push(":: closed the tool's input (EOF)");
            }
            KeyCode::Char('u') if ctrl => e.clear_to_start(),
            KeyCode::Char('w') if ctrl => e.delete_word_left(),
            KeyCode::Char('a') if ctrl => e.home(),
            KeyCode::Char('e') if ctrl => e.end(),
            // other Ctrl/Alt chords aren't text
            KeyCode::Char(_) if ctrl || alt => {}
            KeyCode::Char(c) => e.insert(c),
            _ => {}
        }
        if self.mode == Mode::Filter {
            let f = self.editor.text();
            if f != self.filter {
                self.set_filter(f);
            }
        }
    }

    /// Jump to the next match of the last search: `older` = further up
    /// (n), otherwise further down (N). Case-insensitive; wraps around.
    fn find_next(&mut self, older: bool) {
        let needle = self.search.trim().to_lowercase();
        if needle.is_empty() {
            self.status_line = "nothing to find (press f)".into();
            return;
        }
        let n = self.output.len();
        if n == 0 {
            return;
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
                let total = self
                    .output
                    .iter()
                    .filter(|l| l.to_lowercase().contains(&needle))
                    .count();
                self.status_line = format!(
                    "'{}': {total} matching line(s) · n older · N newer",
                    self.search.trim()
                );
            }
            None => {
                self.search_hit = None;
                self.status_line = format!("'{}' not found in the output", self.search.trim());
            }
        }
    }

    /// Scroll so output line `i` sits in the middle of the pane.
    fn scroll_to_line(&mut self, i: usize) {
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

    /// Indices of the tools that match the current filter, in catalog order.
    pub fn visible(&self) -> Vec<usize> {
        let needle = self.filter.trim().to_lowercase();
        self.tools
            .iter()
            .enumerate()
            .filter(|(_, t)| {
                needle.is_empty()
                    || needle.split_whitespace().all(|w| {
                        t.name.to_lowercase().contains(w)
                            || t.category.to_lowercase().contains(w)
                            || t.description.to_lowercase().contains(w)
                    })
            })
            .map(|(i, _)| i)
            .collect()
    }

    fn set_filter(&mut self, f: String) {
        self.filter = f;
        let vis = self.visible();
        if !vis.is_empty() && !vis.contains(&self.selected) {
            self.selected = vis[0];
        }
    }

    /// Move the selection by `delta` among the visible tools.
    fn move_selection(&mut self, delta: i32) {
        let vis = self.visible();
        let Some(pos) = vis.iter().position(|&i| i == self.selected) else {
            if let Some(&first) = vis.first() {
                self.selected = first;
            }
            return;
        };
        let new = (pos as i32 + delta).clamp(0, vis.len() as i32 - 1) as usize;
        self.selected = vis[new];
    }

    /// Send one line to the running tool's stdin (echoed in the pane).
    fn send_stdin(&mut self, line: &str) {
        use std::io::Write;
        let Some(stdin) = self.proc_stdin.as_mut() else {
            self.push("[err] the tool's input is closed");
            return;
        };
        let res = stdin
            .write_all(line.as_bytes())
            .and_then(|_| stdin.write_all(b"\n"))
            .and_then(|_| stdin.flush());
        match res {
            Ok(()) => {
                if let Some(log) = self.log.as_mut() {
                    log.line(&format!("> {line}"));
                }
                self.push(format!("> {line}"))
            }
            Err(e) => {
                self.proc_stdin = None;
                self.push(format!("[err] couldn't send input: {e}"));
            }
        }
    }

    /// Built-in hash identifier (`h`): no install, works offline.
    fn identify_hash(&mut self, hash: &str) {
        if hash.trim().is_empty() {
            return;
        }
        self.push(format!("$ hashid {}", hash.trim()));
        for line in hashid::report(hash) {
            self.push(line);
        }
    }

    fn launch(&mut self, args_str: &str) {
        let status = self.status[self.selected];
        let name = self.tools[self.selected].name.clone();
        let cmd = self.tools[self.selected].run_cmd.clone();

        if status == ToolStatus::Unknown {
            self.status_line = "still detecting tools — try again in a moment".into();
            return;
        }
        if status == ToolStatus::Conflict {
            let path = self.conflicts[self.selected]
                .as_ref()
                .map(|p| p.display().to_string())
                .unwrap_or_default();
            self.push(format!(
                "[err] the '{}' on PATH ({path}) isn't {name} — a different program with \
                 the same name comes first. Not running it.",
                self.tools[self.selected].exe
            ));
            self.push(
                "[err] fix: press i to install the right one, or move its folder earlier in PATH \
                 (or uninstall the other program), then press r.",
            );
            return;
        }
        if status != ToolStatus::Installed {
            self.push(format!(
                "'{name}' is not installed — press i to install it first."
            ));
            return;
        }

        let args = match args::split(args_str) {
            Ok(a) => a,
            Err(e) => {
                self.push(format!("[err] can't parse args: {e} — nothing was run."));
                return;
            }
        };
        let tool = self.tools[self.selected].clone();
        if let Some(note) = &tool.note {
            self.push(format!(":: note: {note}"));
        }

        match tool.launch {
            Launch::Stream => {
                self.start_streaming(&name, &cmd, &args);
            }
            Launch::Gui => {
                self.push(format!("$ {} {}", cmd, args::join(&args)));
                match runner::spawn_detached(runner::tool_command(&cmd, &args)) {
                    Ok(pid) => {
                        self.push(format!(
                            "── launched {name} (pid {pid}); it runs in its own window ──"
                        ));
                        self.status_line = format!("launched {name}");
                    }
                    Err(e) => self.push(format!("failed to launch: {e}")),
                }
            }
            Launch::Web => {
                let url = tool.url.clone().unwrap_or_default();
                if cmd.trim().is_empty() {
                    self.open_url(&url, 0);
                } else {
                    // start the server (streamed, cancellable), then give it
                    // a moment before opening the browser
                    if self.start_streaming(&name, &cmd, &args) {
                        self.open_url(&url, 2);
                    }
                }
            }
            Launch::Dataset => self.show_dataset(),
        }
    }

    /// Enter on a dataset: list its files and copy the folder path, so it can
    /// be pasted into another tool's arguments.
    fn show_dataset(&mut self) {
        let tool = self.tools[self.selected].clone();
        match self.status[self.selected] {
            ToolStatus::Installed => {}
            ToolStatus::Unknown => {
                self.status_line = "still detecting tools — try again in a moment".into();
                return;
            }
            _ => {
                self.push(format!(
                    "'{}' is not installed — press i to install it first.",
                    tool.name
                ));
                return;
            }
        }
        let Some(dir) = std::path::Path::new(&tool.exe)
            .parent()
            .map(|d| d.to_path_buf())
        else {
            return;
        };
        self.push(format!(":: {} — {}", tool.name, dir.display()));
        let mut files: Vec<(String, u64)> = std::fs::read_dir(&dir)
            .map(|rd| {
                rd.filter_map(|e| e.ok())
                    .filter(|e| e.file_type().map(|t| t.is_file()).unwrap_or(false))
                    .map(|e| {
                        let size = e.metadata().map(|m| m.len()).unwrap_or(0);
                        (e.file_name().to_string_lossy().to_string(), size)
                    })
                    .filter(|(n, _)| {
                        !n.eq_ignore_ascii_case("manifest.json")
                            && !n.eq_ignore_ascii_case("install.json")
                    })
                    .collect()
            })
            .unwrap_or_default();
        files.sort();
        let w = files
            .iter()
            .map(|(n, _)| n.chars().count())
            .max()
            .unwrap_or(0);
        for (n, size) in files {
            self.push(format!("   {n:<w$}  {:>8.1} KB", size as f64 / 1024.0));
        }
        match runner::copy_to_clipboard(&dir.to_string_lossy()) {
            Ok(()) => self.push(":: folder path copied to the clipboard"),
            Err(e) => self.push(format!("[err] couldn't copy the path: {e}")),
        }
    }

    /// Run `cmd args` with output streamed into the pane. Returns whether it
    /// started.
    fn start_streaming(&mut self, name: &str, cmd: &str, args: &[String]) -> bool {
        let shown = format!("{} {}", cmd, args::join(args));
        self.push(format!("$ {shown}"));
        match runner::spawn_streaming(runner::tool_command(cmd, args), self.tx.clone(), true) {
            Ok(sp) => {
                self.open_log(name, &shown);
                let pid = sp.pid;
                self.proc_stdin = sp.stdin;
                self.running = true;
                self.running_pid = Some(pid);
                self.status_line = format!("running {name} (pid {pid})");
                true
            }
            Err(e) => {
                self.push(format!("failed to launch: {e}"));
                false
            }
        }
    }

    /// Start this run's log file and say where it is.
    fn open_log(&mut self, name: &str, command: &str) {
        match runlog::RunLog::create(&runlog::log_dir(), name, command) {
            Ok(log) => {
                self.push(format!(":: log: {}", log.path().display()));
                self.log = Some(log);
            }
            Err(e) => self.push(format!("[err] couldn't create a log file: {e}")),
        }
    }

    /// Open `url` in the default browser after `delay_s` seconds.
    fn open_url(&mut self, url: &str, delay_s: u64) {
        self.push(format!(":: opening {url} in your browser"));
        let url = url.to_string();
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(delay_s));
            if let Err(e) = runner::open_url(&url) {
                let _ = tx.send(AppEvent::Output(format!("[err] couldn't open {url}: {e}")));
            }
        });
    }

    /// Is there a real `python`? The Microsoft Store puts a stub
    /// `python.exe` on PATH that only prints "Python was not found" (exit
    /// 9009), so resolving the name isn't enough; ask it for its version.
    fn python_works() -> bool {
        detect::resolve("python").is_some()
            && std::process::Command::new("python")
                .arg("--version")
                .stdin(std::process::Stdio::null())
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .status()
                .map(|s| s.success())
                .unwrap_or(false)
    }

    fn install_selected(&mut self) {
        if self.running {
            self.status_line = "wait for the current process to finish".into();
            return;
        }
        let tool = self.tools[self.selected].clone();
        let name = tool.name.clone();

        let pipx = detect::resolve("pipx").is_some();
        let pre = install::Prereqs {
            scoop: detect::resolve("scoop").is_some(),
            pipx,
            // only worth the subprocess when pipx has to be bootstrapped
            python: tool.install == Install::Pipx && !pipx && Self::python_works(),
            bucket_added: tool.bucket.as_deref().is_some_and(scoop::bucket_added),
        };
        let steps = match install::plan(&tool, pre, &self.buckets) {
            Ok(s) => s,
            Err(e) => {
                self.push(format!("[err] can't install {name}: {e}"));
                return;
            }
        };
        let line = steps.join(" && ");
        self.push(format!("$ {line}"));

        // scoop and pipx are .cmd/.bat shims, so go through cmd. Every token
        // in `line` was checked by install::plan to be free of cmd
        // metacharacters, and `&&` stops at the first failing step. /D skips
        // any AutoRun commands from the registry.
        let mut cmd = std::process::Command::new("cmd");
        {
            use std::os::windows::process::CommandExt;
            cmd.raw_arg(format!("/D /C {line}"));
        }
        match runner::spawn_streaming(cmd, self.tx.clone(), false) {
            Ok(sp) => {
                self.open_log(&format!("install-{name}"), &line);
                self.running = true;
                self.running_pid = Some(sp.pid);
                self.installing = true;
                self.status_line = format!("installing {name}…");
            }
            Err(e) => self.push(format!("failed to start the installer: {e}")),
        }
    }

    fn cancel(&mut self) {
        self.proc_stdin = None;
        if let Some(pid) = self.running_pid {
            runner::kill_tree(pid);
            self.push(format!("sent kill to pid {pid}"));
        } else {
            self.status_line = "nothing is running".into();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app_with(lines: &[&str]) -> App {
        let cat = crate::registry::parse(include_str!("../tools.yaml")).unwrap();
        let mut a = App::new(cat, PathBuf::from("tools.yaml"), None);
        a.output = lines.iter().map(|s| s.to_string()).collect();
        a.view.set((4, 0, 80));
        a
    }

    #[test]
    fn find_starts_at_the_newest_match_and_wraps() {
        let mut a = app_with(&["open 22", "x", "OPEN 80", "y", "open 443", "z"]);
        a.search = "open".into();
        a.find_next(true);
        assert_eq!(a.search_hit, Some(4)); // newest first
        a.find_next(true);
        assert_eq!(a.search_hit, Some(2)); // n = older, case-insensitive
        a.find_next(true);
        assert_eq!(a.search_hit, Some(0));
        a.find_next(true);
        assert_eq!(a.search_hit, Some(4)); // wraps to the newest
        a.find_next(false);
        assert_eq!(a.search_hit, Some(0)); // N = newer, wraps the other way
    }

    #[test]
    fn find_reports_no_match() {
        let mut a = app_with(&["a", "b"]);
        a.search = "zzz".into();
        a.find_next(true);
        assert_eq!(a.search_hit, None);
        assert!(a.status_line.contains("not found"));
    }

    #[test]
    fn hit_is_scrolled_into_the_middle() {
        let lines: Vec<String> = (0..20).map(|i| format!("line {i}")).collect();
        let refs: Vec<&str> = lines.iter().map(String::as_str).collect();
        let mut a = app_with(&refs);
        a.scroll_to_line(10);
        // 9 rows after line 10, plus its own row, minus half a 4-row page
        assert_eq!(a.scroll, 10 - 3);
        let (rows, _) = crate::ui::visible_rows(&a.output, 80, 4, a.scroll);
        assert!(rows.iter().any(|(i, _)| *i == 10));
        // the newest line needs no scrolling
        a.scroll_to_line(19);
        assert_eq!(a.scroll, 0);
    }
}
