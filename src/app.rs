//! Application state and the main event loop.
//!
//! Tools run in up to `SLOTS` independent terminals (plus the console, which
//! holds installs, hash ids and messages); the tool list is browsed by
//! category, and a tool can sit in several categories.

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::mpsc::{self, Receiver, Sender};
use std::time::{Duration, Instant};

use anyhow::Result;
use ratatui::crossterm::event::{
    self, Event, KeyCode, KeyEvent, KeyEventKind, KeyModifiers, MouseButton, MouseEvent,
    MouseEventKind,
};
use ratatui::layout::Rect;
use ratatui::DefaultTerminal;

use crate::agent;
use crate::args;
use crate::banner;
use crate::chat::{self, Chat, Msg as ChatMsg, Plan, PlanState, Role as ChatRole};
use crate::detect;
use crate::editor::{History, LineEditor};
use crate::envpath;
use crate::firewall;
use crate::hashid;
use crate::install;
use crate::registry::{self, Catalog, Install, Launch, Sandbox, Tool};
use crate::runner::{self, AppEvent};
use crate::scoop;
use crate::session::{RunState, Session, CONSOLE, SLOTS};
use crate::theme::{Field, Settings};
use crate::tls;
use crate::ui;
use crate::web::{self, Caller, GuestStatus, LanEvent, Role, WebAction};
use crate::workspace::{self, Workspace};

/// Something the user asked the session to do. Terminal keys and the web
/// view both end up here (`App::apply`), so a feature is written once.
/// Terminals are positions: 0 = console, 1..=SLOTS = tool terminals.
///
/// Adding a variant? Give it a `kind`, a terminal key in `BINDINGS` (and the
/// `?` overlay), and a `data-action="<kind>"` control in src/web/index.html.
/// The `every_action_has_a_key_and_a_web_control` test checks all three.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    /// Run tool `tool` (an index into `tools`). `slot` (1..=SLOTS) is the
    /// terminal to replace when all are busy; None takes a free one.
    Run {
        tool: usize,
        args: String,
        slot: Option<usize>,
    },
    Install(usize),
    /// Stop the tool in this terminal.
    Cancel(usize),
    /// Send a line to the tool in this terminal.
    Stdin(usize, String),
    /// Close that tool's input.
    Eof(usize),
    /// Close a tool terminal (stopping its tool).
    Close(usize),
    Redetect,
    HashId(String),
    /// Make the challenge with this id (`<event>/<challenge>`) the active one.
    Challenge(String),
    /// LAN view: let guest `id` in with `role` (or change their role).
    Admit(u64, Role),
    /// LAN view: deny a waiting guest, or remove one who is in.
    Remove(u64),
    /// LAN view: a new one-time join key.
    NewKey,
    /// Copy this terminal's output to the clipboard (only the lines that
    /// match the find, while one is active there). Terminal only: the web
    /// page copies into the browser's own clipboard.
    Copy(usize),
}

// kind() and BINDINGS exist for the parity test (and as the checklist above)
#[cfg_attr(not(test), allow(dead_code))]
impl Action {
    pub fn kind(&self) -> &'static str {
        match self {
            Action::Run { .. } => "run",
            Action::Install(_) => "install",
            Action::Cancel(_) => "cancel",
            Action::Stdin(..) => "stdin",
            Action::Eof(_) => "eof",
            Action::Close(_) => "close",
            Action::Redetect => "redetect",
            Action::HashId(_) => "hashid",
            Action::Challenge(_) => "challenge",
            Action::Admit(..) => "admit",
            Action::Remove(_) => "remove",
            Action::NewKey => "newkey",
            Action::Copy(_) => "copy",
        }
    }

    /// Each action kind and the terminal key that triggers it (as written
    /// in the `?` overlay).
    pub const BINDINGS: &'static [(&'static str, &'static str)] = &[
        ("run", "Enter"),
        ("install", "i"),
        ("cancel", "c"),
        ("stdin", "s"),
        ("eof", "Ctrl+D"),
        ("close", "x"),
        ("redetect", "r"),
        ("hashid", "h"),
        ("challenge", "C"),
        // guests are managed in the L panel (v / f / x / n there)
        ("admit", "L"),
        ("remove", "L"),
        ("newkey", "L"),
        ("copy", "y"),
    ];
}

/// Where an action came from (shown in the output and the run log).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Source {
    Terminal,
    /// the owner's browser, or a LAN guest's
    Web(Caller),
}

impl Source {
    pub fn is_web(self) -> bool {
        matches!(self, Source::Web(_))
    }
}

/// How the session starts: ask on the splash screen, or skip the question.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StartMode {
    Ask,
    Terminal,
    /// start the web view and open it in the browser
    Web,
}

/// The two choices of the start-up menu.
pub const START_CHOICES: [(&str, &str); 2] = [
    ("Continue in terminal", "the keyboard dashboard, right here"),
    (
        "Open in browser",
        "the same live session in your browser; this terminal stays in charge",
    ),
];

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Unknown,
    Installed,
    Missing,
    /// the exe on PATH failed its `verify` check (a same-named program)
    Conflict,
}

#[derive(PartialEq, Eq, Debug)]
pub enum Mode {
    Browsing,
    /// editing the selected tool's arguments
    Input,
    /// typing a hash for the built-in identifier
    HashInput,
    /// typing a line for the focused terminal's stdin
    StdinInput,
    /// typing a filter for the tool list (applied live)
    Filter,
    /// typing text to find in the output
    Search,
    /// every terminal is busy: pick one (1-4) to replace, or Esc
    SlotPick,
    /// typing the sample (file or folder) to open in Windows Sandbox
    SandboxInput,
    /// typing the working folder, like `cd` (Tab completes)
    FolderInput,
    /// creating or editing a challenge, one field at a time
    Challenge(ChallengeField),
    /// a value a recipe needs that isn't known yet (target, file, wordlist)
    Recipe(crate::recipe::Slot),
}

/// The fields the challenge prompts ask for, in wizard order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChallengeField {
    Event,
    Name,
    Target,
    Flag,
    File,
}

impl Mode {
    /// Modes that type into the line editor.
    pub fn is_prompt(&self) -> bool {
        !matches!(self, Mode::Browsing | Mode::SlotPick)
    }
}

/// One tab of the category bar.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum CatTab {
    All,
    Installed,
    Cat(String),
}

impl CatTab {
    pub fn label(&self) -> String {
        match self {
            CatTab::All => "All".into(),
            CatTab::Installed => "Installed".into(),
            CatTab::Cat(id) => registry::category_label(id),
        }
    }
}

/// A second press is needed for these.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Confirm {
    /// close terminal N (1-based) although its tool is running
    Close(usize),
    /// quit although tools are running
    Quit,
}

/// The overlays, in the order keys reach them (see `App::layer`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Layer {
    Tour,
    /// the installer's packs: yes / no is left to the keyboard
    FirstRun,
    Join,
    Lan,
    Appearance,
    Flags,
    Challenges,
    Recipes,
    Provider,
    Agent,
    Help,
}

/// What the renderer drew at a spot, for the mouse. `ui::render` records
/// these in `App::hits` every frame; `App::on_mouse` looks them up.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Hit {
    /// the tool list's box (the wheel scrolls it)
    ToolList,
    /// a row of the tool list: this tool (an index into `tools`)
    Tool(usize),
    /// a category chip (an index into `cat_tabs`)
    Category(usize),
    /// a terminal's tab or its pane (0 = console)
    Term(usize),
    /// a key hint: clicking it presses the key
    Key(KeyCode),
    /// the "press again" prompt in the footer: clicking it confirms
    Confirm,
    /// the copy button on terminal `pos`'s title bar
    Copy(usize),
    /// an open overlay's box
    Popup(Layer),
    /// entry `n` of an overlay's list
    Row(Layer, usize),
}

/// Two clicks on the same cell this close together are a double-click.
const DOUBLE_CLICK: Duration = Duration::from_millis(400);

/// A run at or past this is "long": worth a sound when it finishes unfocused (#113).
const LONG_RUN: Duration = Duration::from_secs(30);

/// Human-friendly run time: `45s`, `4m12s`, `1h03m`.
fn fmt_duration(d: Duration) -> String {
    let s = d.as_secs();
    if s < 60 {
        format!("{s}s")
    } else if s < 3600 {
        format!("{}m{:02}s", s / 60, s % 60)
    } else {
        format!("{}h{:02}m", s / 3600, (s % 3600) / 60)
    }
}

/// Ring the terminal bell (BEL) — a cheap, dependency-free notification.
fn ring_bell() {
    use std::io::Write;
    let mut out = std::io::stdout();
    let _ = out.write_all(b"\x07");
    let _ = out.flush();
}

/// State of the `t` appearance overlay.
pub struct Appearance {
    /// highlighted row, an index into `theme::Field::ALL`
    pub cursor: usize,
    /// what to restore on Esc
    pub saved: Settings,
}

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum ToastKind {
    Ok,
    Err,
    Info,
}

/// A short note in the corner that fades on its own.
pub struct Toast {
    pub text: String,
    pub kind: ToastKind,
    pub born: Instant,
}

/// A flag the catcher found in some tool's output.
#[derive(Debug, Clone)]
pub struct Flag {
    pub text: String,
    /// A known prefix (`picoCTF{`), not a generic `WORD{` guess.
    pub known: bool,
    /// The terminal's title (tool name, or "console").
    pub source: String,
    /// `2026-11-07 17:04:21 UTC`
    pub at: String,
}

/// "Someone wants to join" in the terminal: let them in to watch, with full
/// control, or deny.
pub struct JoinPrompt {
    pub guest: web::GuestInfo,
    /// 0 watch only, 1 full control, 2 deny
    pub cursor: usize,
    /// When it appeared: keys in the first moment are ignored, so a fast
    /// `j j Enter` meant for the dashboard can't answer it.
    pub since: Instant,
}

/// The join prompt's choices.
pub const JOIN_CHOICES: [(&str, &str); 3] = [
    ("Watch only", "sees everything live, can't act"),
    ("Full control", "run, stop, install, type into tools"),
    ("Deny", "the key is used up; they'd need a new one"),
];

/// Keys are ignored this long after the join prompt appears.
pub const JOIN_GRACE: Duration = Duration::from_millis(700);

/// How long a toast stays up, and how many stack at once.
pub const TOAST_LIFE: Duration = Duration::from_secs(4);
pub const MAX_TOASTS: usize = 3;

/// Steps of the first-run welcome tour.
pub const TOUR_STEPS: usize = 3;

/// The agent overlay: the AI's analysis of the output and a cursor over its
/// suggested next steps.
pub struct AgentView {
    pub analysis: agent::Analysis,
    /// highlighted suggestion (ignored when there are none)
    pub cursor: usize,
}

/// The agent-provider picker (`A`): choose which AI the `a` key uses and set a
/// model override for it.
pub struct ProviderPick {
    /// highlighted row, an index into `agent::Provider::ALL`
    pub cursor: usize,
    /// `Some` while typing a model override for the highlighted provider
    pub editing: Option<LineEditor>,
}

/// The project manager (`p`): create / open / switch the working folder.
pub enum ProjectUi {
    /// The menu: New project, Open folder, then recent projects.
    Menu { cursor: usize },
    /// Browsing folders. `new` = choosing where to create a new project.
    Browse {
        dir: PathBuf,
        entries: Vec<PathBuf>,
        cursor: usize,
        new: bool,
    },
    /// Typing the new project's name; it's created inside `parent`.
    Name { parent: PathBuf, editor: LineEditor },
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
    /// Tool-list filter (empty = show everything in the category).
    pub filter: String,
    /// The category bar, and which tab is active.
    pub cat_tabs: Vec<CatTab>,
    pub cat: usize,
    /// Installs, hash ids and messages. Always there, can't be closed.
    pub console: Session,
    /// Tool terminals 1..=SLOTS.
    pub slots: [Option<Session>; SLOTS],
    /// Terminal in front: 0 = console, 1..=SLOTS = a slot.
    pub focus: usize,
    /// Show all slots in a 2x2 grid instead of one terminal.
    pub grid: bool,
    /// Last find-in-output text.
    search: String,
    pub mode: Mode,
    pub editor: LineEditor,
    history: History,
    /// A key that needs a second press (x on a running tool, q with tools running).
    pub confirm: Option<Confirm>,
    /// Packs chosen in setup.exe, waiting for a yes or no.
    pub first_run: Option<crate::firstrun::Pending>,
    /// Tools queued by a confirmed first run, installed one after another.
    install_queue: std::collections::VecDeque<usize>,
    /// Run waiting for the user to pick a slot: (tool index, args).
    pending: Option<(usize, Vec<String>)>,
    /// Re-detect was asked for during a pass: run another one after it.
    redetect_queued: bool,
    /// Start-up menu cursor (an index into START_CHOICES) while the splash
    /// asks how to continue; None when the splash is just "press any key".
    pub start_menu: Option<usize>,
    start_mode: StartMode,
    /// The web view, while it's on.
    pub web: Option<web::Server>,
    /// `--port`: serve on exactly this port.
    pub web_port: Option<u16>,
    /// `--lan`: serve the local network too, not only this machine.
    pub web_lan: bool,
    /// Per terminal (session id): output lines already sent to the web view.
    web_sent: BTreeMap<u64, u64>,
    /// Run logs already offered to the web view, by session id.
    web_logs: BTreeMap<u64, PathBuf>,
    /// Toasts not yet sent to the web view (and whether only the owner's
    /// pages get them).
    web_toasts: Vec<(ToastKind, String, bool)>,
    /// The last LAN data (join key, guests) sent to the owner's pages.
    web_admin: String,
    /// LAN view: guests waiting or in, and the join key (mirrored from the
    /// server every frame, for drawing).
    pub lan_guests: Vec<web::GuestInfo>,
    pub lan_key: Option<web::KeyInfo>,
    /// The address guests open (found once, when LAN mode starts).
    pub lan_url: Option<String>,
    /// The "X wants to join" prompt, while it's up.
    pub join_prompt: Option<JoinPrompt>,
    /// Waiting guests whose prompt was put off with Esc (still in `L`).
    join_later: BTreeSet<u64>,
    /// The `L` panel (LAN guests), with its cursor.
    pub lan_panel: Option<usize>,
    /// Port of the firewall rule winsploit added for LAN mode, while open.
    pub fw_port: Option<u16>,
    /// Firewall state for the `L` panel when no rule is open (why not).
    pub fw_note: String,
    /// The last state sent to the web view (only changes are sent).
    web_state: String,
    catalog_path: PathBuf,
    next_id: u64,
    /// Counts finished processes, to find the oldest finished terminal.
    ended_seq: u64,
    pub status_line: String,
    pub tick: u64,
    /// The '?' key overlay is open.
    pub show_help: bool,
    /// Appearance in effect (live-previewed while the `t` overlay is open).
    pub settings: Settings,
    /// The `t` appearance overlay, when open.
    pub appearance: Option<Appearance>,
    /// Flags caught in tool output this session, oldest first, no repeats.
    pub flags: Vec<Flag>,
    /// The `F` flags list, when open: the highlighted row (0 = newest).
    pub flags_view: Option<usize>,
    /// The active challenge (logs, flags and files go into its folder).
    pub challenge: Option<Workspace>,
    /// Challenges found under `ctf_root`, newest first (refreshed when the
    /// picker opens or one is created).
    pub challenge_list: Vec<Workspace>,
    /// The `C` challenge picker, when open: the highlighted row.
    pub challenge_view: Option<usize>,
    /// Where challenge folders live (`%USERPROFILE%\ctf`).
    pub ctf_root: PathBuf,
    /// The challenge being created by the prompts (`challenge_new`), or the
    /// field being edited on the active one.
    challenge_draft: workspace::Info,
    challenge_new: bool,
    /// The recipe picker (Enter on a tool with recipes): the highlighted
    /// row; the last row is "custom".
    pub recipe_view: Option<usize>,
    /// The recipe being filled in (index into the selected tool's recipes).
    pending_recipe: Option<usize>,
    /// Values given without an active challenge, and the file picked when
    /// a challenge has several.
    adhoc: crate::recipe::Values,
    /// Corner notifications, oldest first.
    pub toasts: Vec<Toast>,
    /// Step of the first-run welcome tour, while it's open.
    pub tour: Option<usize>,
    /// Colours are stripped (NO_COLOR); the renderer leans on modifiers.
    pub mono: bool,
    /// The sandbox prompt's network switch (off unless turned on per run).
    pub sandbox_net: bool,
    /// Last sample given to the sandbox prompt, offered again next time.
    sandbox_last: String,
    /// The working folder: tools run here (`d` changes it, like `cd`).
    pub cwd: PathBuf,
    /// The folder before the last change, for `d` then `-`.
    prev_cwd: Option<PathBuf>,
    /// Tab in the folder prompt: what was typed and the matches it cycles
    /// through (index of the one shown).
    pub folder_tab: Option<(String, Vec<String>, usize)>,
    /// The agent overlay (the AI's analysis), when open.
    pub agent: Option<AgentView>,
    /// The agent-provider picker (`A`), when open.
    pub provider_pick: Option<ProviderPick>,
    /// An agent analysis is in flight (icon shows a spinner).
    pub agent_busy: bool,
    /// The side chat (`T`): plain-language requests the AI turns into tool runs.
    pub chat: Chat,
    /// Screen rect of the clickable agent icon, set by the renderer each frame.
    pub agent_icon: std::cell::Cell<Option<Rect>>,
    /// What is drawn where (topmost last), set by the renderer each frame.
    pub hits: std::cell::RefCell<Vec<(Rect, Hit)>>,
    /// The last left click (when, column, row), to spot double-clicks.
    last_click: Option<(Instant, u16, u16)>,
    /// The active project (working folder), if one is open.
    pub project: Option<crate::project::Project>,
    /// Recent projects / last opened, persisted across restarts.
    pub pstore: crate::project::Store,
    /// The project manager overlay (`p`), when open.
    pub project_ui: Option<ProjectUi>,
    /// A background detection pass is in flight.
    detecting: bool,
    /// The console is running a scoop install (so re-detect when it exits).
    installing: bool,
    screen: Screen,
    tx: Sender<AppEvent>,
    rx: Receiver<AppEvent>,
    quit: bool,
}

impl App {
    pub fn new(catalog: Catalog, catalog_path: PathBuf, ignored: Option<PathBuf>) -> Self {
        let Catalog { tools, buckets, .. } = catalog;
        let (tx, rx) = mpsc::channel();
        let n = tools.len();
        let mut console = Session::new(CONSOLE, "console", "installs · hash ids · messages");
        console.push(":: winsploit ready. Pick a tool on the left; ← → switch category.");
        console.push(format!(
            ":: Enter runs it in its own terminal (up to {SLOTS} at once) · i installs it."
        ));
        console.push(format!(":: catalog: {}", catalog_path.display()));
        if let Some(ignored) = ignored {
            console.push(format!(
                "[err] ignored {} — catalogs aren't loaded from the current directory \
                 unless you pass --catalog",
                ignored.display()
            ));
        }
        let cat_tabs = Self::build_tabs(&tools);
        let mut app = App {
            tools,
            buckets,
            status: vec![ToolStatus::Unknown; n],
            conflicts: vec![None; n],
            selected: 0,
            filter: String::new(),
            cat_tabs,
            cat: 0,
            console,
            slots: Default::default(),
            focus: 0,
            grid: false,
            search: String::new(),
            mode: Mode::Browsing,
            editor: LineEditor::default(),
            history: History::default(),
            confirm: None,
            first_run: None,
            install_queue: Default::default(),
            pending: None,
            redetect_queued: false,
            start_menu: Some(0),
            start_mode: StartMode::Ask,
            web: None,
            web_port: None,
            web_lan: false,
            web_sent: BTreeMap::new(),
            web_logs: BTreeMap::new(),
            web_toasts: Vec::new(),
            web_admin: String::new(),
            lan_guests: Vec::new(),
            lan_key: None,
            lan_url: None,
            join_prompt: None,
            join_later: BTreeSet::new(),
            lan_panel: None,
            fw_port: None,
            fw_note: String::new(),
            web_state: String::new(),
            catalog_path,
            next_id: CONSOLE,
            ended_seq: 0,
            status_line: "detecting tools…".into(),
            tick: 0,
            show_help: false,
            settings: Settings::default(),
            appearance: None,
            flags: Vec::new(),
            flags_view: None,
            challenge: None,
            challenge_list: Vec::new(),
            challenge_view: None,
            ctf_root: workspace::root(),
            challenge_draft: workspace::Info::default(),
            challenge_new: false,
            recipe_view: None,
            pending_recipe: None,
            adhoc: crate::recipe::Values::default(),
            toasts: Vec::new(),
            tour: None,
            mono: crate::theme::no_color(),
            sandbox_net: false,
            sandbox_last: String::new(),
            cwd: std::env::current_dir().unwrap_or_else(|_| PathBuf::from(".")),
            prev_cwd: None,
            folder_tab: None,
            agent: None,
            provider_pick: None,
            agent_busy: false,
            chat: Chat::default(),
            agent_icon: std::cell::Cell::new(None),
            hits: std::cell::RefCell::new(Vec::new()),
            last_click: None,
            project: None,
            pstore: crate::project::Store::default(),
            project_ui: None,
            detecting: false,
            installing: false,
            screen: Screen::Splash,
            tx,
            rx,
            quit: false,
        };
        // start on the first tool in list order, not catalog order
        if let Some(&first) = app.visible().first() {
            app.selected = first;
        }
        // reopen the last project, if its folder still exists
        app.pstore = crate::project::Store::load();
        if let Some(last) = app.pstore.last.clone().filter(|p| p.is_dir()) {
            app.cwd = last.clone();
            app.project = Some(crate::project::Project::open(last));
        }
        // show the last probe's install status immediately; the background
        // detection pass (started by `run`) then reconciles it (#106)
        let exes: Vec<String> = app.tools.iter().map(|t| t.exe.clone()).collect();
        for (i, cached) in detect::load_cache(&exes).into_iter().enumerate() {
            match cached {
                Some(detect::Found::Installed(_)) => app.status[i] = ToolStatus::Installed,
                Some(detect::Found::Missing) => app.status[i] = ToolStatus::Missing,
                Some(detect::Found::Conflict(p)) => {
                    app.status[i] = ToolStatus::Conflict;
                    app.conflicts[i] = Some(p);
                }
                None => {}
            }
        }
        app
    }

    /// All, Installed, then every category in use: known ones in taxonomy
    /// order, unknown ones alphabetically after.
    fn build_tabs(tools: &[Tool]) -> Vec<CatTab> {
        let mut ids: Vec<String> = Vec::new();
        for t in tools {
            let cats: Vec<String> = if t.categories.is_empty() {
                vec!["other".into()]
            } else {
                t.categories.clone()
            };
            for c in cats {
                if !ids.contains(&c) {
                    ids.push(c);
                }
            }
        }
        ids.sort_by(|a, b| {
            (registry::category_rank(a), a.as_str()).cmp(&(registry::category_rank(b), b.as_str()))
        });
        let mut tabs = vec![CatTab::All, CatTab::Installed];
        tabs.extend(ids.into_iter().map(CatTab::Cat));
        tabs
    }

    /// Skip the start-up question (`--web` / `--no-web`).
    pub fn set_start(&mut self, mode: StartMode) {
        self.start_mode = mode;
        self.start_menu = (mode == StartMode::Ask).then_some(0);
    }

    pub fn run(mut self, terminal: &mut DefaultTerminal) -> Result<()> {
        self.detect_all(false);
        // once a day, best-effort: is there a newer release? (toast only)
        if self.settings.update_check {
            crate::update::check(self.tx.clone());
        }
        // a crashed run may have left its firewall rule open
        if let Some(port) = firewall::leftover() {
            self.fw_port = Some(port);
            self.fw_close("removed a leftover firewall rule from an earlier run");
        }
        if self.start_mode == StartMode::Web {
            self.start_web(true);
        }
        let mut title = String::new();
        while !self.quit {
            // the console title names the working folder (tabs tell apart)
            let want = crate::shortcut::title_for(&self.cwd);
            if want != title {
                crate::shortcut::set_title(&want);
                title = want;
            }
            terminal.draw(|f| match self.screen {
                Screen::Splash => {
                    banner::render_splash(f, self.tick, &self.settings.theme(), self.start_menu)
                }
                Screen::Main => ui::render(f, &self),
            })?;

            // drain runner (and web view) events
            while let Ok(ev) = self.rx.try_recv() {
                self.on_runner_event(ev);
            }

            if event::poll(Duration::from_millis(80))? {
                match event::read()? {
                    Event::Key(key) if key.kind == KeyEventKind::Press => match self.screen {
                        Screen::Splash => self.on_key_splash(key.code),
                        Screen::Main => self.on_key(key),
                    },
                    // bracketed paste: the whole pasted text arrives at once
                    Event::Paste(text) if self.chat.focused => self.chat.input.insert_str(&text),
                    Event::Paste(text) if self.mode.is_prompt() => self.editor.insert_str(&text),
                    Event::Mouse(m) if self.screen == Screen::Main => self.on_mouse(m),
                    _ => {}
                }
            }
            self.expire_toasts(Instant::now());
            self.sync_web();
            self.refresh_join_prompt(Instant::now());
            self.tick = self.tick.wrapping_add(1);
        }
        self.kill_all();
        self.fw_close("closed the LAN firewall opening");
        // ends the session for every browser ("session ended")
        self.web = None;
        Ok(())
    }

    /// Splash keys. With the start-up menu: up/down (or 1/2) choose, Enter
    /// confirms; without it any key enters. The first-run tour follows.
    fn on_key_splash(&mut self, code: KeyCode) {
        let pick = match self.start_menu {
            None => Some(0),
            Some(cur) => {
                let n = START_CHOICES.len();
                match code {
                    KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                        self.start_menu = Some((cur + n - 1) % n);
                        None
                    }
                    KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                        self.start_menu = Some((cur + 1) % n);
                        None
                    }
                    KeyCode::Char(c @ '1'..='2') => Some(c as usize - '1' as usize),
                    KeyCode::Enter => Some(cur),
                    KeyCode::Char('q') => {
                        self.quit = true;
                        None
                    }
                    _ => None,
                }
            }
        };
        let Some(choice) = pick else { return };
        self.start_menu = None;
        self.screen = Screen::Main;
        if !self.settings.tour_done {
            self.tour = Some(0);
        }
        if choice == 1 {
            self.start_web(true);
        }
    }

    // --------------------------------------------------------------- toasts

    /// A toast here and on every web page.
    pub fn toast(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toast_to(kind, text.into(), false);
    }

    /// A toast here and on the owner's web pages only (LAN guest news).
    fn toast_host(&mut self, kind: ToastKind, text: impl Into<String>) {
        self.toast_to(kind, text.into(), true);
    }

    fn toast_to(&mut self, kind: ToastKind, text: String, owner_only: bool) {
        if self.web.is_some() {
            self.web_toasts.push((kind, text.clone(), owner_only));
        }
        self.toasts.push(Toast {
            text,
            kind,
            born: Instant::now(),
        });
        if self.toasts.len() > MAX_TOASTS {
            let extra = self.toasts.len() - MAX_TOASTS;
            self.toasts.drain(0..extra);
        }
    }

    pub fn expire_toasts(&mut self, now: Instant) {
        self.toasts
            .retain(|t| now.saturating_duration_since(t.born) < TOAST_LIFE);
    }

    // ---------------------------------------------------------------- flags

    /// Look for flags in a line of output from terminal `pos`. A new one is
    /// listed, announced, copied to the clipboard and saved to flags.txt.
    fn catch_flags(&mut self, pos: usize, line: &str) {
        let extra: Vec<String> = self
            .challenge
            .as_ref()
            .map(|w| w.info.flag_format.clone())
            .into_iter()
            .filter(|f| !f.is_empty())
            .collect();
        for hit in crate::flags::find_with(line, &extra) {
            if self.flags.iter().any(|f| f.text == hit.text) {
                continue;
            }
            let source = self
                .session(pos)
                .map_or_else(|| "console".to_string(), |s| s.title.clone());
            let flag = Flag {
                text: hit.text.clone(),
                known: hit.known,
                source: source.clone(),
                at: crate::runlog::stamp_iso_now(),
            };
            // tests must not touch the real clipboard or %LOCALAPPDATA%
            let copied = !cfg!(test) && runner::copy_to_clipboard(&flag.text).is_ok();
            if !cfg!(test) {
                Self::save_flag(&self.flags_path(), &flag, line);
            }
            let kind = if hit.known { "flag" } else { "likely flag" };
            self.toast(ToastKind::Ok, format!("{kind} found: {}", flag.text));
            self.status_line = format!(
                "{kind} in {source}: {}{} · F lists them all",
                flag.text,
                if copied { " · copied" } else { "" }
            );
            self.flags.push(flag);
        }
    }

    /// Where flags are saved: the challenge's flags.txt, else
    /// `%LOCALAPPDATA%\winsploit\flags.txt`.
    fn flags_path(&self) -> PathBuf {
        match &self.challenge {
            Some(w) => w.flags_path(),
            None => crate::runlog::log_dir()
                .parent()
                .map(|p| p.join("flags.txt"))
                .unwrap_or_else(|| PathBuf::from("flags.txt")),
        }
    }

    /// Append one line per flag to `path`.
    fn save_flag(path: &std::path::Path, flag: &Flag, line: &str) {
        use std::io::Write;
        if let Some(dir) = path.parent() {
            let _ = std::fs::create_dir_all(dir);
        }
        let context: String = line.chars().take(300).collect();
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(path)
        {
            let _ = writeln!(
                f,
                "{}\t{}\t{}\t{}",
                flag.at,
                flag.source,
                flag.text,
                context.trim()
            );
        }
    }

    /// Keys while the `F` flags list is open (newest first).
    fn on_key_flags(&mut self, code: KeyCode) {
        let Some(cur) = self.flags_view else {
            return;
        };
        let n = self.flags.len();
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.flags_view = Some(cur.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => {
                self.flags_view = Some((cur + 1).min(n.saturating_sub(1)))
            }
            KeyCode::Enter | KeyCode::Char('y') if n > 0 => {
                let text = self.flags[n - 1 - cur.min(n - 1)].text.clone();
                self.status_line = match runner::copy_to_clipboard(&text) {
                    Ok(()) => format!("copied {text}"),
                    Err(e) => format!("couldn't copy: {e}"),
                };
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('F') => self.flags_view = None,
            _ => {}
        }
    }

    // ----------------------------------------------------------- challenges

    /// Where run logs go: the active challenge's `logs\`, else the open
    /// project's `.winsploit\logs`, else the default.
    fn logs_dir(&self) -> PathBuf {
        if let Some(ws) = self.challenge.as_ref() {
            return ws.logs_dir();
        }
        if let Some(p) = self.project.as_ref() {
            return p.log_dir();
        }
        crate::runlog::log_dir()
    }

    /// At start-up: reopen the challenge that was active last time.
    pub fn restore_challenge(&mut self) {
        self.challenge_list = workspace::list(&self.ctf_root);
        if let Some(ws) = workspace::recall(&self.ctf_root) {
            self.console.push(format!(
                ":: challenge: {} ({})",
                ws.title(),
                ws.dir.display()
            ));
            self.challenge = Some(ws);
        }
    }

    /// Make `ws` the active challenge (None: no challenge).
    fn set_challenge(&mut self, ws: Option<Workspace>) {
        if !cfg!(test) {
            workspace::remember(ws.as_ref().map(|w| w.id.as_str()));
        }
        match &ws {
            Some(w) => {
                let target = if w.info.target.is_empty() {
                    String::new()
                } else {
                    format!(" · {}", w.info.target)
                };
                self.console.push(format!(
                    ":: challenge: {}{target} · logs, flags and files go to {}",
                    w.title(),
                    w.dir.display()
                ));
                self.status_line = format!("challenge: {}{target}", w.title());
                self.toast(ToastKind::Info, format!("challenge: {}", w.title()));
                // work where the challenge's files are
                if w.dir.is_dir() {
                    let dir = w.dir.clone();
                    self.set_cwd(dir);
                }
            }
            None => {
                self.console
                    .push(":: no active challenge: logs and flags go to the default folder");
                self.status_line = "no active challenge".into();
            }
        }
        self.challenge = ws;
        self.adhoc.file = None;
        self.challenge_list = workspace::list(&self.ctf_root);
    }

    /// `C`: open the challenge picker on the active challenge.
    fn open_challenges(&mut self) {
        self.challenge_list = workspace::list(&self.ctf_root);
        let cur = self
            .challenge
            .as_ref()
            .and_then(|a| self.challenge_list.iter().position(|w| w.id == a.id))
            .unwrap_or(0);
        self.challenge_view = Some(cur);
    }

    /// Start a challenge prompt for `field`, prefilled with `text`.
    fn challenge_prompt(&mut self, field: ChallengeField, text: &str) {
        self.challenge_view = None;
        self.editor.set(text);
        self.mode = Mode::Challenge(field);
    }

    /// Keys while the challenge picker is open.
    fn on_key_challenges(&mut self, code: KeyCode) {
        let Some(cur) = self.challenge_view else {
            return;
        };
        let n = self.challenge_list.len();
        let need_active = |app: &mut App| {
            if app.challenge.is_none() {
                app.status_line = "no active challenge: Enter picks one, n creates one".into();
                false
            } else {
                true
            }
        };
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.challenge_view = Some(cur.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => {
                self.challenge_view = Some((cur + 1).min(n.saturating_sub(1)))
            }
            KeyCode::Enter if n > 0 => {
                let id = self.challenge_list[cur.min(n - 1)].id.clone();
                self.challenge_view = None;
                self.apply(Action::Challenge(id), Source::Terminal);
            }
            KeyCode::Char('n') => {
                self.challenge_new = true;
                self.challenge_draft = workspace::Info {
                    event: self
                        .challenge
                        .as_ref()
                        .map(|w| w.info.event.clone())
                        .unwrap_or_default(),
                    ..Default::default()
                };
                let ev = self.challenge_draft.event.clone();
                self.challenge_prompt(ChallengeField::Event, &ev);
            }
            KeyCode::Char('t') if need_active(self) => {
                self.challenge_new = false;
                let t = self.challenge.as_ref().map(|w| w.info.target.clone());
                self.challenge_prompt(ChallengeField::Target, &t.unwrap_or_default());
            }
            KeyCode::Char('l') if need_active(self) => {
                self.challenge_new = false;
                let f = self.challenge.as_ref().map(|w| w.info.flag_format.clone());
                self.challenge_prompt(ChallengeField::Flag, &f.unwrap_or_default());
            }
            KeyCode::Char('f') if need_active(self) => {
                self.challenge_new = false;
                self.challenge_prompt(ChallengeField::File, "");
            }
            KeyCode::Char('m') if need_active(self) => {
                let p = self.challenge.as_ref().map(Workspace::notes_path);
                self.open_in_explorer(p);
            }
            KeyCode::Char('o') if need_active(self) => {
                let p = self.challenge.as_ref().map(|w| w.dir.clone());
                self.open_in_explorer(p);
            }
            KeyCode::Char('x') => {
                self.challenge_view = None;
                self.set_challenge(None);
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('C') => self.challenge_view = None,
            _ => {}
        }
    }

    /// Open a file (with its default app) or a folder in Explorer.
    fn open_in_explorer(&mut self, path: Option<PathBuf>) {
        let Some(p) = path else { return };
        self.challenge_view = None;
        if cfg!(test) {
            return;
        }
        let mut cmd = std::process::Command::new("explorer");
        cmd.arg(&p);
        self.status_line = match runner::spawn_detached(cmd) {
            Ok(_) => format!("opened {}", p.display()),
            Err(e) => format!("couldn't open {}: {e}", p.display()),
        };
    }

    /// Enter in a challenge prompt: store the field, then ask for the next
    /// one (creating), or save it (editing).
    fn challenge_step(&mut self, field: ChallengeField, text: String) {
        use ChallengeField::*;
        let text = text.trim().to_string();
        if self.challenge_new {
            match field {
                Event | Name if workspace::slug(&text).is_empty() => {
                    self.status_line = "it needs a name with letters or digits".into();
                    self.challenge_prompt(field, &text);
                }
                Event => {
                    self.challenge_draft.event = text;
                    self.challenge_prompt(Name, "");
                }
                Name => {
                    self.challenge_draft.name = text;
                    self.challenge_prompt(Target, "");
                }
                Target => {
                    self.challenge_draft.target = text;
                    self.challenge_prompt(Flag, "");
                }
                Flag => {
                    let d = self.challenge_draft.clone();
                    match Workspace::create(&self.ctf_root, &d.event, &d.name, &d.target, &text) {
                        Ok(ws) => {
                            self.set_challenge(Some(ws));
                            self.challenge_new = false;
                            self.challenge_prompt(File, "");
                            self.status_line
                                .push_str(" · add a challenge file now (path), or Enter to skip");
                        }
                        Err(e) => {
                            self.status_line = format!("couldn't create the challenge: {e:#}");
                        }
                    }
                }
                File => {}
            }
            return;
        }
        let Some(ws) = self.challenge.as_mut() else {
            return;
        };
        match field {
            Target => ws.info.target = text,
            Flag => ws.info.flag_format = workspace::flag_prefix(&text),
            File if text.is_empty() => return,
            File => {
                let src = PathBuf::from(text.trim_matches('"'));
                match ws.add_file(&src) {
                    Ok(dest) => {
                        self.console.push(format!(
                            ":: copied {} -> {}",
                            src.display(),
                            dest.display()
                        ));
                        self.status_line = format!(
                            "added {} to the challenge (original untouched)",
                            dest.file_name().unwrap_or_default().to_string_lossy()
                        );
                    }
                    Err(e) => self.status_line = format!("not added: {e:#}"),
                }
                return;
            }
            Event | Name => return,
        }
        self.status_line = match ws.save() {
            Ok(()) => format!("saved: {} · target {}", ws.title(), ws.info.target),
            Err(e) => format!("couldn't save challenge.yaml: {e}"),
        };
    }

    // -------------------------------------------------------------- recipes

    /// SecLists-lite's common.txt, when it's installed.
    fn default_wordlist() -> Option<String> {
        let p = scoop::scoop_root()?
            .join("apps")
            .join("seclists-lite")
            .join("current")
            .join("common.txt");
        p.is_file().then(|| p.display().to_string())
    }

    /// What the placeholders mean right now. With `make_out`, the `{out}`
    /// folder is created (only when a recipe is really being used).
    pub fn recipe_values(&self, make_out: bool) -> crate::recipe::Values {
        let ws = self.challenge.as_ref();
        let nonempty = |s: &str| (!s.trim().is_empty()).then(|| s.trim().to_string());
        let target = ws
            .and_then(|w| nonempty(&w.info.target))
            .or_else(|| self.adhoc.target.clone());
        // one challenge file: that one; several: the one picked this session
        let file = match ws.map(Workspace::files_newest_first) {
            Some(f) if f.len() == 1 => Some(f[0].display().to_string()),
            Some(f) if f.len() > 1 => self
                .adhoc
                .file
                .clone()
                .filter(|p| f.iter().any(|x| x.display().to_string() == *p)),
            _ => self.adhoc.file.clone(),
        };
        let out_dir = match ws {
            Some(w) => w.out_dir(),
            None => crate::runlog::log_dir()
                .parent()
                .map(|p| p.join("out"))
                .unwrap_or_else(std::env::temp_dir),
        };
        if make_out {
            let _ = std::fs::create_dir_all(&out_dir);
        }
        let wordlist = ws
            .and_then(|w| nonempty(&w.info.wordlist))
            .or_else(|| self.adhoc.wordlist.clone())
            .or_else(Self::default_wordlist);
        crate::recipe::Values {
            target,
            file,
            out: Some(out_dir.display().to_string()),
            wordlist,
        }
    }

    /// Keys while the recipe picker is open.
    fn on_key_recipes(&mut self, code: KeyCode) {
        let Some(cur) = self.recipe_view else {
            return;
        };
        let n = self.tools[self.selected].recipes.len() + 1; // + custom
        match code {
            KeyCode::Up | KeyCode::Char('k') => self.recipe_view = Some(cur.saturating_sub(1)),
            KeyCode::Down | KeyCode::Char('j') => self.recipe_view = Some((cur + 1).min(n - 1)),
            KeyCode::Enter => {
                self.recipe_view = None;
                if cur + 1 == n {
                    self.open_args_prompt();
                } else {
                    self.pending_recipe = Some(cur);
                    self.continue_recipe();
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => self.recipe_view = None,
            _ => {}
        }
    }

    /// The args prompt with the tool's example args (no recipe).
    fn open_args_prompt(&mut self) {
        let hint = self.tools[self.selected].args_hint.clone();
        self.editor.set(&hint.unwrap_or_default());
        self.history.reset();
        self.mode = Mode::Input;
    }

    /// Fill the pending recipe: ask for the first missing value, or open
    /// the args prompt with everything filled in.
    fn continue_recipe(&mut self) {
        use crate::recipe::{fill, Slot};
        let Some(idx) = self.pending_recipe else {
            return;
        };
        let Some(r) = self.tools[self.selected].recipes.get(idx).cloned() else {
            self.pending_recipe = None;
            return;
        };
        match fill(&r.args, &self.recipe_values(true)) {
            Ok(args) => {
                self.pending_recipe = None;
                self.editor.set(&args);
                self.history.reset();
                self.mode = Mode::Input;
                self.status_line = format!("recipe {} · check it, then Enter runs it", r.name);
            }
            Err(missing) => {
                let slot = missing[0];
                let prefill = match slot {
                    // several challenge files: offer the newest
                    Slot::File => self
                        .challenge
                        .as_ref()
                        .and_then(|w| w.files_newest_first().first().cloned())
                        .map(|p| p.display().to_string())
                        .unwrap_or_default(),
                    _ => String::new(),
                };
                self.editor.set(&prefill);
                self.history.reset();
                self.mode = Mode::Recipe(slot);
                self.status_line = format!("the {} recipe needs a {}", r.name, slot.name());
            }
        }
    }

    /// Enter in a recipe value prompt: keep the value (in the challenge
    /// when there is one), then carry on filling the recipe.
    fn recipe_value(&mut self, slot: crate::recipe::Slot, text: String) {
        use crate::recipe::Slot;
        let text = text.trim().trim_matches('"').trim().to_string();
        if text.is_empty() {
            self.status_line = format!("the recipe needs a {}", slot.name());
            self.editor.set("");
            self.mode = Mode::Recipe(slot);
            return;
        }
        match slot {
            Slot::Target => match self.challenge.as_mut() {
                Some(ws) => {
                    ws.info.target = text;
                    let _ = ws.save();
                }
                None => self.adhoc.target = Some(text),
            },
            Slot::Wordlist => match self.challenge.as_mut() {
                Some(ws) => {
                    ws.info.wordlist = text;
                    let _ = ws.save();
                }
                None => self.adhoc.wordlist = Some(text),
            },
            Slot::File => {
                let src = PathBuf::from(&text);
                let inside = self
                    .challenge
                    .as_ref()
                    .is_some_and(|w| src.starts_with(w.files_dir()));
                // a file from elsewhere is copied into the challenge first
                let used = match self.challenge.as_ref() {
                    Some(ws) if !inside && src.is_file() => match ws.add_file(&src) {
                        Ok(dest) => {
                            self.console.push(format!(
                                ":: copied {} -> {}",
                                src.display(),
                                dest.display()
                            ));
                            dest
                        }
                        Err(e) => {
                            self.status_line = format!("not added: {e:#}");
                            self.pending_recipe = None;
                            return;
                        }
                    },
                    _ => src,
                };
                self.adhoc.file = Some(used.display().to_string());
            }
            Slot::Out => {}
        }
        self.continue_recipe();
    }

    // ----------------------------------------------------------------- tour

    fn on_key_tour(&mut self, code: KeyCode) {
        let Some(step) = self.tour else {
            return;
        };
        match code {
            KeyCode::Enter | KeyCode::Right | KeyCode::Char(' ') | KeyCode::Char('l') => {
                if step + 1 >= TOUR_STEPS {
                    self.finish_tour();
                } else {
                    self.tour = Some(step + 1);
                }
            }
            KeyCode::Left | KeyCode::Char('h') => self.tour = Some(step.saturating_sub(1)),
            KeyCode::Esc | KeyCode::Char('q') => self.finish_tour(),
            _ => {}
        }
    }

    /// Done or skipped: never show it again.
    fn finish_tour(&mut self) {
        self.tour = None;
        self.settings.tour_done = true;
        // tests must not write the real %APPDATA%
        if !cfg!(test) {
            let _ = self.settings.save();
        }
    }

    // ------------------------------------------------------------ terminals

    /// The terminal at position `pos` (0 = console, 1..=SLOTS = slots).
    pub fn session(&self, pos: usize) -> Option<&Session> {
        if pos == 0 {
            Some(&self.console)
        } else {
            self.slots.get(pos - 1).and_then(Option::as_ref)
        }
    }

    fn session_mut(&mut self, pos: usize) -> Option<&mut Session> {
        if pos == 0 {
            Some(&mut self.console)
        } else {
            self.slots.get_mut(pos - 1).and_then(Option::as_mut)
        }
    }

    pub fn focused(&self) -> Option<&Session> {
        self.session(self.focus)
    }

    fn focused_mut(&mut self) -> Option<&mut Session> {
        self.session_mut(self.focus)
    }

    /// Where the session with this id lives, if it still exists.
    fn position_of(&self, sid: u64) -> Option<usize> {
        if sid == CONSOLE {
            return Some(0);
        }
        self.slots
            .iter()
            .position(|s| s.as_ref().is_some_and(|s| s.id == sid))
            .map(|i| i + 1)
    }

    /// Is the terminal at `pos` on screen right now?
    fn on_screen(&self, pos: usize) -> bool {
        pos == self.focus || (self.grid && pos > 0)
    }

    fn set_focus(&mut self, pos: usize) {
        self.focus = pos.min(SLOTS);
        if self.focus == 0 {
            self.grid = false;
        }
        if let Some(s) = self.focused_mut() {
            s.unread = false;
        }
    }

    /// Tab / Shift+Tab: the next terminal that has something in it (the
    /// console counts, except in the grid, which only shows slots).
    fn cycle_focus(&mut self, forward: bool) {
        let n = SLOTS + 1;
        let mut pos = self.focus;
        for _ in 0..n {
            pos = if forward {
                (pos + 1) % n
            } else {
                (pos + n - 1) % n
            };
            let usable = if pos == 0 {
                !self.grid
            } else {
                self.slots[pos - 1].is_some()
            };
            if usable {
                self.set_focus(pos);
                return;
            }
        }
    }

    pub fn running_count(&self) -> usize {
        self.slots.iter().flatten().filter(|s| s.running()).count()
            + usize::from(self.console.running())
    }

    /// The slot a new run goes to: the first empty one, else the one whose
    /// tool finished longest ago. None when every slot is still running.
    pub fn free_slot(&self) -> Option<usize> {
        if let Some(i) = self.slots.iter().position(Option::is_none) {
            return Some(i);
        }
        self.slots
            .iter()
            .enumerate()
            .filter_map(|(i, s)| s.as_ref().filter(|s| !s.running()).map(|s| (s.ended, i)))
            .min()
            .map(|(_, i)| i)
    }

    /// Start `tool` with `args` in slot `slot` (0-based), replacing (and
    /// killing) whatever is there, and bring it to the front.
    fn start_tool(&mut self, slot: usize, tool: usize, args: Vec<String>, src: Source) {
        if let Some(mut old) = self.slots[slot].take() {
            if old.running() {
                old.kill();
                old.exited(None, 0); // closes its log
            }
        }
        let t = self.tools[tool].clone();
        self.next_id += 1;
        let mut s = Session::new(self.next_id, &t.name, &t.description);
        s.redact = self.settings.redact;
        if let Some(by) = self.started_by(src) {
            s.push(format!(":: started {by}"));
        }
        if let Some(note) = &t.note {
            s.push(format!(":: note: {note}"));
        }
        // secrets in the command line stay out of the pane (which the web
        // view mirrors to LAN guests) and out of the run log (#70)
        let shown = crate::redact::command(&t.run_cmd, &args, &t.secret_args, self.settings.redact);
        s.push(format!(":: in {}", crate::folder::short(&self.cwd)));
        s.push(format!("$ {shown}"));
        let mut cmd = runner::tool_command(&t.run_cmd, &args);
        cmd.current_dir(&self.cwd);
        match runner::spawn_streaming(cmd, self.tx.clone(), true, s.id) {
            Ok(sp) => {
                s.open_log(&self.logs_dir(), &t.name, &shown, &self.source_label(src));
                self.status_line = format!(
                    "running {} in terminal {} (pid {})",
                    t.name,
                    slot + 1,
                    sp.pid
                );
                s.started(sp.pid, sp.stdin);
                if t.launch == Launch::Web {
                    // a local web app: give the server a moment, then open it
                    let url = t.url.clone().unwrap_or_default();
                    s.push(format!(":: opening {url} in your browser"));
                    self.open_url_later(s.id, url, 2);
                }
            }
            Err(e) => s.push(format!("[err] failed to launch: {e}")),
        }
        self.slots[slot] = Some(s);
        self.set_focus(slot + 1);
    }

    /// Run in `slot` (1..=SLOTS) if given, else a free slot; when every
    /// slot is busy the terminal asks which to replace, and the web view
    /// (which asks before sending) gets an error.
    fn run_in_slot(&mut self, tool: usize, args: Vec<String>, src: Source, slot: Option<usize>) {
        if let Some(n) = slot.filter(|n| (1..=SLOTS).contains(n)) {
            self.start_tool(n - 1, tool, args, src);
            return;
        }
        match self.free_slot() {
            Some(slot) => self.start_tool(slot, tool, args, src),
            None if src.is_web() => {
                self.toast(
                    ToastKind::Err,
                    format!("all {SLOTS} terminals are busy: pick one to replace"),
                );
            }
            None => {
                self.pending = Some((tool, args));
                self.mode = Mode::SlotPick;
            }
        }
    }

    /// `x`: close the focused terminal. A running tool needs a second `x`.
    fn close_focused(&mut self, confirmed: bool) {
        if self.focus == 0 {
            self.status_line = "the console stays open (it holds installs and messages)".into();
            return;
        }
        let pos = self.focus;
        let Some(s) = self.slots[pos - 1].as_ref() else {
            self.status_line = format!("terminal {pos} is already free");
            return;
        };
        if s.running() && !confirmed {
            self.status_line = format!(
                "{} is still running — press x again to stop it and close terminal {pos}",
                s.title
            );
            self.confirm = Some(Confirm::Close(pos));
            return;
        }
        self.close_terminal(pos);
    }

    /// Close tool terminal `pos` (1..=SLOTS), stopping its tool.
    fn close_terminal(&mut self, pos: usize) {
        let Some(mut s) = self
            .slots
            .get_mut(pos.wrapping_sub(1))
            .and_then(Option::take)
        else {
            return;
        };
        if s.running() {
            s.kill();
            s.exited(None, 0);
        }
        self.status_line = format!("closed terminal {pos} ({}) · its log is kept", s.title);
        if !self.grid {
            // show a neighbour that still has something in it, else the console
            let next = (1..=SLOTS)
                .filter(|&p| self.slots[p - 1].is_some())
                .min_by_key(|&p| p.abs_diff(pos))
                .unwrap_or(0);
            self.set_focus(next);
        }
    }

    /// Stop every running process (on quit).
    fn kill_all(&mut self) {
        for s in self.slots.iter_mut().flatten() {
            s.kill();
        }
        self.console.kill();
    }

    // ----------------------------------------------------------- detection

    /// Re-probe every tool on a background thread; the result comes back as
    /// `AppEvent::Detected`, so the UI keeps drawing meanwhile. With
    /// `refresh_path`, first pick up PATH changes persisted since winsploit
    /// started (installs that add a PATH entry, a freshly installed Scoop).
    fn detect_all(&mut self, refresh_path: bool) {
        if self.detecting {
            // run again when this pass is done: it may predate an install
            // or a PATH change the user is asking about
            self.redetect_queued = true;
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
                    let line = format!(":: PATH: picked up {dir}");
                    let _ = tx.send(AppEvent::Output { sid: CONSOLE, line });
                }
            }
            let found = detect::probe_all(&probes);
            let _ = tx.send(AppEvent::Detected(found));
        });
    }

    fn on_detected(&mut self, found: Vec<detect::Found>) {
        self.detecting = false;
        if std::mem::take(&mut self.redetect_queued) {
            self.detect_all(true);
            return;
        }
        // remember this probe so the next launch can show status instantly (#106)
        let exes: Vec<String> = self.tools.iter().map(|t| t.exe.clone()).collect();
        detect::save_cache(&exes, &found);
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
        // the Installed tab may have changed under the selection
        self.keep_selection_visible();
    }

    fn on_runner_event(&mut self, ev: AppEvent) {
        match ev {
            AppEvent::Output { sid, line } => {
                let Some(pos) = self.position_of(sid) else {
                    return; // its terminal was closed or replaced
                };
                let seen = self.on_screen(pos);
                self.catch_flags(pos, &line);
                if let Some(s) = self.session_mut(pos) {
                    s.output_line(line);
                    s.unread |= !seen;
                }
            }
            AppEvent::Exited { sid, code } => {
                // a run the chat started: tell the chat how it ended
                if self.chat.watch.as_ref().is_some_and(|(id, _)| *id == sid) {
                    if let Some((_, name)) = self.chat.watch.take() {
                        let how = match code {
                            Some(0) => "finished".to_string(),
                            Some(c) => format!("exited with code {c}"),
                            None => "ended".to_string(),
                        };
                        self.chat.say(
                            ChatRole::Note,
                            format!("{name} {how}. Ask me what it found."),
                        );
                    }
                }
                self.ended_seq += 1;
                let seq = self.ended_seq;
                if let Some(pos) = self.position_of(sid) {
                    let seen = self.on_screen(pos);
                    let focused = pos == self.focus;
                    let mut note = None;
                    let mut dur = None;
                    if let Some(s) = self.session_mut(pos) {
                        s.exited(code, seq);
                        s.unread |= !seen;
                        let title = s.title.clone();
                        dur = s.last_duration;
                        // "finished in 4m12s" so a long scan announces how long it took (#113)
                        let took = dur
                            .map(|d| format!(" in {}", fmt_duration(d)))
                            .unwrap_or_default();
                        if pos > 0 {
                            note = Some(if !s.failed() {
                                (
                                    ToastKind::Ok,
                                    format!("✓ {title} finished{took} · terminal {pos}"),
                                )
                            } else if let RunState::Exited(Some(c)) = s.state {
                                (ToastKind::Err, format!("✗ {title} exited {c}{took}"))
                            } else {
                                (ToastKind::Err, format!("✗ {title} stopped"))
                            });
                        }
                        // also when focused: don't leave "running X" behind
                        if pos > 0 {
                            let how = match code {
                                Some(c) => format!("exit {c}"),
                                None => "ended".into(),
                            };
                            self.status_line =
                                format!("{title} finished in terminal {pos} ({how})");
                        }
                    }
                    if let Some((kind, text)) = note {
                        self.toast(kind, text);
                    }
                    // a long run that finished while you were elsewhere: ring the
                    // terminal bell, if the user opted into sound (#113)
                    let long = dur.is_some_and(|d| d >= LONG_RUN);
                    if pos > 0 && long && !focused && self.settings.notify_sound {
                        ring_bell();
                    }
                    if focused && self.mode == Mode::StdinInput {
                        self.mode = Mode::Browsing;
                        self.editor.set("");
                    }
                }
                // Only an install can change what's on PATH; a plain tool run
                // doesn't need a re-probe.
                if sid == CONSOLE && std::mem::take(&mut self.installing) {
                    if code == Some(0) {
                        self.toast(ToastKind::Ok, "✓ install finished · re-detecting");
                    } else {
                        self.toast(ToastKind::Err, "✗ install failed (see console)");
                    }
                    self.detect_all(true);
                    self.next_queued_install();
                }
            }
            AppEvent::Detected(found) => self.on_detected(found),
            AppEvent::Web(action, caller) => self.on_web_action(action, caller),
            AppEvent::Lan(ev) => self.on_lan_event(ev),
            AppEvent::Agent(res) => self.on_agent_result(res),
            AppEvent::Chat { req, result } => self.on_chat_result(req, result),
            AppEvent::Update(tag) => {
                self.toast(
                    ToastKind::Info,
                    format!(
                        "Winsploit {tag} is available (you have {}) — see the releases page to update",
                        crate::update::current_version()
                    ),
                );
            }
        }
    }

    /// Resolve a web request (tools are named, not indexed) and apply it.
    fn on_web_action(&mut self, action: WebAction, caller: Caller) {
        let tool = |app: &mut App, name: &str| {
            let i = app.tools.iter().position(|t| t.name == name);
            if i.is_none() {
                app.console.push(format!(
                    "[err] the browser asked for an unknown tool '{name}'"
                ));
            }
            i
        };
        let action = match action {
            WebAction::Run {
                tool: name,
                args,
                slot,
            } => match tool(self, &name) {
                Some(i) => Action::Run {
                    tool: i,
                    args,
                    slot,
                },
                None => return,
            },
            WebAction::Install { tool: name } => match tool(self, &name) {
                Some(i) => Action::Install(i),
                None => return,
            },
            WebAction::Cancel { term } => Action::Cancel(term),
            WebAction::Stdin { term, line } => Action::Stdin(term, line),
            WebAction::Eof { term } => Action::Eof(term),
            WebAction::Close { term } => Action::Close(term),
            WebAction::Redetect => Action::Redetect,
            WebAction::Hashid { hash } => Action::HashId(hash),
            WebAction::Challenge { id } => Action::Challenge(id),
            WebAction::Admit { guest, role } => Action::Admit(guest, role),
            WebAction::Remove { guest } => Action::Remove(guest),
            WebAction::Newkey => Action::NewKey,
        };
        self.apply(action, Source::Web(caller));
    }

    /// News from the LAN view's connection threads.
    fn on_lan_event(&mut self, ev: LanEvent) {
        match ev {
            LanEvent::JoinRequest(id) => {
                let Some(g) = self
                    .web
                    .as_ref()
                    .and_then(|w| w.guests().into_iter().find(|g| g.id == id))
                else {
                    return;
                };
                self.console.push(format!(
                    ":: {} · {} used the join key and is waiting to be let in",
                    g.label(),
                    g.agent
                ));
                self.toast_host(ToastKind::Info, format!("{} wants to join", g.label()));
            }
            LanEvent::KeyBurned => {
                self.console.push(format!(
                    "[err] {} wrong join keys: that key no longer works. L, then n, makes a new one.",
                    web::KEY_TRIES
                ));
                self.toast_host(ToastKind::Err, "join key burned after too many wrong tries");
            }
        }
    }

    /// How a run's source is written in its log: `terminal`, `web`, or
    /// `web: "Taha" (192.168.1.23)` for a LAN guest.
    fn source_label(&self, src: Source) -> String {
        match src {
            Source::Terminal => "terminal".into(),
            Source::Web(Caller::Owner) => "web".into(),
            Source::Web(Caller::Guest(id)) => format!("web: {}", self.guest_label(id)),
        }
    }

    /// "from the browser" / "by "Taha" (192.168.1.23) from the LAN", or
    /// None for the terminal.
    fn started_by(&self, src: Source) -> Option<String> {
        match src {
            Source::Terminal => None,
            Source::Web(Caller::Owner) => Some("from the browser".into()),
            Source::Web(Caller::Guest(id)) => {
                Some(format!("by {} from the LAN", self.guest_label(id)))
            }
        }
    }

    fn guest_label(&self, id: u64) -> String {
        self.web
            .as_ref()
            .and_then(|w| w.guests().into_iter().find(|g| g.id == id))
            .map_or_else(|| format!("guest #{id}"), |g| g.label())
    }

    /// Do `action`. The one place where terminal keys and web requests
    /// change the session.
    pub fn apply(&mut self, action: Action, src: Source) {
        match action {
            Action::Run { tool, args, slot } => {
                if tool >= self.tools.len() {
                    return;
                }
                let name = self.tools[tool].name.clone();
                self.history.push(&name, &args);
                self.launch(tool, &args, src, slot);
            }
            Action::Install(tool) if tool < self.tools.len() => self.install(tool, src),
            Action::Install(_) => {}
            Action::Cancel(pos) => self.cancel(pos),
            Action::Stdin(pos, line) => match self.session_mut(pos) {
                Some(s) if s.running() => s.send(&line),
                _ => self.status_line = "nothing is running in that terminal".into(),
            },
            Action::Eof(pos) => {
                if let Some(s) = self.session_mut(pos).filter(|s| s.has_stdin()) {
                    s.close_stdin();
                }
                if self.mode == Mode::StdinInput && pos == self.focus {
                    self.mode = Mode::Browsing;
                    self.editor.set("");
                }
            }
            Action::Close(pos) => self.close_terminal(pos),
            Action::Redetect => self.detect_all(true),
            Action::HashId(hash) => {
                self.history.push("\u{0}hashid", &hash);
                self.identify_hash(&hash);
            }
            // only ids of folders found under the root resolve: a browser
            // can't name an arbitrary path
            Action::Challenge(id) => match Workspace::open(&self.ctf_root, &id) {
                Ok(ws) => self.set_challenge(Some(ws)),
                Err(_) => {
                    self.toast(ToastKind::Err, format!("no challenge {id:?}"));
                }
            },
            // guests never manage guests (the server refuses it too)
            Action::Admit(..) | Action::Remove(_) | Action::NewKey
                if matches!(src, Source::Web(Caller::Guest(_))) => {}
            Action::Admit(id, role) => self.admit_guest(id, role),
            Action::Remove(id) => self.remove_guest(id),
            Action::NewKey => self.new_join_key(),
            // a browser never writes this machine's clipboard
            Action::Copy(_) if src.is_web() => {}
            Action::Copy(pos) => self.copy_output(pos),
        }
    }

    /// What `y` copies from terminal `pos`: its whole output, or only the
    /// lines matching the find while one is active there. (text, lines,
    /// the find text when only matches were taken)
    fn output_to_copy(&self, pos: usize) -> Option<(String, usize, Option<String>)> {
        let s = self.session(pos)?;
        let needle = self.search.trim().to_lowercase();
        let only = (!needle.is_empty() && s.search_hit.is_some()).then(|| self.search.trim());
        let lines: Vec<&str> = s
            .output
            .iter()
            .map(String::as_str)
            .filter(|l| only.is_none() || l.to_lowercase().contains(&needle))
            .collect();
        (!lines.is_empty()).then(|| (lines.join("\r\n"), lines.len(), only.map(str::to_string)))
    }

    /// `y` (or the copy button): put terminal `pos`'s output on the
    /// clipboard and say what was copied.
    fn copy_output(&mut self, pos: usize) {
        let Some((text, n, only)) = self.output_to_copy(pos) else {
            self.status_line = "nothing to copy in this terminal".into();
            return;
        };
        let from = match self.session(pos) {
            Some(s) if pos > 0 => format!("terminal {pos} ({})", s.title),
            _ => "the console".into(),
        };
        let what = format!(
            "{n} line{}{}",
            if n == 1 { "" } else { "s" },
            only.map(|f| format!(" matching '{f}'")).unwrap_or_default()
        );
        // tests must not touch the real clipboard
        let copied = if cfg!(test) {
            Ok(())
        } else {
            runner::copy_to_clipboard(&text)
        };
        match copied {
            Ok(()) => {
                let msg = format!("copied {what} from {from}");
                self.status_line = msg.clone();
                self.toast(ToastKind::Ok, msg);
            }
            Err(e) => {
                self.status_line = format!("couldn't copy: {e}");
                self.toast(ToastKind::Err, format!("couldn't copy {from}: {e}"));
            }
        }
    }

    // ----------------------------------------------------------- LAN guests

    /// The LAN view's server, or a note that LAN mode is off.
    fn lan_server(&mut self) -> Option<&web::Server> {
        if !self.web.as_ref().is_some_and(web::Server::lan) {
            self.status_line = "LAN mode is off: W shares this session on your network".into();
            return None;
        }
        self.web.as_ref()
    }

    fn admit_guest(&mut self, id: u64, role: Role) {
        let Some(w) = self.lan_server() else { return };
        let was = w.guests().into_iter().find(|g| g.id == id);
        match w.admit(id, role) {
            Some(g) => {
                let msg = if was.is_some_and(|w| w.status == GuestStatus::Waiting) {
                    format!("{} is in ({})", g.label(), role.label())
                } else {
                    format!("{} now has {}", g.label(), role.label())
                };
                self.console.push(format!(":: {msg}"));
                self.status_line = msg.clone();
                self.toast_host(ToastKind::Ok, msg);
            }
            None => self.status_line = "that guest is gone".into(),
        }
        self.mirror_lan();
    }

    fn remove_guest(&mut self, id: u64) {
        let Some(w) = self.lan_server() else { return };
        match w.remove(id) {
            Some(g) => {
                let msg = match g.status {
                    GuestStatus::Denied => format!("{} was turned away", g.label()),
                    _ => format!("{} was removed", g.label()),
                };
                self.console.push(format!(":: {msg}"));
                self.status_line = msg.clone();
                self.toast_host(ToastKind::Info, msg);
            }
            None => self.status_line = "that guest is gone".into(),
        }
        self.mirror_lan();
    }

    fn new_join_key(&mut self) {
        let Some(w) = self.lan_server() else { return };
        match w.new_key() {
            // not in the console: guests can read the console
            Ok(_) => self.status_line = "new join key: one use, 10 minutes (shown in L)".into(),
            Err(e) => self.status_line = format!("couldn't make a join key: {e}"),
        }
        self.mirror_lan();
    }

    /// Copy the server's guests and key, for drawing.
    fn mirror_lan(&mut self) {
        match self.web.as_ref().filter(|w| w.lan()) {
            Some(w) => {
                self.lan_guests = w.guests();
                self.lan_key = w.key();
            }
            None => {
                self.lan_guests.clear();
                self.lan_key = None;
                self.lan_panel = None;
                self.join_prompt = None;
            }
        }
    }

    /// Put the join prompt up for the next waiting guest when nothing else
    /// is going on (not while typing, not over another overlay), and take it
    /// down if that guest was decided elsewhere (the owner's browser).
    fn refresh_join_prompt(&mut self, now: Instant) {
        if let Some(p) = &self.join_prompt {
            let still = self
                .lan_guests
                .iter()
                .any(|g| g.id == p.guest.id && g.status == GuestStatus::Waiting);
            if !still {
                self.join_prompt = None;
            }
        }
        let quiet = self.screen == Screen::Main
            && self.mode == Mode::Browsing
            && self.tour.is_none()
            && self.appearance.is_none()
            && !self.show_help
            && self.lan_panel.is_none()
            && self.agent.is_none()
            && self.provider_pick.is_none()
            && self.flags_view.is_none()
            && self.challenge_view.is_none();
        if self.join_prompt.is_some() || !quiet {
            return;
        }
        let next = self
            .lan_guests
            .iter()
            .find(|g| g.status == GuestStatus::Waiting && !self.join_later.contains(&g.id));
        if let Some(g) = next {
            self.join_prompt = Some(JoinPrompt {
                guest: g.clone(),
                cursor: 0,
                since: now,
            });
        }
    }

    /// Keys while the join prompt is up: 1-3 or arrows + Enter decide, Esc
    /// puts it off (the guest keeps waiting in `L`).
    fn on_key_join(&mut self, code: KeyCode) {
        let Some(p) = self.join_prompt.as_mut() else {
            return;
        };
        if p.since.elapsed() < JOIN_GRACE {
            return;
        }
        let id = p.guest.id;
        let n = JOIN_CHOICES.len();
        let pick = match code {
            KeyCode::Up | KeyCode::Char('k') | KeyCode::BackTab => {
                p.cursor = (p.cursor + n - 1) % n;
                None
            }
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => {
                p.cursor = (p.cursor + 1) % n;
                None
            }
            KeyCode::Char(c @ '1'..='3') => Some(c as usize - '1' as usize),
            KeyCode::Enter => Some(p.cursor),
            KeyCode::Esc => {
                self.join_later.insert(id);
                self.join_prompt = None;
                self.status_line = "they keep waiting: L lets them in later".into();
                None
            }
            _ => None,
        };
        let Some(i) = pick else { return };
        self.join_prompt = None;
        let action = match i {
            0 => Action::Admit(id, Role::Watch),
            1 => Action::Admit(id, Role::Control),
            _ => Action::Remove(id),
        };
        self.apply(action, Source::Terminal);
    }

    /// `L`: the LAN panel. ↑↓ pick a guest, v / f let them in (or switch
    /// their role), x denies or removes, n makes a new join key.
    fn on_key_lan(&mut self, code: KeyCode) {
        let Some(cur) = self.lan_panel else { return };
        let n = self.lan_guests.len();
        let cur = cur.min(n.saturating_sub(1));
        let picked = self.lan_guests.get(cur).map(|g| g.id);
        match code {
            KeyCode::Up | KeyCode::Char('k') if n > 0 => self.lan_panel = Some((cur + n - 1) % n),
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab if n > 0 => {
                self.lan_panel = Some((cur + 1) % n)
            }
            KeyCode::Char('v') => {
                if let Some(id) = picked {
                    self.apply(Action::Admit(id, Role::Watch), Source::Terminal);
                }
            }
            KeyCode::Char('f') => {
                if let Some(id) = picked {
                    self.apply(Action::Admit(id, Role::Control), Source::Terminal);
                }
            }
            KeyCode::Char('x') => {
                if let Some(id) = picked {
                    self.apply(Action::Remove(id), Source::Terminal);
                }
            }
            KeyCode::Char('n') => self.apply(Action::NewKey, Source::Terminal),
            KeyCode::Char('c') => {
                if self.fw_port.is_some() {
                    self.fw_close("closed the LAN firewall opening");
                } else {
                    self.console.push(":: firewall: no winsploit rule is open");
                }
            }
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('L') => self.lan_panel = None,
            _ => {}
        }
    }

    // ----------------------------------------------------------------- keys

    fn on_key(&mut self, key: KeyEvent) {
        if self.tour.is_some() {
            self.on_key_tour(key.code);
            return;
        }
        if self.first_run.is_some() {
            self.on_key_first_run(key.code);
            return;
        }
        if self.join_prompt.is_some() {
            self.on_key_join(key.code);
            return;
        }
        if self.lan_panel.is_some() {
            self.on_key_lan(key.code);
            return;
        }
        if self.appearance.is_some() {
            self.on_key_appearance(key.code);
            return;
        }
        if self.flags_view.is_some() {
            self.on_key_flags(key.code);
            return;
        }
        if self.challenge_view.is_some() {
            self.on_key_challenges(key.code);
            return;
        }
        if self.recipe_view.is_some() {
            self.on_key_recipes(key.code);
            return;
        }
        if self.provider_pick.is_some() {
            self.on_key_provider(key.code);
            return;
        }
        if self.agent.is_some() {
            self.on_key_agent(key.code);
            return;
        }
        if self.project_ui.is_some() {
            self.on_key_project(key);
            return;
        }
        if self.show_help {
            // any key closes the overlay
            self.show_help = false;
            return;
        }
        // Ctrl+T shows or hides the chat panel from anywhere
        if key.code == KeyCode::Char('t') && key.modifiers == KeyModifiers::CONTROL {
            if self.chat.open {
                self.close_chat();
            } else if self.mode == Mode::Browsing {
                self.open_chat();
            }
            return;
        }
        if self.chat.focused {
            self.on_key_chat(key);
            return;
        }
        match self.mode {
            Mode::Browsing => self.on_key_browsing(key),
            Mode::SlotPick => self.on_key_slot_pick(key.code),
            _ => self.on_key_input(key),
        }
    }

    /// Keys while the appearance overlay is open. Changes apply live;
    /// Enter keeps and saves them, Esc puts the previous look back.
    fn on_key_appearance(&mut self, code: KeyCode) {
        let Some(ap) = self.appearance.as_mut() else {
            return;
        };
        let n = Field::ALL.len();
        let field = Field::ALL[ap.cursor];
        match code {
            KeyCode::Up | KeyCode::Char('k') => ap.cursor = (ap.cursor + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') | KeyCode::Tab => ap.cursor = (ap.cursor + 1) % n,
            KeyCode::Left | KeyCode::Char('h') => self.settings.step(field, -1),
            KeyCode::Right | KeyCode::Char('l') | KeyCode::Char(' ') => {
                self.settings.step(field, 1)
            }
            // 1-8 pick a theme directly, as in Claude Code's /theme
            KeyCode::Char(c @ '1'..='9') => {
                let i = c as usize - '1' as usize;
                if let Some(t) = crate::theme::ThemeName::ALL.get(i) {
                    self.settings.theme = *t;
                    ap.cursor = 0;
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.settings = ap.saved.clone();
                self.appearance = None;
            }
            KeyCode::Enter => {
                self.appearance = None;
                match self.settings.save() {
                    Ok(path) => {
                        self.status_line = format!("appearance saved · {}", path.display());
                        self.toast(ToastKind::Info, "appearance saved");
                    }
                    Err(e) => {
                        self.status_line = "appearance applied (not saved)".into();
                        self.console
                            .push(format!("[err] could not save appearance: {e}"));
                    }
                }
            }
            _ => {}
        }
    }

    /// All terminals busy: 1-4 replaces that one, Esc gives up.
    fn on_key_slot_pick(&mut self, code: KeyCode) {
        match code {
            KeyCode::Char(c @ '1'..='9') if (c as usize - '0' as usize) <= SLOTS => {
                self.mode = Mode::Browsing;
                if let Some((tool, args)) = self.pending.take() {
                    self.start_tool(c as usize - '1' as usize, tool, args, Source::Terminal);
                }
            }
            KeyCode::Esc | KeyCode::Char('q') => {
                self.mode = Mode::Browsing;
                self.pending = None;
                self.status_line = "nothing was started".into();
            }
            _ => {}
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
        } else if matches!(self.mode, Mode::Recipe(_)) {
            "\u{0}recipe".to_string()
        } else if matches!(self.mode, Mode::Challenge(_)) {
            "\u{0}challenge".to_string()
        } else if self.mode == Mode::SandboxInput {
            "\u{0}sandbox".to_string()
        } else if self.mode == Mode::FolderInput {
            "\u{0}folder".to_string()
        } else {
            // tool-args history, namespaced per project so each project recalls
            // its own commands with ↑
            let name = &self.tools[self.selected].name;
            match &self.project {
                Some(p) => format!("{}\u{1}{}", p.path.display(), name),
                None => name.clone(),
            }
        }
    }

    fn on_key_browsing(&mut self, key: KeyEvent) {
        // a pending confirmation only survives until the next key
        let confirm = self.confirm.take();
        match key.code {
            KeyCode::Char('q') | KeyCode::Esc => {
                let n = self.running_count();
                if n > 0 && confirm != Some(Confirm::Quit) {
                    self.status_line =
                        format!("{n} tool(s) still running — press q again to stop them and quit");
                    self.confirm = Some(Confirm::Quit);
                } else {
                    self.quit = true;
                }
            }
            KeyCode::Up | KeyCode::Char('k') => self.move_selection(-1),
            KeyCode::Down | KeyCode::Char('j') => self.move_selection(1),
            KeyCode::Left | KeyCode::Char('[') => self.step_category(-1),
            KeyCode::Right | KeyCode::Char(']') => self.step_category(1),
            KeyCode::Char(c @ '0'..='9') if (c as usize - '0' as usize) <= SLOTS => {
                self.set_focus(c as usize - '0' as usize)
            }
            KeyCode::Tab => self.cycle_focus(true),
            KeyCode::BackTab => self.cycle_focus(false),
            KeyCode::Char('g') => {
                self.grid = !self.grid;
                if self.grid && self.focus == 0 {
                    let first = (1..=SLOTS)
                        .find(|&p| self.slots[p - 1].is_some())
                        .unwrap_or(1);
                    self.set_focus(first);
                }
            }
            KeyCode::Char('x') => self.close_focused(confirm == Some(Confirm::Close(self.focus))),
            KeyCode::Char('c') => self.apply(Action::Cancel(self.focus), Source::Terminal),
            KeyCode::Char('s') => self.open_stdin(),
            KeyCode::Char('S') => self.open_sandbox(),
            KeyCode::Char('d') => self.open_folder_prompt(),
            KeyCode::Char('?') => self.show_help = true,
            KeyCode::Char('t') => {
                self.appearance = Some(Appearance {
                    cursor: 0,
                    saved: self.settings.clone(),
                })
            }
            KeyCode::Char('f') => {
                self.editor.set(&self.search.clone());
                self.mode = Mode::Search;
            }
            KeyCode::Char('C') => self.open_challenges(),
            KeyCode::Char('F') => {
                self.flags_view = Some(0);
                if self.flags.is_empty() {
                    self.status_line =
                        "no flags yet · they're caught from tool output automatically".into();
                }
            }
            KeyCode::Char('y') => self.apply(Action::Copy(self.focus), Source::Terminal),
            KeyCode::Char('n') => self.find_next(true),
            KeyCode::Char('N') => self.find_next(false),
            KeyCode::Char('/') => {
                self.editor.set(&self.filter.clone());
                self.mode = Mode::Filter;
            }
            KeyCode::Char('r') => self.apply(Action::Redetect, Source::Terminal),
            KeyCode::Char('w') => self.toggle_web(self.web_lan),
            KeyCode::Char('W') => self.toggle_web(true),
            KeyCode::Char('o') => self.start_web(true),
            KeyCode::Char('L') => {
                if self.lan_server().is_some() {
                    self.mirror_lan();
                    self.lan_panel = Some(0);
                }
            }
            KeyCode::Char('p') => self.open_project_menu(),
            KeyCode::Char('a') => self.start_analysis(),
            KeyCode::Char('T') => self.open_chat(),
            KeyCode::Char('A') => {
                let cursor = agent::Provider::ALL
                    .iter()
                    .position(|&p| p == self.settings.agent)
                    .unwrap_or(0);
                self.provider_pick = Some(ProviderPick {
                    cursor,
                    editing: None,
                });
            }
            KeyCode::PageUp => self.focused_mut().map_or((), Session::page_up),
            KeyCode::PageDown => self.focused_mut().map_or((), Session::page_down),
            KeyCode::End => self.focused_mut().map_or((), Session::follow),
            KeyCode::Home => self.focused_mut().map_or((), Session::scroll_top),
            KeyCode::Char('h') => {
                self.editor.set("");
                self.history.reset();
                self.mode = Mode::HashInput;
            }
            KeyCode::Enter | KeyCode::Char('i') if !self.visible().contains(&self.selected) => {
                self.status_line = "no tool selected here (← → category, / filter)".into();
            }
            KeyCode::Char('i') => self.apply(Action::Install(self.selected), Source::Terminal),
            KeyCode::Enter => {
                if self.tools[self.selected].launch == Launch::Dataset {
                    // nothing to run, so no args prompt
                    let run = Action::Run {
                        tool: self.selected,
                        args: String::new(),
                        slot: None,
                    };
                    self.apply(run, Source::Terminal);
                } else if !self.tools[self.selected].recipes.is_empty() {
                    self.recipe_view = Some(0);
                } else {
                    self.open_args_prompt();
                }
            }
            _ => {}
        }
    }

    /// `s`: type into the focused terminal's tool.
    fn open_stdin(&mut self) {
        let state = self
            .focused()
            .map(|s| (s.running(), s.has_stdin(), s.title.clone()));
        match state {
            Some((true, true, _)) => {
                self.editor.set("");
                self.history.reset();
                self.mode = Mode::StdinInput;
            }
            Some((true, false, title)) => {
                self.status_line = format!("{title} isn't reading input (closed with Ctrl+D?)");
            }
            _ => self.status_line = "nothing is running in this terminal".into(),
        }
    }

    fn on_key_input(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        // Windows reports AltGr as Ctrl+Alt: on AZERTY, QWERTZ and many other
        // layouts that's how \ { } [ ] @ # | ~ are typed (and how pasted
        // text containing them arrives). Those are text, not shortcuts.
        let altgr = ctrl && alt;
        let ctrl = ctrl && !altgr;
        // any key but Tab ends a run of folder completions
        if !matches!(key.code, KeyCode::Tab | KeyCode::BackTab) {
            self.folder_tab = None;
        }
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
                self.pending_recipe = None;
            }
            KeyCode::Enter if self.mode == Mode::Filter => {
                // keep the filter, back to the list
                self.editor.set("");
                self.mode = Mode::Browsing;
            }
            KeyCode::Tab if self.mode == Mode::SandboxInput => {
                self.sandbox_net = !self.sandbox_net;
            }
            KeyCode::Tab | KeyCode::BackTab if self.mode == Mode::FolderInput => {
                let back =
                    key.code == KeyCode::BackTab || key.modifiers.contains(KeyModifiers::SHIFT);
                self.folder_complete(back);
                return;
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
                        if let Some(s) = self.focused_mut() {
                            s.search_hit = None;
                        }
                        self.find_next(true);
                    }
                    Mode::HashInput => self.apply(Action::HashId(text), Source::Terminal),
                    Mode::SandboxInput => self.launch_sandboxed(&text),
                    Mode::FolderInput => self.change_folder(&text),
                    Mode::Challenge(field) => self.challenge_step(field, text),
                    Mode::Recipe(slot) => self.recipe_value(slot, text),
                    Mode::StdinInput => {
                        self.apply(Action::Stdin(self.focus, text), Source::Terminal)
                    }
                    _ => {
                        let run = Action::Run {
                            tool: self.selected,
                            args: text,
                            slot: None,
                        };
                        self.apply(run, Source::Terminal)
                    }
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
                self.mode = Mode::Browsing;
                self.editor.set("");
                self.apply(Action::Eof(self.focus), Source::Terminal);
            }
            KeyCode::Char('u') if ctrl => e.clear_to_start(),
            KeyCode::Char('w') if ctrl => e.delete_word_left(),
            KeyCode::Char('a') if ctrl => e.home(),
            KeyCode::Char('e') if ctrl => e.end(),
            KeyCode::Char(c) if altgr => e.insert(c),
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

    /// Jump to the next match of the last search in the focused terminal:
    /// `older` = further up (n), otherwise further down (N).
    fn find_next(&mut self, older: bool) {
        let needle = self.search.trim().to_string();
        if needle.is_empty() {
            self.status_line = "nothing to find (press f)".into();
            return;
        }
        let Some(s) = self.focused_mut() else {
            self.status_line = "this terminal is empty".into();
            return;
        };
        self.status_line = match s.find_next(&needle, older) {
            Some(total) => format!("'{needle}': {total} matching line(s) · n older · N newer"),
            None => format!("'{needle}' not found in this terminal"),
        };
    }

    // ----------------------------------------------------------- tool list

    /// Is tool `i` in category tab `tab`?
    fn in_tab(&self, i: usize, tab: &CatTab) -> bool {
        match tab {
            CatTab::All => true,
            CatTab::Installed => self.status[i] == ToolStatus::Installed,
            CatTab::Cat(id) => self.tools[i].in_category(id),
        }
    }

    /// Does tool `i` match the filter? Words AND together, each matching
    /// the name, the description, or any of its categories.
    fn matches_filter(&self, i: usize) -> bool {
        let needle = self.filter.trim().to_lowercase();
        let t = &self.tools[i];
        needle.split_whitespace().all(|w| {
            t.name.to_lowercase().contains(w)
                || t.description.to_lowercase().contains(w)
                || t.categories.iter().any(|c| {
                    c.contains(w) || registry::category_label(c).to_lowercase().contains(w)
                })
        })
    }

    /// Tools in the active tab that match the filter, in display order.
    pub fn visible(&self) -> Vec<usize> {
        self.visible_in(&self.cat_tabs[self.cat])
    }

    /// Display order: in a category, the tools made for it come first, then
    /// the ones that are also useful there. Elsewhere, grouped by main
    /// category. Catalog order breaks ties.
    pub fn visible_in(&self, tab: &CatTab) -> Vec<usize> {
        let mut v: Vec<usize> = (0..self.tools.len())
            .filter(|&i| self.in_tab(i, tab) && self.matches_filter(i))
            .collect();
        match tab {
            CatTab::Cat(id) => v.sort_by_key(|&i| (self.tools[i].primary() != id, i)),
            _ => v.sort_by_key(|&i| {
                let p = self.tools[i].primary();
                (registry::category_rank(p), p.to_string(), i)
            }),
        }
        v
    }

    /// ← →: switch category tab.
    fn step_category(&mut self, delta: i32) {
        let n = self.cat_tabs.len() as i32;
        self.cat = (self.cat as i32 + delta).rem_euclid(n) as usize;
        self.keep_selection_visible();
        let tab = &self.cat_tabs[self.cat];
        self.status_line = match tab {
            CatTab::All => "all tools, grouped by main category".into(),
            CatTab::Installed => "tools installed on this machine".into(),
            CatTab::Cat(id) => {
                let blurb = registry::CATEGORIES
                    .iter()
                    .find(|c| c.id == id)
                    .map(|c| c.blurb)
                    .unwrap_or("");
                format!("{}: {blurb}", tab.label())
            }
        };
    }

    /// If the selected tool left the list, select the first one shown.
    fn keep_selection_visible(&mut self) {
        let vis = self.visible();
        if !vis.is_empty() && !vis.contains(&self.selected) {
            self.selected = vis[0];
        }
    }

    fn set_filter(&mut self, f: String) {
        self.filter = f;
        self.keep_selection_visible();
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

    // --------------------------------------------------------------- actions

    /// Built-in hash identifier (`h`): no install, works offline.
    fn identify_hash(&mut self, hash: &str) {
        if hash.trim().is_empty() {
            return;
        }
        self.console.push(format!("$ hashid {}", hash.trim()));
        for line in hashid::report(hash) {
            self.console.push(line);
        }
        self.set_focus(0);
    }

    fn launch(&mut self, idx: usize, args_str: &str, src: Source, slot: Option<usize>) {
        let status = self.status[idx];
        let tool = self.tools[idx].clone();
        let name = tool.name.clone();

        match status {
            ToolStatus::Installed => {}
            ToolStatus::Unknown => {
                self.status_line = "still detecting tools — try again in a moment".into();
                return;
            }
            ToolStatus::Missing => {
                self.status_line = format!("{name} is not installed — press i to install it");
                return;
            }
            ToolStatus::Conflict => {
                let path = self.conflicts[idx]
                    .as_ref()
                    .map(|p| p.display().to_string())
                    .unwrap_or_default();
                self.console.push(format!(
                    "[err] the '{}' on PATH ({path}) isn't {name} — a different program with \
                     the same name comes first. Not running it.",
                    tool.exe
                ));
                self.console.push(
                    "[err] fix: press i to install the right one, or move its folder earlier in \
                     PATH (or uninstall the other program), then press r.",
                );
                self.status_line = format!("{name}: name conflict, not run (see console)");
                self.set_focus(0);
                return;
            }
        }

        let args = match args::split(args_str) {
            Ok(a) => a,
            Err(e) => {
                self.status_line = format!("can't parse the args: {e} — nothing was run");
                return;
            }
        };
        // {proj} in any argument expands to the open project's path
        let args: Vec<String> = args
            .into_iter()
            .map(|a| crate::project::expand(&a, self.project.as_ref()))
            .collect();

        match tool.launch {
            Launch::Stream => self.run_in_slot(idx, args, src, slot),
            Launch::Web if !tool.run_cmd.trim().is_empty() => {
                // the server streams in a terminal; the browser opens after
                self.run_in_slot(idx, args, src, slot)
            }
            Launch::Web => {
                let url = tool.url.clone().unwrap_or_default();
                self.console
                    .push(format!(":: {name}: opening {url} in your browser"));
                self.open_url_later(CONSOLE, url, 0);
                self.status_line = format!("opened {name} in your browser");
            }
            Launch::Gui => {
                let shown = crate::redact::command(
                    &tool.run_cmd,
                    &args,
                    &tool.secret_args,
                    self.settings.redact,
                );
                self.console.push(format!("$ {shown}"));
                let mut cmd = runner::tool_command(&tool.run_cmd, &args);
                cmd.current_dir(&self.cwd);
                match runner::spawn_detached(cmd) {
                    Ok(pid) => {
                        self.console.push(format!(
                            "── launched {name} (pid {pid}); it runs in its own window ──"
                        ));
                        self.status_line = format!("launched {name} in its own window");
                        if tool.sandbox == Sandbox::Recommended {
                            self.status_line
                                .push_str(" · tip: S runs it in Windows Sandbox instead");
                        }
                    }
                    Err(e) => {
                        self.console
                            .push(format!("[err] failed to launch {name}: {e}"));
                        self.status_line = format!("{name} failed to launch (see console)");
                    }
                }
            }
            Launch::Dataset => self.show_dataset(idx),
        }
    }

    /// `S`: ask for the sample to open in Windows Sandbox.
    fn open_sandbox(&mut self) {
        let tool = &self.tools[self.selected];
        let name = tool.name.clone();
        if !self.visible().contains(&self.selected) {
            self.status_line = "no tool selected here (← → category, / filter)".into();
            return;
        }
        if tool.sandbox == Sandbox::Never {
            self.status_line = format!("{name} isn't set up to run in the sandbox");
            return;
        }
        match self.status[self.selected] {
            ToolStatus::Installed => {}
            ToolStatus::Unknown => {
                self.status_line = "still detecting tools — try again in a moment".into();
                return;
            }
            _ => {
                self.status_line = format!("{name} is not installed — press i to install it");
                return;
            }
        }
        if crate::sandbox::sandbox_exe().is_none() {
            for line in crate::sandbox::ENABLE_HINT {
                self.console.push(format!("[err] {line}"));
            }
            self.status_line = "Windows Sandbox isn't enabled (see console for how)".into();
            self.set_focus(0);
            return;
        }
        self.sandbox_net = false;
        let last = self.sandbox_last.clone();
        self.editor.set(&last);
        self.mode = Mode::SandboxInput;
    }

    /// `d`: ask for the working folder, like `cd` in a shell.
    fn open_folder_prompt(&mut self) {
        self.editor.set("");
        self.history.reset();
        self.folder_tab = None;
        self.mode = Mode::FolderInput;
        self.status_line = format!("working folder: {}", self.cwd.display());
    }

    /// Tab in the folder prompt: fill in the next folder name that matches
    /// what was typed (Shift+Tab the previous one), like PowerShell.
    fn folder_complete(&mut self, back: bool) {
        let (typed, matches, i) = match self.folder_tab.take() {
            Some((typed, matches, i)) if !matches.is_empty() => {
                let n = matches.len();
                let i = if back { (i + n - 1) % n } else { (i + 1) % n };
                (typed, matches, i)
            }
            _ => {
                let typed = self.editor.text();
                let matches = crate::folder::complete(&self.cwd, &typed);
                let i = if back {
                    matches.len().saturating_sub(1)
                } else {
                    0
                };
                (typed, matches, i)
            }
        };
        match matches.get(i) {
            Some(m) => {
                self.editor.set(m);
                self.status_line = if matches.len() > 1 {
                    format!("{}/{} · Tab next · Shift+Tab back", i + 1, matches.len())
                } else {
                    "Enter goes there · Tab again lists what's inside".into()
                };
                // a single match: the next Tab looks inside it
                if matches.len() > 1 {
                    self.folder_tab = Some((typed, matches, i));
                }
            }
            None => {
                self.status_line = format!(
                    "no folder matches {:?} in {}",
                    typed.trim(),
                    crate::folder::short(&self.cwd)
                );
            }
        }
    }

    /// Enter in the folder prompt: go there (`-` = back to the previous one).
    fn change_folder(&mut self, text: &str) {
        let target = if text.trim() == "-" {
            match self.prev_cwd.clone() {
                Some(p) => Ok(p),
                None => Err("no previous folder yet".to_string()),
            }
        } else {
            crate::folder::resolve(&self.cwd, text)
        };
        match target {
            Ok(dir) => self.set_cwd(dir),
            Err(e) => {
                self.status_line = e.clone();
                self.toast(ToastKind::Err, e);
            }
        }
    }

    /// Make `dir` the working folder and say so in the console.
    fn set_cwd(&mut self, dir: PathBuf) {
        if dir == self.cwd {
            return;
        }
        self.prev_cwd = Some(std::mem::replace(&mut self.cwd, dir));
        let short = crate::folder::short(&self.cwd);
        self.console.push(format!(
            ":: working folder: {} · tools run here",
            self.cwd.display()
        ));
        self.status_line = format!("working folder: {short}");
        self.toast(ToastKind::Info, format!("in {short}"));
    }

    /// Start the selected tool in Windows Sandbox on `target` (a file or a
    /// folder, or nothing), network off unless switched on in the prompt.
    fn launch_sandboxed(&mut self, target: &str) {
        use crate::sandbox;
        let tool = self.tools[self.selected].clone();
        let name = tool.name.clone();
        let target = target.trim().trim_matches('"').trim();
        self.sandbox_last = target.to_string();
        let result = (|| -> anyhow::Result<(std::path::PathBuf, sandbox::Plan)> {
            let exe = sandbox::sandbox_exe()
                .ok_or_else(|| anyhow::anyhow!("Windows Sandbox isn't enabled"))?;
            let root = scoop::scoop_root().ok_or_else(|| anyhow::anyhow!("no Scoop root"))?;
            let pkg = tool.scoop.clone().unwrap_or_default();
            let (app_dir, exe_rel) = sandbox::locate(&tool.run_cmd, &root, &pkg)?;
            let mut deps = Vec::new();
            for d in &tool.sandbox_deps {
                let dir = root.join("apps").join(d).join("current");
                if !dir.is_dir() {
                    anyhow::bail!(
                        "{name} needs {d} inside the sandbox: install it first (scoop install {d})"
                    );
                }
                deps.push((d.clone(), sandbox::real_dir(&dir)?));
            }
            let out = crate::runlog::log_dir()
                .parent()
                .map(|p| p.join("sandbox"))
                .unwrap_or_else(std::env::temp_dir)
                .join(format!("{}-{name}", crate::runlog::stamp_now()));
            std::fs::create_dir_all(&out)?;
            let req = sandbox::Request {
                app_dir,
                exe_rel,
                console: tool.launch == Launch::Stream,
                deps,
                target: (!target.trim().is_empty())
                    .then(|| crate::folder::expand(&self.cwd, target)),
                out: out.clone(),
                network: self.sandbox_net,
            };
            let plan = sandbox::plan(&req, &sandbox::protected_dirs())?;
            let wsb = out.join("winsploit.wsb");
            std::fs::write(&wsb, sandbox::wsb(&plan))?;
            let mut cmd = std::process::Command::new(exe);
            cmd.arg(&wsb);
            runner::spawn_detached(cmd)?;
            Ok((out, plan))
        })();
        match result {
            Ok((out, plan)) => {
                let c = &mut self.console;
                c.push(format!(
                    "$ sandbox {name} {}",
                    if target.is_empty() { "" } else { target }
                ));
                for m in &plan.mappings {
                    c.push(format!(
                        ":: {} {} -> {}",
                        if m.writable { "rw" } else { "ro" },
                        m.host.display(),
                        m.guest
                    ));
                }
                c.push(format!(
                    ":: network {} · clipboard, GPU, audio, camera, printers off",
                    if plan.network { "ON" } else { "off" }
                ));
                c.push(format!(
                    "── {name} is starting in Windows Sandbox; save findings to Desktop\\out, \
                     they land in {} ──",
                    out.display()
                ));
                self.status_line = format!(
                    "{name} is starting in Windows Sandbox · everything inside is wiped when you close it"
                );
                self.toast(
                    ToastKind::Info,
                    format!("{name} starting in Windows Sandbox"),
                );
            }
            Err(e) => {
                self.console
                    .push(format!("[err] sandbox: {e:#} — nothing was started"));
                self.status_line = format!("{name}: sandbox not started (see console)");
                self.set_focus(0);
            }
        }
    }

    /// Enter on a dataset: list its files and copy the folder path, so it can
    /// be pasted into another tool's arguments.
    fn show_dataset(&mut self, idx: usize) {
        let tool = self.tools[idx].clone();
        match self.status[idx] {
            ToolStatus::Installed => {}
            ToolStatus::Unknown => {
                self.status_line = "still detecting tools — try again in a moment".into();
                return;
            }
            _ => {
                self.status_line =
                    format!("{} is not installed — press i to install it", tool.name);
                return;
            }
        }
        let Some(dir) = std::path::Path::new(&tool.exe)
            .parent()
            .map(|d| d.to_path_buf())
        else {
            return;
        };
        let c = &mut self.console;
        c.push(format!("$ {} — {}", tool.name, dir.display()));
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
            c.push(format!("   {n:<w$}  {:>8.1} KB", size as f64 / 1024.0));
        }
        match runner::copy_to_clipboard(&dir.to_string_lossy()) {
            Ok(()) => c.push(":: folder path copied to the clipboard"),
            Err(e) => c.push(format!("[err] couldn't copy the path: {e}")),
        }
        self.set_focus(0);
    }

    /// Open `url` in the default browser after `delay_s` seconds; a failure
    /// is reported in terminal `sid`.
    fn open_url_later(&self, sid: u64, url: String, delay_s: u64) {
        let tx = self.tx.clone();
        std::thread::spawn(move || {
            std::thread::sleep(Duration::from_secs(delay_s));
            if let Err(e) = runner::open_url(&url) {
                let line = format!("[err] couldn't open {url}: {e}");
                let _ = tx.send(AppEvent::Output { sid, line });
            }
        });
    }

    // ------------------------------------------------------------ web view

    /// Start the web view (no-op if it's on). With `open`, also open it in
    /// the default browser.
    fn start_web(&mut self, open: bool) {
        if self.web.is_none() {
            // the network gets HTTPS only: load (or make) the certificate
            let identity = if self.web_lan {
                // tests never touch the real %APPDATA%
                let dir = if cfg!(test) { None } else { tls::default_dir() };
                match tls::load_or_create(dir.as_deref()) {
                    Ok(id) => Some(id),
                    Err(e) => {
                        self.console
                            .push(format!("[err] couldn't set up HTTPS for LAN mode: {e:#}"));
                        self.set_focus(0);
                        return;
                    }
                }
            } else {
                None
            };
            match web::Server::start(
                self.web_port,
                self.web_lan,
                self.web_hello(),
                self.tx.clone(),
                identity,
            ) {
                Ok(srv) => {
                    let c = &mut self.console;
                    c.push(format!(":: web view on {}", srv.login_url()));
                    if srv.lan() {
                        match srv.lan_url() {
                            Some(u) => c.push(format!(":: on your network: {u} (HTTPS only)")),
                            None => c.push(":: on your network: no LAN address found"),
                        }
                        if let Some(fp) = srv.fingerprint() {
                            c.push(format!(
                                ":: certificate SHA-256 {}… (guests can compare it on the \
                                 browser's warning page)",
                                &fp[..fp.len().min(23)]
                            ));
                        }
                        c.push(
                            ":: guests need a one-time join key and your OK: L shows the key \
                             and who's waiting",
                        );
                    }
                    c.push(":: same session as this terminal · o opens it · w stops it");
                    self.status_line = format!(
                        "web view on {}:{}",
                        if srv.lan() {
                            "your network"
                        } else {
                            "127.0.0.1"
                        },
                        srv.port()
                    );
                    if srv.lan() {
                        match firewall::open(srv.port()) {
                            Ok(()) => {
                                self.fw_port = Some(srv.port());
                                self.fw_note.clear();
                                self.console.push(format!(
                                    ":: firewall: opened TCP {} (private) as {}; it closes                                      when LAN mode ends",
                                    srv.port(),
                                    firewall::rule_name(srv.port())
                                ));
                            }
                            Err(e) => {
                                self.fw_note = e.clone();
                                self.console.push(format!(":: firewall: {e}"));
                            }
                        }
                    }
                    self.web = Some(srv);
                    self.web_sent.clear();
                    self.web_logs.clear();
                    self.web_state.clear();
                    self.web_admin.clear();
                    self.join_later.clear();
                    self.set_focus(0);
                    self.lan_url = self.web.as_ref().and_then(web::Server::lan_url);
                    self.mirror_lan();
                    // LAN: show the key and the address right away
                    if self.lan_key.is_some() {
                        self.lan_panel = Some(0);
                    }
                }
                Err(e) => {
                    let port = self.web_port.unwrap_or(web::DEFAULT_PORT);
                    self.console.push(format!(
                        "[err] couldn't start the web view on port {port}: {e}"
                    ));
                    self.set_focus(0);
                    return;
                }
            }
        }
        if open {
            if let Some(url) = self.web.as_ref().map(|w| w.login_url()) {
                if let Err(e) = runner::open_url(&url) {
                    self.console
                        .push(format!("[err] couldn't open the browser: {e}"));
                }
            }
        }
    }

    /// `w`: start the web view on this machine, or stop it. `W`: the same
    /// on your network (LAN), or switch a running one between the two.
    fn toggle_web(&mut self, lan: bool) {
        if let Some(was_lan) = self.web.as_ref().map(web::Server::lan) {
            self.fw_close("closed the LAN firewall opening");
            self.web = None; // every open tab shows "session ended"
            self.web_toasts.clear();
            self.mirror_lan();
            // W on a local web view switches it to the LAN; anything else stops
            if !lan || was_lan {
                self.console.push(":: web view stopped");
                self.status_line = "web view stopped".into();
                return;
            }
        }
        self.web_lan = lan;
        self.start_web(false);
    }

    /// Remove winsploit's LAN firewall rule, if one is open. `ok` is the
    /// console line on success. Terminal-only: never reachable from the web.
    fn fw_close(&mut self, ok: &str) {
        let Some(port) = self.fw_port else { return };
        match firewall::close(port) {
            Ok(()) => {
                self.fw_port = None;
                self.console.push(format!(":: firewall: {ok}"));
            }
            Err(e) => {
                self.fw_note = e.clone();
                self.console.push(format!(":: firewall: {e}"));
            }
        }
    }

    /// Catalog data the page needs once: sent when a browser connects.
    fn web_hello(&self) -> String {
        let tools: Vec<serde_json::Value> = self
            .tools
            .iter()
            .map(|t| {
                serde_json::json!({
                    "name": t.name,
                    "categories": t.categories,
                    "primary": t.primary(),
                    "description": t.description,
                    "run_cmd": t.run_cmd,
                    "args_hint": t.args_hint,
                    "launch": match t.launch {
                        Launch::Stream => "stream",
                        Launch::Gui => "gui",
                        Launch::Web => "web",
                        Launch::Dataset => "dataset",
                    },
                    "install": match t.install {
                        Install::Scoop => "scoop",
                        Install::Pipx => "pipx",
                    },
                    "note": t.note,
                    "options": t.options.iter().map(|o| serde_json::json!({
                        "flag": o.flag, "arg": o.arg, "help": o.help,
                    })).collect::<Vec<_>>(),
                })
            })
            .collect();
        let tabs: Vec<serde_json::Value> = self
            .cat_tabs
            .iter()
            .map(|tab| {
                let (key, blurb) = match tab {
                    CatTab::All => ("all".to_string(), "all tools, grouped by main category"),
                    CatTab::Installed => ("installed".into(), "tools installed on this machine"),
                    CatTab::Cat(id) => (
                        format!("cat:{id}"),
                        registry::CATEGORIES
                            .iter()
                            .find(|c| c.id == id)
                            .map_or("", |c| c.blurb),
                    ),
                };
                serde_json::json!({ "key": key, "label": tab.label(), "blurb": blurb })
            })
            .collect();
        let rank: Vec<usize> = self
            .tools
            .iter()
            .map(|t| registry::category_rank(t.primary()))
            .collect();
        serde_json::json!({
            "version": env!("CARGO_PKG_VERSION"),
            "catalog": self.catalog_path.display().to_string(),
            "slots": SLOTS,
            "tools": tools,
            "tabs": tabs,
            "rank": rank,
        })
        .to_string()
    }

    /// Everything that changes, as the page sees it.
    fn web_state_json(&self) -> String {
        let status: Vec<&str> = self
            .status
            .iter()
            .map(|s| match s {
                ToolStatus::Unknown => "unknown",
                ToolStatus::Installed => "installed",
                ToolStatus::Missing => "missing",
                ToolStatus::Conflict => "conflict",
            })
            .collect();
        let conflicts: Vec<Option<String>> = self
            .conflicts
            .iter()
            .map(|c| c.as_ref().map(|p| p.display().to_string()))
            .collect();
        let terminals: Vec<serde_json::Value> = (0..=SLOTS)
            .map(|pos| match self.session(pos) {
                None => serde_json::Value::Null,
                Some(s) => {
                    let (state, code) = match s.state {
                        RunState::Idle => ("idle", None),
                        RunState::Running => ("running", None),
                        RunState::Exited(c) => ("exited", c),
                        RunState::Killed => ("killed", None),
                    };
                    serde_json::json!({
                        "pos": pos,
                        "sid": s.id,
                        "title": s.title,
                        "subtitle": s.subtitle,
                        "state": state,
                        "code": code,
                        "failed": s.failed(),
                        "pid": s.pid,
                        "stdin": s.has_stdin(),
                        "log": s.log_path.is_some(),
                    })
                }
            })
            .collect();
        let history: BTreeMap<&str, &[String]> = self
            .tools
            .iter()
            .filter_map(|t| {
                let h = self.history.list(&t.name);
                (!h.is_empty()).then_some((t.name.as_str(), h))
            })
            .collect();
        // each tool's recipes, filled from the active challenge ({gaps} left)
        let v = self.recipe_values(false);
        let recipes: serde_json::Map<String, serde_json::Value> = self
            .tools
            .iter()
            .filter(|t| !t.recipes.is_empty())
            .map(|t| {
                let list: Vec<serde_json::Value> = t
                    .recipes
                    .iter()
                    .map(|r| {
                        serde_json::json!({
                            "name": r.name,
                            "args": crate::recipe::fill_partial(&r.args, &v),
                        })
                    })
                    .collect();
                (t.name.clone(), serde_json::Value::Array(list))
            })
            .collect();
        serde_json::json!({
            "status": status,
            "conflicts": conflicts,
            "terminals": terminals,
            "installing": self.installing,
            "detecting": self.detecting,
            "status_line": self.status_line,
            // the working folder, shown read-only (`d` in the terminal changes
            // it); the profile reads as ~ so guests don't see the user name
            "cwd": crate::folder::short(&self.cwd),
            "history": history,
            "challenge": self.challenge.as_ref().map(|w| serde_json::json!({
                "id": w.id,
                "event": w.info.event,
                "name": w.info.name,
                "target": w.info.target,
                "flag_format": w.info.flag_format,
                "files": w.files(),
                // so the command helper can offer full paths to them
                "files_dir": w.files_dir(),
            })),
            "recipes": recipes,
            "challenges": self
                .challenge_list
                .iter()
                .map(|w| serde_json::json!({ "id": w.id, "title": w.title() }))
                .collect::<Vec<_>>(),
            "flags": self
                .flags
                .iter()
                .map(|f| serde_json::json!({
                    "text": f.text,
                    "known": f.known,
                    "source": f.source,
                    "at": f.at,
                }))
                .collect::<Vec<_>>(),
            "viewers": self.web.as_ref().map_or(0, |w| w.viewers()),
            "lan": self.web.as_ref().is_some_and(|w| w.lan()),
        })
        .to_string()
    }

    /// Once per frame: send each terminal's new output, new run logs,
    /// toasts and (if it changed) the state.
    fn sync_web(&mut self) {
        if self.web.is_none() {
            return;
        }
        self.mirror_lan();
        // what each open terminal still owes the browser:
        // (sid, reset, new lines, seq now, a log to offer)
        let mut owed = Vec::new();
        for pos in 0..=SLOTS {
            let Some(s) = self.session(pos) else { continue };
            let (reset, lines) = match self.web_sent.get(&s.id) {
                None => (true, s.output.clone()),
                Some(&sent) if s.seq > sent => {
                    let k = ((s.seq - sent) as usize).min(s.output.len());
                    (false, s.output[s.output.len() - k..].to_vec())
                }
                _ => (false, Vec::new()),
            };
            let log = s
                .log_path
                .clone()
                .filter(|p| self.web_logs.get(&s.id) != Some(p));
            owed.push((s.id, reset, lines, s.seq, log));
        }
        let open: Vec<u64> = owed.iter().map(|o| o.0).collect();
        let toasts = std::mem::take(&mut self.web_toasts);
        let state = self.web_state_json();

        let Some(w) = self.web.as_ref() else { return };
        for (sid, reset, lines, seq, log) in owed {
            w.push_lines(sid, reset, lines);
            self.web_sent.insert(sid, seq);
            if let Some(p) = log {
                w.set_log(sid, p.clone());
                self.web_logs.insert(sid, p);
            }
        }
        if self.web_sent.len() != open.len() {
            self.web_sent.retain(|sid, _| open.contains(sid));
            self.web_logs.retain(|sid, _| open.contains(sid));
            w.retain_terms(&open);
        }
        for (kind, text, owner_only) in toasts {
            let k = match kind {
                ToastKind::Ok => "ok",
                ToastKind::Err => "err",
                ToastKind::Info => "info",
            };
            w.toast(k, &text, owner_only);
        }
        if w.lan() {
            let admin = w.admin_json();
            if admin != self.web_admin {
                w.push_admin(&admin);
                self.web_admin = admin;
            }
        }
        if state != self.web_state {
            w.set_state(state.clone());
            self.web_state = state;
        }
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

    /// Confirmation for the packs setup.exe left behind: y/Enter queues the
    /// installs in the console, n/Esc drops them. The file is gone either way.
    fn on_key_first_run(&mut self, code: KeyCode) {
        let yes = match code {
            KeyCode::Char('y') | KeyCode::Char('Y') | KeyCode::Enter => true,
            KeyCode::Char('n') | KeyCode::Char('N') | KeyCode::Esc => false,
            _ => return,
        };
        let Some(pending) = self.first_run.take() else {
            return;
        };
        let names = pending.finish();
        if !yes {
            self.console
                .push(":: skipped the installer's tool packs (i installs a tool any time)");
            return;
        }
        self.set_focus(0);
        self.console.push(format!(
            ":: installing {} tools chosen in setup",
            names.len()
        ));
        self.install_queue = self
            .tools
            .iter()
            .enumerate()
            .filter(|(i, t)| {
                names.contains(&t.name) && self.status.get(*i) != Some(&ToolStatus::Installed)
            })
            .map(|(i, _)| i)
            .collect();
        self.next_queued_install();
    }

    /// Start the next queued install once the console is free.
    fn next_queued_install(&mut self) {
        while !self.installing && !self.console.running() {
            let Some(i) = self.install_queue.pop_front() else {
                break;
            };
            if self.status[i] != ToolStatus::Installed {
                self.install(i, Source::Terminal);
            }
        }
    }

    /// Installs run in the console, one at a time, so they never take a
    /// tool terminal.
    fn install(&mut self, idx: usize, src: Source) {
        if self.console.running() {
            self.status_line = "an install is already running (see console, 0)".into();
            if src.is_web() {
                self.toast(ToastKind::Err, "an install is already running");
            }
            return;
        }
        let tool = self.tools[idx].clone();
        let name = tool.name.clone();

        let pipx = detect::resolve("pipx").is_some();
        let pre = install::Prereqs {
            scoop: detect::resolve("scoop").is_some(),
            pipx,
            // only worth the subprocess when pipx has to be bootstrapped
            python: tool.install == Install::Pipx && !pipx && Self::python_works(),
            bucket_added: tool.bucket.as_deref().is_some_and(scoop::bucket_added),
        };
        self.set_focus(0);
        let steps = match install::plan(&tool, pre, &self.buckets) {
            Ok(s) => s,
            Err(e) => {
                self.console
                    .push(format!("[err] can't install {name}: {e}"));
                return;
            }
        };
        let line = steps.join(" && ");
        if let Some(by) = self.started_by(src) {
            self.console
                .push(format!(":: install of {name} started {by}"));
        }
        self.console.push(format!("$ {line}"));

        // scoop and pipx are .cmd/.bat shims, so go through cmd. Every token
        // in `line` was checked by install::plan to be free of cmd
        // metacharacters, and `&&` stops at the first failing step. /D skips
        // any AutoRun commands from the registry.
        let mut cmd = std::process::Command::new("cmd");
        {
            use std::os::windows::process::CommandExt;
            cmd.raw_arg(format!("/D /C {line}"));
        }
        match runner::spawn_streaming(cmd, self.tx.clone(), false, CONSOLE) {
            Ok(sp) => {
                let dir = self.logs_dir();
                let by = self.source_label(src);
                self.console
                    .open_log(&dir, &format!("install-{name}"), &line, &by);
                self.console.started(sp.pid, None);
                self.installing = true;
                self.status_line = format!("installing {name}…");
            }
            Err(e) => self
                .console
                .push(format!("[err] failed to start the installer: {e}")),
        }
    }

    // ---------------------------------------------------------------- mouse

    /// Mouse: clicks and the wheel, hit-tested against what the last frame
    /// drew (`hits`). A click does what the key it stands for does, so the
    /// mouse, the keyboard and the web view share one code path.
    fn on_mouse(&mut self, m: MouseEvent) {
        let hit = self.hit_at(m.column, m.row);
        match m.kind {
            MouseEventKind::Down(MouseButton::Left) => {
                let double = self.double_click(m.column, m.row, Instant::now());
                self.on_click(hit, m.column, m.row, double);
            }
            MouseEventKind::ScrollUp => self.on_wheel(hit, true),
            MouseEventKind::ScrollDown => self.on_wheel(hit, false),
            _ => {}
        }
    }

    /// The topmost thing drawn at (`x`, `y`).
    fn hit_at(&self, x: u16, y: u16) -> Option<Hit> {
        let pos = ratatui::layout::Position { x, y };
        self.hits
            .borrow()
            .iter()
            .rev()
            .find(|(r, _)| r.contains(pos))
            .map(|&(_, h)| h)
    }

    /// Is this click the second of a double-click?
    fn double_click(&mut self, x: u16, y: u16, now: Instant) -> bool {
        let double = self.last_click.is_some_and(|(t, lx, ly)| {
            (lx, ly) == (x, y) && now.saturating_duration_since(t) <= DOUBLE_CLICK
        });
        // a third click starts over
        self.last_click = if double { None } else { Some((now, x, y)) };
        double
    }

    /// The overlay that keys go to right now, if one is open (the same
    /// order as `on_key`).
    fn layer(&self) -> Option<Layer> {
        if self.tour.is_some() {
            Some(Layer::Tour)
        } else if self.first_run.is_some() {
            Some(Layer::FirstRun)
        } else if self.join_prompt.is_some() {
            Some(Layer::Join)
        } else if self.lan_panel.is_some() {
            Some(Layer::Lan)
        } else if self.appearance.is_some() {
            Some(Layer::Appearance)
        } else if self.flags_view.is_some() {
            Some(Layer::Flags)
        } else if self.challenge_view.is_some() {
            Some(Layer::Challenges)
        } else if self.recipe_view.is_some() {
            Some(Layer::Recipes)
        } else if self.provider_pick.is_some() {
            Some(Layer::Provider)
        } else if self.agent.is_some() {
            Some(Layer::Agent)
        } else if self.show_help {
            Some(Layer::Help)
        } else {
            None
        }
    }

    /// Press `code`, as if typed.
    fn press(&mut self, code: KeyCode) {
        self.on_key(KeyEvent::new(code, KeyModifiers::NONE));
    }

    fn on_click(&mut self, hit: Option<Hit>, x: u16, y: u16, double: bool) {
        // the header agent icon runs the agent, or closes its overlay
        let on_icon = self
            .agent_icon
            .get()
            .is_some_and(|r| r.contains(ratatui::layout::Position { x, y }));
        if on_icon && matches!(self.layer(), None | Some(Layer::Agent)) {
            if self.agent.is_some() {
                self.agent = None;
            } else {
                self.start_analysis();
            }
            return;
        }
        if let Some(layer) = self.layer() {
            self.click_layer(layer, hit);
            return;
        }
        match self.mode {
            Mode::Browsing => {}
            // all terminals busy: clicking one picks it
            Mode::SlotPick => {
                if let Some(Hit::Term(pos @ 1..=SLOTS)) = hit {
                    self.press(KeyCode::Char((b'0' + pos as u8) as char));
                }
                return;
            }
            // a prompt is open: the keyboard has it
            _ => return,
        }
        // a pending confirmation only survives until the next click, unless
        // the click is the confirmation (or presses the key again)
        let confirm = self.confirm.take();
        match hit {
            Some(Hit::Confirm) => self.confirm_now(confirm),
            Some(Hit::Key(code)) => {
                self.confirm = confirm;
                self.press(code);
            }
            Some(Hit::Tool(i)) => {
                self.selected = i;
                if double {
                    self.press(KeyCode::Enter);
                }
            }
            Some(Hit::Category(i)) => {
                self.cat = i.min(self.cat_tabs.len().saturating_sub(1));
                self.step_category(0);
            }
            Some(Hit::Term(pos)) if pos != self.focus => self.set_focus(pos),
            Some(Hit::Copy(pos)) => self.apply(Action::Copy(pos), Source::Terminal),
            _ => {}
        }
    }

    /// The "press again" prompt was clicked: do what it asked about.
    fn confirm_now(&mut self, confirm: Option<Confirm>) {
        match confirm {
            Some(Confirm::Close(pos)) => self.apply(Action::Close(pos), Source::Terminal),
            Some(Confirm::Quit) => self.quit = true,
            None => {}
        }
    }

    /// A click while overlay `layer` is open: on one of its entries it picks
    /// that entry; outside its box it closes it (as Esc does).
    fn click_layer(&mut self, layer: Layer, hit: Option<Hit>) {
        match (layer, hit) {
            // any click closes the key list, as any key does
            (Layer::Help, _) => self.press(KeyCode::Esc),
            // the welcome card: a click is "next" (Esc still skips it)
            (Layer::Tour, _) => self.press(KeyCode::Enter),
            // installs or drops the packs: a stray click mustn't decide
            (Layer::FirstRun, _) => {}
            (_, Some(Hit::Row(l, n))) if l == layer => self.click_row(layer, n),
            (_, Some(Hit::Popup(l))) if l == layer => {}
            _ => self.press(KeyCode::Esc),
        }
    }

    /// Entry `n` of an overlay was clicked: the first click highlights it, a
    /// click on the highlighted one does what Enter does.
    fn click_row(&mut self, layer: Layer, n: usize) {
        /// Highlight `n`; true when it already was.
        fn pick(cur: &mut Option<usize>, n: usize) -> bool {
            let again = *cur == Some(n);
            *cur = Some(n);
            again
        }
        let enter = match layer {
            Layer::Recipes => pick(&mut self.recipe_view, n),
            Layer::Flags => pick(&mut self.flags_view, n),
            Layer::Challenges => pick(&mut self.challenge_view, n),
            // guests have no Enter: v / f / x act on the highlighted one
            Layer::Lan => {
                self.lan_panel = Some(n);
                false
            }
            Layer::Join => match self.join_prompt.as_mut() {
                // the same grace as for keys: a stray click can't answer it
                Some(p) if p.since.elapsed() >= JOIN_GRACE => {
                    let again = p.cursor == n;
                    p.cursor = n.min(JOIN_CHOICES.len() - 1);
                    again
                }
                _ => false,
            },
            Layer::Provider => match self.provider_pick.as_mut() {
                Some(p) if p.editing.is_none() => {
                    let again = p.cursor == n;
                    p.cursor = n.min(agent::Provider::ALL.len() - 1);
                    again
                }
                _ => false,
            },
            Layer::Appearance => {
                let fields = Field::ALL.len();
                let Some(ap) = self.appearance.as_mut() else {
                    return;
                };
                if n < fields {
                    // a setting: highlight it, then each click steps it
                    if ap.cursor == n {
                        self.settings.step(Field::ALL[n], 1);
                    }
                    ap.cursor = n;
                } else if let Some(t) = crate::theme::ThemeName::ALL.get(n - fields) {
                    // a theme from the numbered list, as 1-8 picks it
                    self.settings.theme = *t;
                    ap.cursor = 0;
                }
                false
            }
            Layer::Tour | Layer::FirstRun | Layer::Agent | Layer::Help => false,
        };
        if enter {
            self.press(KeyCode::Enter);
        }
    }

    /// The wheel: over the tool list it moves the selection, over a terminal
    /// it scrolls that one, elsewhere the shown one. In an overlay it moves
    /// the highlight.
    fn on_wheel(&mut self, hit: Option<Hit>, up: bool) {
        if let Some(layer) = self.layer() {
            // in these a key closes, skips or answers
            if !matches!(layer, Layer::Help | Layer::Tour | Layer::FirstRun) {
                self.press(if up { KeyCode::Up } else { KeyCode::Down });
            }
            return;
        }
        let pos = match hit {
            Some(Hit::Tool(_) | Hit::ToolList) => {
                // not while typing args: there ↑↓ is history, not the list
                if matches!(self.mode, Mode::Browsing | Mode::Filter) {
                    self.move_selection(if up { -3 } else { 3 });
                }
                return;
            }
            Some(Hit::Term(pos) | Hit::Copy(pos)) => pos,
            _ => self.focus,
        };
        if let Some(s) = self.session_mut(pos) {
            if up {
                let max = s.view.get().1;
                s.scroll = (s.scroll + 3).min(max);
            } else {
                s.scroll = s.scroll.saturating_sub(3);
            }
        }
    }

    // ---- agent (`a` key / header icon) --------------------------------

    /// Kick off an analysis of the current output with the chosen provider.
    fn start_analysis(&mut self) {
        if self.agent_busy {
            self.status_line = "already analysing the output…".into();
            return;
        }
        let provider = self.settings.agent;
        if let Some((detail, status)) = Self::provider_problem(provider) {
            self.console.push(format!("[err] {detail}"));
            self.status_line = status;
            return;
        }
        let tail = self.for_the_ai(self.output_tail(150, 6000));
        if tail.trim().is_empty() {
            self.status_line = "no output to analyse yet — run a tool first".into();
            return;
        }
        let ran = self
            .last_command()
            .unwrap_or_else(|| "the current session".into());
        let names: Vec<String> = self.tools.iter().map(|t| t.name.clone()).collect();
        let model = self.effective_model(provider);
        self.agent_busy = true;
        self.status_line = format!("analysing with {} ({model})…", provider.short());
        agent::analyze(provider, model, names, ran, tail, self.tx.clone());
    }

    /// Why `provider` can't be used right now: (what to tell the user, the
    /// status line), or None when it's ready.
    fn provider_problem(provider: agent::Provider) -> Option<(String, String)> {
        match provider.availability() {
            agent::Availability::Ready => None,
            agent::Availability::NeedsCli(exe) => Some((
                format!(
                    "{} needs the `{exe}` CLI on PATH. Install it, or press A to pick \
                     another provider.",
                    provider.short()
                ),
                format!("{} unavailable (press A)", provider.short()),
            )),
            agent::Availability::NeedsKey(env) => Some((
                format!(
                    "{} needs the {env} environment variable set. Set it (then restart \
                     winsploit), or press A to pick another provider.",
                    provider.short()
                ),
                format!("{} needs {env} (press A)", provider.short()),
            )),
            agent::Availability::NeedsCurl => Some((
                "HTTP providers need `curl` on PATH (ships with Windows 10/11).".to_string(),
                "curl not found".to_string(),
            )),
        }
    }

    /// Terminal text on its way to an AI provider, with secrets masked
    /// unless the user turned `redact:` off.
    fn for_the_ai(&self, text: String) -> String {
        if self.settings.redact == crate::redact::Level::Off {
            text
        } else {
            crate::redact::text(&text)
        }
    }

    /// The model used for `provider`: the user's override if set, else the
    /// provider's built-in default.
    pub fn effective_model(&self, provider: agent::Provider) -> String {
        self.settings
            .agent_models
            .get(provider.short())
            .map(|s| s.trim())
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .unwrap_or_else(|| provider.default_model().to_string())
    }

    /// Keys while the agent-provider picker (`A`) is open.
    fn on_key_provider(&mut self, code: KeyCode) {
        let Some(pick) = self.provider_pick.as_mut() else {
            return;
        };
        let n = agent::Provider::ALL.len();
        let provider = agent::Provider::ALL[pick.cursor];

        // Sub-mode: typing a model override for the highlighted provider.
        if let Some(ed) = pick.editing.as_mut() {
            match code {
                KeyCode::Esc => pick.editing = None,
                KeyCode::Enter => {
                    let text = ed.text().trim().to_string();
                    pick.editing = None;
                    if text.is_empty() {
                        self.settings.agent_models.remove(provider.short());
                    } else {
                        self.settings
                            .agent_models
                            .insert(provider.short().to_string(), text);
                    }
                    self.save_settings_status(&format!("{} model set", provider.short()));
                }
                KeyCode::Backspace => ed.backspace(),
                KeyCode::Left => ed.left(),
                KeyCode::Right => ed.right(),
                KeyCode::Home => ed.home(),
                KeyCode::End => ed.end(),
                KeyCode::Char(c) => ed.insert(c),
                _ => {}
            }
            return;
        }

        match code {
            KeyCode::Up | KeyCode::Char('k') => pick.cursor = (pick.cursor + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') => pick.cursor = (pick.cursor + 1) % n,
            KeyCode::Esc | KeyCode::Char('q') => self.provider_pick = None,
            // m: edit this provider's model; r: reset it to the default
            KeyCode::Char('m') => {
                let mut ed = LineEditor::default();
                ed.set(&self.effective_model(provider));
                if let Some(pick) = self.provider_pick.as_mut() {
                    pick.editing = Some(ed);
                }
            }
            KeyCode::Char('r') => {
                self.settings.agent_models.remove(provider.short());
                self.save_settings_status(&format!(
                    "{} model reset to {}",
                    provider.short(),
                    provider.default_model()
                ));
            }
            KeyCode::Enter => {
                self.provider_pick = None;
                self.settings.agent = provider;
                self.save_settings_status(&format!(
                    "agent provider: {} ({})",
                    provider.short(),
                    provider.status_note()
                ));
            }
            _ => {}
        }
    }

    /// Persist settings and set the status line (or report a save error).
    fn save_settings_status(&mut self, ok_msg: &str) {
        match self.settings.save() {
            Ok(_) => self.status_line = ok_msg.to_string(),
            Err(e) => {
                self.status_line = format!("{ok_msg} (not saved)");
                self.console
                    .push(format!("[err] couldn't save settings: {e}"));
            }
        }
    }

    /// The output of the terminal in front — what the user is looking at.
    fn visible_output(&self) -> &[String] {
        match self.focused() {
            Some(s) => &s.output,
            None => &self.console.output,
        }
    }

    /// The most recent command echoed in the focused terminal (a `$ ` line).
    fn last_command(&self) -> Option<String> {
        self.visible_output()
            .iter()
            .rev()
            .find_map(|l| l.strip_prefix("$ ").map(|s| s.to_string()))
    }

    /// The last `max_lines` of the focused terminal, trimmed to `max_chars`.
    fn output_tail(&self, max_lines: usize, max_chars: usize) -> String {
        let out = self.visible_output();
        let start = out.len().saturating_sub(max_lines);
        let joined = out[start..].join("\n");
        let n = joined.chars().count();
        if n <= max_chars {
            joined
        } else {
            joined.chars().skip(n - max_chars).collect()
        }
    }

    fn on_agent_result(&mut self, res: Result<agent::Analysis, String>) {
        self.agent_busy = false;
        match res {
            Ok(a) => {
                let n = a.suggestions.len();
                self.status_line = if n > 0 {
                    format!("agent: {n} suggestion(s) · ↑↓ pick · Enter run · Esc close")
                } else {
                    "agent: analysis ready (no tool suggestions)".into()
                };
                self.agent = Some(AgentView {
                    analysis: a,
                    cursor: 0,
                });
            }
            Err(e) => {
                self.console.push(format!("[err] agent: {e}"));
                self.status_line = "agent failed (see output)".into();
            }
        }
    }

    /// Keys while the agent overlay is open.
    fn on_key_agent(&mut self, code: KeyCode) {
        let Some(ag) = self.agent.as_mut() else {
            return;
        };
        let n = ag.analysis.suggestions.len();
        match code {
            KeyCode::Esc | KeyCode::Char('q') | KeyCode::Char('a') => self.agent = None,
            KeyCode::Up | KeyCode::Char('k') if n > 0 => ag.cursor = (ag.cursor + n - 1) % n,
            KeyCode::Down | KeyCode::Char('j') if n > 0 => ag.cursor = (ag.cursor + 1) % n,
            KeyCode::Enter if n > 0 => {
                let s = ag.analysis.suggestions[ag.cursor].clone();
                self.run_suggestion(s);
            }
            _ => {}
        }
    }

    /// Take a suggested next step into the normal args prompt: pick its tool,
    /// prefill the args, and let the user review and press Enter to run it —
    /// so every run still goes through the install / conflict / arg checks.
    fn run_suggestion(&mut self, s: agent::Suggestion) {
        match self
            .tools
            .iter()
            .position(|t| t.name.eq_ignore_ascii_case(s.tool.trim()))
        {
            Some(i) => {
                self.agent = None;
                self.selected = i;
                // make sure the chosen tool is visible so the selection is valid:
                // drop any filter and jump to the "All" category tab.
                if !self.visible().contains(&i) {
                    self.filter.clear();
                    self.cat = 0;
                }
                self.editor.set(&s.args);
                self.history.reset();
                self.mode = Mode::Input;
                self.status_line = format!(
                    "agent → {} · review args, Enter runs · Esc cancels",
                    self.tools[i].name
                );
            }
            None => {
                self.status_line = format!("agent suggested an unknown tool: '{}'", s.tool);
            }
        }
    }

    // ---- chat (`T`) ---------------------------------------------------
    //
    // Terminal only, on purpose (like Sandbox runs): the web view must never be
    // able to make an AI choose commands, least of all for a LAN guest.

    /// Show the chat panel and put the keyboard in it.
    fn open_chat(&mut self) {
        self.chat.open = true;
        self.chat.focused = true;
        if self.chat.msgs.is_empty() {
            let then = if self.settings.chat_autorun {
                "and run it right away"
            } else {
                "and ask before running it"
            };
            self.chat.say(
                ChatRole::Note,
                format!(
                    "Tell me what to do in plain words, e.g. \"scan 10.0.0.5 for open ports\". \
                     I pick the tool from your catalog {then}. Esc goes back to the dashboard, \
                     Ctrl+T hides me, /clear starts over."
                ),
            );
        }
        self.status_line =
            "chat: type what you want · Enter sends · Esc back to the dashboard".into();
    }

    fn close_chat(&mut self) {
        self.chat.open = false;
        self.chat.focused = false;
    }

    /// Keys while the chat input has the keyboard.
    fn on_key_chat(&mut self, key: KeyEvent) {
        let ctrl = key.modifiers.contains(KeyModifiers::CONTROL);
        let alt = key.modifiers.contains(KeyModifiers::ALT);
        // AltGr arrives as Ctrl+Alt on Windows: those characters are text
        let altgr = ctrl && alt;
        let ctrl = ctrl && !altgr;
        let page = 8;
        let e = &mut self.chat.input;
        match key.code {
            KeyCode::Esc => self.chat_escape(),
            KeyCode::Enter => self.chat_enter(),
            KeyCode::PageUp => self.chat.scroll += page,
            KeyCode::PageDown => self.chat.scroll = self.chat.scroll.saturating_sub(page),
            KeyCode::Up => self.chat.scroll += 1,
            KeyCode::Down => self.chat.scroll = self.chat.scroll.saturating_sub(1),
            KeyCode::Left if ctrl => e.word_left(),
            KeyCode::Right if ctrl => e.word_right(),
            KeyCode::Left => e.left(),
            KeyCode::Right => e.right(),
            KeyCode::Home => e.home(),
            KeyCode::End => e.end(),
            KeyCode::Backspace if ctrl => e.delete_word_left(),
            KeyCode::Backspace => e.backspace(),
            KeyCode::Delete => e.delete(),
            KeyCode::Char('u') if ctrl => e.clear_to_start(),
            KeyCode::Char('w') if ctrl => e.delete_word_left(),
            KeyCode::Char('a') if ctrl => e.home(),
            KeyCode::Char('e') if ctrl => e.end(),
            KeyCode::Char(c) if altgr => e.insert(c),
            // other Ctrl/Alt chords aren't text
            KeyCode::Char(_) if ctrl || alt => {}
            KeyCode::Char(c) => e.insert(c),
            _ => {}
        }
        self.chat.scroll = self.chat.scroll.min(self.chat.max_scroll.get());
    }

    /// Esc, one step at a time: stop waiting, clear the line, skip the plan,
    /// then hand the keyboard back to the dashboard.
    fn chat_escape(&mut self) {
        if self.chat.busy {
            self.chat.req += 1; // the answer, if it still comes, is dropped
            self.chat.busy = false;
            self.chat
                .say(ChatRole::Note, "stopped waiting for the answer");
        } else if !self.chat.input.text().is_empty() {
            self.chat.input.set("");
        } else if self.chat.pending().is_some() {
            self.chat.skip_pending();
            self.chat.say(ChatRole::Note, "skipped, nothing was run");
        } else {
            self.chat.focused = false;
            self.status_line = "chat still open · T talks again · Ctrl+T hides it".into();
        }
    }

    /// Enter: send what's typed, or (empty line) run the plan waiting for it.
    fn chat_enter(&mut self) {
        let text = self.chat.input.text().trim().to_string();
        if text.is_empty() {
            if self.chat.pending().is_some() {
                self.run_plan();
            }
            return;
        }
        match text.as_str() {
            "/clear" => {
                self.chat.input.set("");
                self.chat.clear();
            }
            "/close" => {
                self.chat.input.set("");
                self.close_chat();
            }
            _ => self.send_chat(text),
        }
    }

    /// What the AI is told about the session: the catalog with install
    /// status, where we are, the challenge target, and the terminal in front
    /// (its output run through the redactor, like anything leaving the machine).
    fn chat_context(&self) -> chat::Context {
        fn clip(s: &str, n: usize) -> String {
            s.chars().take(n).collect()
        }
        let tools = self
            .tools
            .iter()
            .enumerate()
            .filter(|(_, t)| t.launch != Launch::Dataset)
            .map(|(i, t)| chat::ToolInfo {
                name: t.name.clone(),
                status: match self.status[i] {
                    ToolStatus::Installed => "installed",
                    ToolStatus::Missing => "missing",
                    ToolStatus::Conflict => "conflict",
                    ToolStatus::Unknown => "unknown",
                },
                what: t.description.clone(),
                example: clip(t.args_hint.as_deref().unwrap_or(""), 90),
                flags: t
                    .options
                    .iter()
                    .take(6)
                    .map(|o| {
                        let arg = o
                            .arg
                            .as_deref()
                            .map(|a| format!(" {a}"))
                            .unwrap_or_default();
                        clip(&format!("{}{arg} {}", o.flag, o.help), 50)
                    })
                    .collect(),
            })
            .collect();
        let tail = self.for_the_ai(self.output_tail(60, 3000));
        chat::Context {
            tools,
            cwd: crate::folder::short(&self.cwd),
            project: self.project.as_ref().map(|p| p.name.clone()),
            challenge: self
                .challenge
                .as_ref()
                .map(|w| (w.info.name.clone(), w.info.target.clone())),
            ran: self.last_command(),
            tail,
        }
    }

    /// Send the typed message to the AI along with the context.
    fn send_chat(&mut self, text: String) {
        if self.chat.busy {
            self.status_line = "still waiting for the answer · Esc stops waiting".into();
            return;
        }
        let provider = self.settings.agent;
        // (tests never talk to a real provider)
        if !cfg!(test) {
            if let Some((detail, status)) = Self::provider_problem(provider) {
                self.chat
                    .say(ChatRole::Note, format!("{detail} (Esc first, then A)"));
                self.status_line = status;
                return; // what was typed stays in the input
            }
        }
        let prompt = chat::build_prompt(&self.chat_context(), &self.chat.history(), &text);
        self.chat.input.set("");
        self.chat.skip_pending();
        self.chat.say(ChatRole::You, text);
        self.chat.busy = true;
        self.chat.req += 1;
        let model = self.effective_model(provider);
        self.status_line = format!("asking {} ({model})…", provider.short());
        if !cfg!(test) {
            agent::chat(provider, model, prompt, self.chat.req, self.tx.clone());
        }
    }

    fn on_chat_result(&mut self, req: u64, result: Result<String, String>) {
        if req != self.chat.req || !self.chat.busy {
            return; // stopped waiting, cleared, or an older request
        }
        self.chat.busy = false;
        match result {
            Err(e) => {
                self.chat
                    .say(ChatRole::Note, format!("the AI request failed: {e}"));
                self.status_line = "chat: the AI request failed".into();
            }
            Ok(text) => {
                let reply = chat::parse_reply(&text);
                match reply.run {
                    Some(spec) => self.chat_plan(reply.text, spec),
                    None if reply.text.is_empty() => self
                        .chat
                        .say(ChatRole::Agent, "(the AI had nothing to say)"),
                    None => self.chat.say(ChatRole::Agent, reply.text),
                }
            }
        }
    }

    /// Turn what the AI wants to run into a plan. Only a catalog tool with
    /// args that parse can become one; a missing tool is shown, not run.
    fn chat_plan(&mut self, text: String, spec: chat::RunSpec) {
        let wanted = spec.tool.trim();
        let Some(idx) = self
            .tools
            .iter()
            .position(|t| t.name.eq_ignore_ascii_case(wanted))
        else {
            if !text.is_empty() {
                self.chat.say(ChatRole::Agent, text);
            }
            self.chat.say(
                ChatRole::Note,
                format!(
                    "the AI picked \"{wanted}\", which isn't in your catalog, so nothing was run"
                ),
            );
            return;
        };
        let name = self.tools[idx].name.clone();
        let args = spec.args.trim().to_string();
        let state = if let Err(e) = args::split(&args) {
            PlanState::Blocked(format!("can't read the args: {e}"))
        } else {
            match self.status[idx] {
                ToolStatus::Installed => PlanState::Ready,
                ToolStatus::Missing => PlanState::Blocked(format!(
                    "{name} isn't installed: Esc, select it and press i"
                )),
                ToolStatus::Conflict => PlanState::Blocked(format!(
                    "a different program called {} comes first on PATH",
                    self.tools[idx].exe
                )),
                ToolStatus::Unknown => {
                    PlanState::Blocked("still detecting tools, ask again in a moment".into())
                }
            }
        };
        let ready = state == PlanState::Ready;
        self.chat.push(ChatMsg {
            role: ChatRole::Agent,
            text,
            plan: Some(Plan {
                tool: idx,
                name,
                args,
                why: spec.why.trim().to_string(),
                state,
            }),
        });
        if ready && self.settings.chat_autorun {
            self.run_plan();
        } else if ready {
            self.status_line = "chat: Enter runs the plan · Esc skips it".into();
        }
    }

    /// Run the plan waiting in the chat through the normal launch path (so the
    /// install / conflict / arg checks and the run log all apply).
    fn run_plan(&mut self) {
        let Some(plan) = self.chat.pending().cloned() else {
            return;
        };
        if let Some(p) = self.chat.pending_mut() {
            p.state = PlanState::Ran;
        }
        let before = self.next_id;
        self.launch(plan.tool, &plan.args, Source::Terminal, None);
        if self.mode == Mode::SlotPick {
            // every terminal is busy: that question needs the keyboard
            self.chat.focused = false;
            self.chat.say(
                ChatRole::Note,
                "all four terminals are busy: press 1-4 to replace one, or Esc",
            );
        } else if self.next_id > before {
            self.chat.watch = Some((self.next_id, plan.name.clone()));
            self.chat.say(
                ChatRole::Note,
                format!("started {} in terminal {}", plan.name, self.focus),
            );
        } else {
            // refused (not installed, ...) or a window / browser launch
            self.chat.say(ChatRole::Note, self.status_line.clone());
        }
    }

    // ---- projects (`p`) -----------------------------------------------

    fn open_project_menu(&mut self) {
        self.project_ui = Some(ProjectUi::Menu { cursor: 0 });
    }

    /// Menu rows: New project, Open folder, then the existing recents.
    fn menu_len(&self) -> usize {
        2 + self.pstore.existing_recents().len()
    }

    /// Open the folder browser, starting at the current project (or home).
    fn enter_browser(&mut self, new: bool) {
        let start = self
            .project
            .as_ref()
            .map(|p| p.path.clone())
            .filter(|p| p.is_dir())
            .unwrap_or_else(crate::project::home_dir);
        let entries = crate::project::subdirs(&start);
        self.project_ui = Some(ProjectUi::Browse {
            dir: start,
            entries,
            cursor: 0,
            new,
        });
    }

    fn on_key_project(&mut self, key: KeyEvent) {
        let code = key.code;
        let Some(ui) = self.project_ui.take() else {
            return;
        };
        match ui {
            ProjectUi::Menu { mut cursor } => {
                let n = self.menu_len().max(1);
                match code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        cursor = (cursor + n - 1) % n;
                        self.project_ui = Some(ProjectUi::Menu { cursor });
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        cursor = (cursor + 1) % n;
                        self.project_ui = Some(ProjectUi::Menu { cursor });
                    }
                    KeyCode::Esc | KeyCode::Char('q') => {} // closed
                    KeyCode::Enter => match cursor {
                        0 => self.enter_browser(true),
                        1 => self.enter_browser(false),
                        c => {
                            let recents = self.pstore.existing_recents();
                            match recents.get(c - 2).cloned() {
                                Some(path) => self.open_project(path),
                                None => self.project_ui = Some(ProjectUi::Menu { cursor }),
                            }
                        }
                    },
                    _ => self.project_ui = Some(ProjectUi::Menu { cursor }),
                }
            }
            ProjectUi::Browse {
                dir,
                entries,
                mut cursor,
                new,
            } => {
                let n = entries.len() + 1; // row 0 = ".." (parent)
                match code {
                    KeyCode::Up | KeyCode::Char('k') => {
                        cursor = (cursor + n - 1) % n;
                        self.project_ui = Some(ProjectUi::Browse { dir, entries, cursor, new });
                    }
                    KeyCode::Down | KeyCode::Char('j') => {
                        cursor = (cursor + 1) % n;
                        self.project_ui = Some(ProjectUi::Browse { dir, entries, cursor, new });
                    }
                    KeyCode::Esc => self.project_ui = Some(ProjectUi::Menu { cursor: 0 }),
                    KeyCode::Enter => {
                        if cursor == 0 {
                            match dir.parent().map(|p| p.to_path_buf()) {
                                Some(parent) => {
                                    let entries = crate::project::subdirs(&parent);
                                    self.project_ui = Some(ProjectUi::Browse {
                                        dir: parent,
                                        entries,
                                        cursor: 0,
                                        new,
                                    });
                                }
                                None => {
                                    self.project_ui =
                                        Some(ProjectUi::Browse { dir, entries, cursor, new })
                                }
                            }
                        } else if let Some(sub) = entries.get(cursor - 1).cloned() {
                            let entries = crate::project::subdirs(&sub);
                            self.project_ui = Some(ProjectUi::Browse {
                                dir: sub,
                                entries,
                                cursor: 0,
                                new,
                            });
                        } else {
                            self.project_ui =
                                Some(ProjectUi::Browse { dir, entries, cursor, new });
                        }
                    }
                    // o: choose this folder (open it, or name a new project in it)
                    KeyCode::Char('o') => {
                        if new {
                            self.project_ui = Some(ProjectUi::Name {
                                parent: dir,
                                editor: LineEditor::default(),
                            });
                        } else {
                            self.open_project(dir);
                        }
                    }
                    _ => self.project_ui = Some(ProjectUi::Browse { dir, entries, cursor, new }),
                }
            }
            ProjectUi::Name { parent, mut editor } => match code {
                KeyCode::Esc => self.enter_browser(true),
                KeyCode::Enter => {
                    let name = editor.text().trim().to_string();
                    if name.is_empty() {
                        self.project_ui = Some(ProjectUi::Name { parent, editor });
                    } else {
                        let path = parent.join(&name);
                        match std::fs::create_dir_all(&path) {
                            Ok(()) => self.open_project(path),
                            Err(e) => {
                                self.status_line = format!("couldn't create the project: {e}");
                            }
                        }
                    }
                }
                KeyCode::Backspace => {
                    editor.backspace();
                    self.project_ui = Some(ProjectUi::Name { parent, editor });
                }
                KeyCode::Left => {
                    editor.left();
                    self.project_ui = Some(ProjectUi::Name { parent, editor });
                }
                KeyCode::Right => {
                    editor.right();
                    self.project_ui = Some(ProjectUi::Name { parent, editor });
                }
                KeyCode::Char(c) => {
                    editor.insert(c);
                    self.project_ui = Some(ProjectUi::Name { parent, editor });
                }
                _ => self.project_ui = Some(ProjectUi::Name { parent, editor }),
            },
        }
    }

    /// Open `path` as the active project, record it, and persist.
    fn open_project(&mut self, path: PathBuf) {
        let proj = crate::project::Project::open(path.clone());
        self.pstore.touch(&path);
        if let Err(e) = self.pstore.save() {
            self.console
                .push(format!("[err] couldn't save the project list: {e}"));
        }
        self.console.push(format!(
            ":: project '{}' — tools now run in {}",
            proj.name,
            proj.path.display()
        ));
        self.status_line = format!("project: {} (tools run here · {{proj}} = its path)", proj.name);
        // tools run in the project folder (reuses the cwd the runner already uses)
        self.cwd = proj.path.clone();
        self.project = Some(proj);
        self.project_ui = None;
    }

    /// `c`: stop the tool in terminal `pos`; its output stays.
    fn cancel(&mut self, pos: usize) {
        match self.session_mut(pos).and_then(Session::kill) {
            Some(pid) => {
                self.status_line = format!("stopping pid {pid} · x closes the terminal");
            }
            None if pos == 0 => self.status_line = "no install is running".into(),
            None => self.status_line = "nothing is running in this terminal".into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn app() -> App {
        let cat = crate::registry::parse(include_str!("../tools.yaml")).unwrap();
        App::new(cat, PathBuf::from("tools.yaml"), None)
    }

    fn key(c: KeyCode) -> KeyEvent {
        KeyEvent::new(c, KeyModifiers::NONE)
    }

    /// A terminal holding `name`, running or finished at `ended`. No real
    /// process (pid stays None), so kill() never calls taskkill.
    fn fake(app: &mut App, slot: usize, name: &str, running: bool, ended: u64) {
        app.next_id += 1;
        let mut s = Session::new(app.next_id, name, "");
        s.state = if running {
            RunState::Running
        } else {
            RunState::Exited(Some(0))
        };
        s.ended = ended;
        app.slots[slot] = Some(s);
    }

    #[test]
    fn sandbox_key_needs_a_sandbox_ready_tool() {
        let mut a = app();
        // nmap isn't set up for the sandbox
        a.selected = a.tools.iter().position(|t| t.name == "nmap").unwrap();
        a.status[a.selected] = ToolStatus::Installed;
        a.on_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
        assert_eq!(a.mode, Mode::Browsing);
        assert!(a.status_line.contains("isn't set up"), "{}", a.status_line);
        // x64dbg is, but must be installed first
        a.cat = 0;
        a.selected = a.tools.iter().position(|t| t.name == "x64dbg").unwrap();
        a.status[a.selected] = ToolStatus::Missing;
        a.on_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
        assert!(a.status_line.contains("not installed"), "{}", a.status_line);
        // installed: the prompt opens, or the console explains how to enable it
        a.status[a.selected] = ToolStatus::Installed;
        a.on_key(KeyEvent::new(KeyCode::Char('S'), KeyModifiers::SHIFT));
        if crate::sandbox::sandbox_exe().is_some() {
            assert_eq!(a.mode, Mode::SandboxInput);
            assert!(!a.sandbox_net);
            a.on_key(key(KeyCode::Tab));
            assert!(a.sandbox_net, "Tab switches the network on");
        } else {
            assert_eq!(a.mode, Mode::Browsing);
            assert!(a
                .console
                .output
                .iter()
                .any(|l| l.contains("Containers-DisposableClientVM")));
        }
    }

    fn type_line(a: &mut App, text: &str) {
        for c in text.chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        a.on_key(key(KeyCode::Enter));
    }

    #[test]
    fn d_changes_the_working_folder_like_cd() {
        let root = std::env::temp_dir()
            .join("winsploit-app-tests")
            .join("folders");
        for d in ["htb\\Lame\\loot", "htb\\Legacy", "picoctf"] {
            std::fs::create_dir_all(root.join(d)).unwrap();
        }
        let mut a = app();
        a.cwd = root.clone();

        // d opens the prompt; a relative path goes there
        a.on_key(key(KeyCode::Char('d')));
        assert_eq!(a.mode, Mode::FolderInput);
        type_line(&mut a, "htb");
        assert_eq!(a.mode, Mode::Browsing);
        assert_eq!(a.cwd, root.join("htb"));
        assert!(a
            .console
            .output
            .iter()
            .any(|l| l.contains("working folder:")));

        // Tab cycles the matches (case-insensitive), Shift+Tab goes back
        a.on_key(key(KeyCode::Char('d')));
        a.on_key(key(KeyCode::Char('l')));
        a.on_key(key(KeyCode::Tab));
        assert_eq!(a.editor.text(), "Lame\\");
        a.on_key(key(KeyCode::Tab));
        assert_eq!(a.editor.text(), "Legacy\\");
        a.on_key(key(KeyCode::Tab));
        assert_eq!(a.editor.text(), "Lame\\", "wraps around");
        a.on_key(KeyEvent::new(KeyCode::BackTab, KeyModifiers::SHIFT));
        assert_eq!(a.editor.text(), "Legacy\\");
        a.on_key(key(KeyCode::Enter));
        assert_eq!(a.cwd, root.join("htb").join("Legacy"));

        // - goes back to the previous folder; .. climbs
        a.on_key(key(KeyCode::Char('d')));
        type_line(&mut a, "-");
        assert_eq!(a.cwd, root.join("htb"));
        a.on_key(key(KeyCode::Char('d')));
        type_line(&mut a, "..");
        assert_eq!(a.cwd, root);

        // a missing folder keeps you where you are, with a clear error
        a.on_key(key(KeyCode::Char('d')));
        type_line(&mut a, "nope");
        assert_eq!(a.cwd, root);
        assert!(
            a.status_line.contains("no such folder"),
            "{}",
            a.status_line
        );

        // Esc cancels without moving
        a.on_key(key(KeyCode::Char('d')));
        a.on_key(key(KeyCode::Char('p')));
        a.on_key(key(KeyCode::Esc));
        assert_eq!(a.mode, Mode::Browsing);
        assert_eq!(a.cwd, root);

        // the folder reaches the web view (profile shown as ~)
        let st: serde_json::Value = serde_json::from_str(&a.web_state_json()).unwrap();
        assert_eq!(st["cwd"], crate::folder::short(&root));
    }

    #[test]
    fn switching_challenge_moves_into_its_folder() {
        let root = std::env::temp_dir()
            .join("winsploit-app-tests")
            .join("challenge-cwd");
        let _ = std::fs::remove_dir_all(&root);
        let mut a = app();
        a.ctf_root = root.clone();
        let ws = Workspace::create(&root, "acme", "web login", "", "").unwrap();
        let dir = ws.dir.clone();
        a.set_challenge(Some(ws));
        assert_eq!(a.cwd, dir);
    }

    #[test]
    fn challenges_are_created_switched_and_used() {
        let root = std::env::temp_dir()
            .join("winsploit-app-tests")
            .join("challenges");
        let _ = std::fs::remove_dir_all(&root);
        let mut a = app();
        a.ctf_root = root.clone();
        let big_c = KeyEvent::new(KeyCode::Char('C'), KeyModifiers::SHIFT);
        a.on_key(big_c);
        assert_eq!(a.challenge_view, Some(0));
        // the wizard: event, name, target, flag format, then a file
        a.on_key(key(KeyCode::Char('n')));
        assert_eq!(a.mode, Mode::Challenge(ChallengeField::Event));
        type_line(&mut a, "pico 2026");
        type_line(&mut a, "rev 1");
        type_line(&mut a, "10.0.0.5");
        type_line(&mut a, "ACME{");
        let ws = a.challenge.clone().expect("created and active");
        assert_eq!(ws.id, "pico-2026/rev-1");
        assert_eq!(ws.info.target, "10.0.0.5");
        assert_eq!(a.mode, Mode::Challenge(ChallengeField::File));
        let sample = root.join("chall.bin");
        std::fs::write(&sample, b"MZ").unwrap();
        type_line(&mut a, &sample.display().to_string());
        assert_eq!(ws.files(), ["chall.bin"]);
        assert_eq!(a.mode, Mode::Browsing);
        // logs and flags follow it; its flag format is trusted
        assert_eq!(a.logs_dir(), ws.logs_dir());
        assert_eq!(a.flags_path(), ws.flags_path());
        a.on_runner_event(AppEvent::Output {
            sid: CONSOLE,
            line: "acme{7}".into(),
        });
        assert_eq!(a.flags.len(), 1);
        assert!(a.flags[0].known);
        // edit the target from the picker
        a.on_key(big_c);
        a.on_key(key(KeyCode::Char('t')));
        a.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        type_line(&mut a, "10.0.0.9");
        assert_eq!(
            Workspace::open(&root, &ws.id).unwrap().info.target,
            "10.0.0.9"
        );
        // a second challenge, then switch back from the web: only real ids
        Workspace::create(&root, "HTB", "Lame", "", "").unwrap();
        a.on_web_action(
            WebAction::Challenge {
                id: "HTB/Lame".into(),
            },
            Caller::Owner,
        );
        assert_eq!(a.challenge.as_ref().unwrap().id, "HTB/Lame");
        for bad in ["../x", "C:/Windows/System32", "HTB/../pico-2026"] {
            a.on_web_action(WebAction::Challenge { id: bad.into() }, Caller::Owner);
            assert_eq!(a.challenge.as_ref().unwrap().id, "HTB/Lame", "{bad}");
        }
        let st = a.web_state_json();
        assert!(
            st.contains("pico-2026/rev-1") && st.contains("\"HTB/Lame\""),
            "{st}"
        );
        // x leaves the challenge
        a.on_key(big_c);
        a.on_key(key(KeyCode::Char('x')));
        assert!(a.challenge.is_none());
    }

    #[test]
    fn recipes_ask_for_what_they_need_then_fill_the_prompt() {
        let root = std::env::temp_dir()
            .join("winsploit-app-tests")
            .join("recipes");
        let _ = std::fs::remove_dir_all(&root);
        let mut a = app();
        a.ctf_root = root.clone();
        let ws = Workspace::create(&root, "pico", "rev 1", "", "").unwrap();
        a.challenge = Some(ws.clone());
        a.selected = a.tools.iter().position(|t| t.name == "nmap").unwrap();
        a.status[a.selected] = ToolStatus::Installed;
        a.cat = 0;
        // Enter opens the picker; the first recipe needs a target
        a.on_key(key(KeyCode::Enter));
        assert_eq!(a.recipe_view, Some(0));
        a.on_key(key(KeyCode::Enter));
        assert_eq!(a.mode, Mode::Recipe(crate::recipe::Slot::Target));
        type_line(&mut a, "10.10.10.3");
        // saved into the challenge, and the prompt is filled
        assert_eq!(
            Workspace::open(&root, &ws.id).unwrap().info.target,
            "10.10.10.3"
        );
        assert_eq!(a.mode, Mode::Input);
        assert!(
            a.editor.text().contains("\"10.10.10.3\""),
            "{}",
            a.editor.text()
        );
        a.on_key(key(KeyCode::Esc));
        // a {file} recipe with a file from outside: copied into the challenge
        let sample = root.join("crack me.bin");
        std::fs::write(&sample, b"MZ").unwrap();
        a.selected = a.tools.iter().position(|t| t.name == "strings").unwrap();
        a.status[a.selected] = ToolStatus::Installed;
        a.on_key(key(KeyCode::Enter));
        a.on_key(key(KeyCode::Enter));
        assert_eq!(a.mode, Mode::Recipe(crate::recipe::Slot::File));
        type_line(&mut a, &sample.display().to_string());
        let copied = ws.files_dir().join("crack me.bin");
        assert!(copied.is_file());
        assert_eq!(a.mode, Mode::Input);
        let argv = crate::args::split(&a.editor.text()).unwrap();
        assert!(argv.contains(&copied.display().to_string()), "{argv:?}");
        a.on_key(key(KeyCode::Esc));
        // the last row is "custom": the plain example args
        a.on_key(key(KeyCode::Enter));
        let n = a.tools[a.selected].recipes.len();
        for _ in 0..n {
            a.on_key(key(KeyCode::Down));
        }
        a.on_key(key(KeyCode::Enter));
        assert_eq!(a.mode, Mode::Input);
        assert_eq!(
            a.editor.text(),
            a.tools[a.selected].args_hint.clone().unwrap_or_default()
        );
        // the browser gets the recipes filled in
        assert!(a.web_state_json().contains("\"recipes\""));
    }

    #[test]
    fn altgr_characters_are_typed_not_dropped() {
        let mut a = app();
        a.mode = Mode::Input;
        a.editor.set("");
        let altgr = KeyModifiers::CONTROL | KeyModifiers::ALT;
        for c in "C:\\ctf\\a{b}[c]@#|~".chars() {
            let m = if c.is_ascii_alphanumeric() || c == ':' {
                KeyModifiers::NONE
            } else {
                altgr
            };
            a.on_key(KeyEvent::new(KeyCode::Char(c), m));
        }
        assert_eq!(a.editor.text(), "C:\\ctf\\a{b}[c]@#|~");
        // plain Ctrl shortcuts still work, and Alt alone still isn't text
        a.on_key(KeyEvent::new(KeyCode::Char('x'), KeyModifiers::ALT));
        assert_eq!(a.editor.text(), "C:\\ctf\\a{b}[c]@#|~");
        a.on_key(KeyEvent::new(KeyCode::Char('u'), KeyModifiers::CONTROL));
        assert_eq!(a.editor.text(), "", "Ctrl+U clears to the start");
    }

    #[test]
    fn flags_in_output_are_caught_once() {
        let mut a = app();
        let out = |line: &str| AppEvent::Output {
            sid: CONSOLE,
            line: line.to_string(),
        };
        a.on_runner_event(out("nothing here {a:1}"));
        assert!(a.flags.is_empty());
        a.on_runner_event(out("Secret: picoCTF{unp4ck3d} and HACK{c0ffee_cup}"));
        assert_eq!(a.flags.len(), 2);
        assert!(a.flags[0].known && !a.flags[1].known);
        assert_eq!(a.flags[0].source, "console");
        assert!(a
            .toasts
            .iter()
            .any(|t| t.text.contains("picoCTF{unp4ck3d}")));
        assert!(a.status_line.contains("F lists"), "{}", a.status_line);
        // the same flag again is not a new one
        a.on_runner_event(out("again: picoCTF{unp4ck3d}"));
        assert_eq!(a.flags.len(), 2);
        // F opens the list, keys move within it, Esc closes it
        a.on_key(KeyEvent::new(KeyCode::Char('F'), KeyModifiers::SHIFT));
        assert_eq!(a.flags_view, Some(0));
        a.on_key(key(KeyCode::Down));
        a.on_key(key(KeyCode::Down));
        assert_eq!(a.flags_view, Some(1), "clamped to the last row");
        a.on_key(key(KeyCode::Esc));
        assert_eq!(a.flags_view, None);
        assert!(a.web_state_json().contains("HACK{c0ffee_cup}"));
    }

    #[test]
    fn toasts_cap_at_three_and_expire() {
        let mut a = app();
        for i in 0..4 {
            a.toast(ToastKind::Info, format!("t{i}"));
        }
        assert_eq!(a.toasts.len(), MAX_TOASTS);
        assert_eq!(a.toasts[0].text, "t1"); // the oldest went first
        let later = a.toasts[2].born + Duration::from_secs(5);
        a.expire_toasts(later);
        assert!(a.toasts.is_empty());
    }

    #[test]
    fn tour_steps_back_and_skips() {
        let mut a = app();
        a.tour = Some(0);
        a.on_key(key(KeyCode::Enter));
        assert_eq!(a.tour, Some(1));
        // the tour eats keys: this doesn't move the list
        let sel = a.selected;
        a.on_key(key(KeyCode::Down));
        assert_eq!(a.selected, sel);
        a.on_key(key(KeyCode::Left));
        assert_eq!(a.tour, Some(0));
        a.on_key(key(KeyCode::Esc));
        assert_eq!(a.tour, None);
        assert!(a.settings.tour_done);
        // walking off the last step finishes it too
        a.tour = Some(TOUR_STEPS - 1);
        a.settings.tour_done = false;
        a.on_key(key(KeyCode::Enter));
        assert!(a.tour.is_none() && a.settings.tour_done);
    }

    fn tab(a: &App, label: &str) -> usize {
        a.cat_tabs.iter().position(|t| t.label() == label).unwrap()
    }

    fn names(a: &App) -> Vec<String> {
        a.visible()
            .iter()
            .map(|&i| a.tools[i].name.clone())
            .collect()
    }

    #[test]
    fn category_tabs_follow_the_taxonomy() {
        let a = app();
        let labels: Vec<String> = a.cat_tabs.iter().map(CatTab::label).collect();
        assert_eq!(&labels[..4], ["All", "Installed", "OSINT", "Recon"]);
        assert_eq!(labels.last().unwrap(), "Wordlists");
    }

    #[test]
    fn a_tool_shows_in_every_category_it_has_with_specialists_first() {
        let mut a = app();
        a.cat = tab(&a, "Recon");
        let recon = names(&a);
        assert!(recon.contains(&"nmap".to_string())); // network first, recon second
                                                      // tools whose main category is recon come before the others
        let first_secondary = recon
            .iter()
            .position(|n| {
                let t = a.tools.iter().find(|t| &t.name == n).unwrap();
                t.primary() != "recon"
            })
            .unwrap();
        assert!(recon[first_secondary..]
            .iter()
            .all(|n| { a.tools.iter().find(|t| &t.name == n).unwrap().primary() != "recon" }));
        a.cat = tab(&a, "Network");
        assert_eq!(names(&a)[0], "nmap");
    }

    #[test]
    fn filter_works_inside_a_tab_and_matches_category_labels() {
        let mut a = app();
        a.cat = tab(&a, "Web");
        a.set_filter("brute".into());
        let n = names(&a);
        assert!(n.contains(&"dirsearch".to_string()), "{n:?}");
        assert!(!n.contains(&"nmap".to_string()));
        a.cat = 0;
        a.set_filter("mobile".into()); // a category label, not in any description
        assert!(names(&a).contains(&"jadx".to_string()));
    }

    #[test]
    fn switching_category_keeps_the_selection_in_view() {
        let mut a = app();
        a.selected = a.tools.iter().position(|t| t.name == "nmap").unwrap();
        a.cat = tab(&a, "Recon") - 1;
        a.step_category(1); // to Recon: nmap is there too
        assert_eq!(a.tools[a.selected].name, "nmap");
        a.step_category(1); // Network: still there
        assert_eq!(a.tools[a.selected].name, "nmap");
        a.cat = tab(&a, "Stego");
        a.keep_selection_visible();
        assert!(a.visible().contains(&a.selected));
        // wraps around both ways
        a.cat = 0;
        a.step_category(-1);
        assert_eq!(a.cat, a.cat_tabs.len() - 1);
    }

    #[test]
    fn slots_fill_then_reuse_the_oldest_finished() {
        let mut a = app();
        assert_eq!(a.free_slot(), Some(0));
        fake(&mut a, 0, "a", true, 0);
        fake(&mut a, 1, "b", false, 9);
        assert_eq!(a.free_slot(), Some(2)); // empty beats finished
        fake(&mut a, 2, "c", false, 3);
        fake(&mut a, 3, "d", true, 0);
        assert_eq!(a.free_slot(), Some(2)); // c finished before b
        fake(&mut a, 1, "b", true, 0);
        fake(&mut a, 2, "c", true, 0);
        assert_eq!(a.free_slot(), None); // all busy: ask
    }

    #[test]
    fn all_busy_asks_which_to_replace() {
        let mut a = app();
        for i in 0..SLOTS {
            fake(&mut a, i, "busy", true, 0);
        }
        let nmap = a.tools.iter().position(|t| t.name == "nmap").unwrap();
        a.run_in_slot(nmap, vec![], Source::Terminal, None);
        assert_eq!(a.mode, Mode::SlotPick);
        a.on_key(key(KeyCode::Esc));
        assert_eq!(a.mode, Mode::Browsing);
        assert!(a.pending.is_none());
        assert!(a.slots.iter().flatten().all(|s| s.title == "busy"));
    }

    #[test]
    fn closing_a_running_terminal_needs_a_second_x() {
        let mut a = app();
        fake(&mut a, 0, "nmap", true, 0);
        fake(&mut a, 2, "ffuf", false, 1);
        a.set_focus(1);
        a.on_key(key(KeyCode::Char('x')));
        assert!(a.slots[0].is_some(), "first x only asks");
        assert_eq!(a.confirm, Some(Confirm::Close(1)));
        a.on_key(key(KeyCode::Char('x')));
        assert!(a.slots[0].is_none());
        assert_eq!(a.focus, 3, "moves to the terminal that's left");
        // a finished one closes at once; then only the console is left
        a.on_key(key(KeyCode::Char('x')));
        assert!(a.slots[2].is_none());
        assert_eq!(a.focus, 0);
        // another key in between cancels the confirmation
        fake(&mut a, 0, "nmap", true, 0);
        a.set_focus(1);
        a.on_key(key(KeyCode::Char('x')));
        a.on_key(key(KeyCode::Down));
        a.on_key(key(KeyCode::Char('x')));
        assert!(a.slots[0].is_some());
    }

    #[test]
    fn quitting_with_tools_running_needs_a_second_q() {
        let mut a = app();
        fake(&mut a, 1, "nmap", true, 0);
        a.on_key(key(KeyCode::Char('q')));
        assert!(!a.quit);
        a.on_key(key(KeyCode::Char('q')));
        assert!(a.quit);
    }

    #[test]
    fn output_goes_to_its_own_terminal_and_marks_unread() {
        let mut a = app();
        fake(&mut a, 0, "one", true, 0);
        fake(&mut a, 1, "two", true, 0);
        let (one, two) = (
            a.slots[0].as_ref().unwrap().id,
            a.slots[1].as_ref().unwrap().id,
        );
        a.set_focus(1);
        a.on_runner_event(AppEvent::Output {
            sid: one,
            line: "a".into(),
        });
        a.on_runner_event(AppEvent::Output {
            sid: two,
            line: "b".into(),
        });
        assert_eq!(a.slots[0].as_ref().unwrap().output, ["a"]);
        assert_eq!(a.slots[1].as_ref().unwrap().output, ["b"]);
        assert!(!a.slots[0].as_ref().unwrap().unread);
        assert!(a.slots[1].as_ref().unwrap().unread);
        a.on_runner_event(AppEvent::Exited {
            sid: two,
            code: Some(0),
        });
        assert!(a.status_line.contains("two finished in terminal 2"));
        a.on_key(key(KeyCode::Char('2')));
        assert!(!a.slots[1].as_ref().unwrap().unread);
        // a closed terminal's late output is dropped, not misrouted
        a.slots[0] = None;
        a.on_runner_event(AppEvent::Output {
            sid: one,
            line: "late".into(),
        });
        assert!(a
            .slots
            .iter()
            .flatten()
            .all(|s| !s.output.contains(&"late".into())));
    }

    #[test]
    fn tab_cycles_through_open_terminals_and_grid_skips_the_console() {
        let mut a = app();
        fake(&mut a, 1, "x", true, 0);
        fake(&mut a, 3, "y", true, 0);
        a.on_key(key(KeyCode::Tab));
        assert_eq!(a.focus, 2);
        a.on_key(key(KeyCode::Tab));
        assert_eq!(a.focus, 4);
        a.on_key(key(KeyCode::Tab));
        assert_eq!(a.focus, 0);
        a.on_key(key(KeyCode::Char('g')));
        assert!(a.grid);
        assert_eq!(a.focus, 2);
        a.on_key(key(KeyCode::BackTab));
        assert_eq!(a.focus, 4, "grid never focuses the console");
        a.on_key(key(KeyCode::Char('0')));
        assert!(!a.grid);
    }

    // ---- the side chat (`T`) --------------------------------------------

    fn tool_idx(a: &App, name: &str) -> usize {
        a.tools.iter().position(|t| t.name == name).unwrap()
    }

    fn ctrl(c: char) -> KeyEvent {
        KeyEvent::new(KeyCode::Char(c), KeyModifiers::CONTROL)
    }

    /// Type `text` into the chat and press Enter (no provider is ever called
    /// under test), then deliver `answer` as the AI's reply.
    fn chat_says(a: &mut App, text: &str, answer: &str) {
        for c in text.chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        a.on_key(key(KeyCode::Enter));
        let req = a.chat.req;
        a.on_runner_event(AppEvent::Chat {
            req,
            result: Ok(answer.to_string()),
        });
    }

    fn answer(tool: &str, args: &str) -> String {
        serde_json::json!({
            "reply": "On it.",
            "run": { "tool": tool, "args": args, "why": "because" }
        })
        .to_string()
    }

    #[test]
    fn t_opens_the_chat_and_it_keeps_the_keyboard_until_esc() {
        let mut a = app();
        assert!(!a.chat.open);
        a.on_key(key(KeyCode::Char('T')));
        assert!(a.chat.open && a.chat.focused);
        assert_eq!(a.chat.msgs.len(), 1, "a short greeting");
        assert_eq!(a.chat.msgs[0].role, ChatRole::Note);

        // dashboard keys are typed text now, not commands
        for c in "qxg".chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        assert_eq!(a.chat.input.text(), "qxg");
        assert!(!a.quit && !a.grid);

        // Esc steps back: clear the line, then give the keyboard back
        a.on_key(key(KeyCode::Esc));
        assert_eq!(a.chat.input.text(), "");
        assert!(a.chat.focused);
        a.on_key(key(KeyCode::Esc));
        assert!(a.chat.open && !a.chat.focused);
        a.on_key(key(KeyCode::Char('g'))); // the dashboard answers again
        assert!(a.grid);
        a.on_key(key(KeyCode::Char('g')));

        // T talks again; Ctrl+T hides and shows
        a.on_key(key(KeyCode::Char('T')));
        assert!(a.chat.focused);
        a.on_key(ctrl('t'));
        assert!(!a.chat.open && !a.chat.focused);
        a.on_key(ctrl('t'));
        assert!(a.chat.open && a.chat.focused);
        assert_eq!(a.chat.msgs.len(), 1, "the transcript survives hiding");
    }

    #[test]
    fn a_message_goes_out_once_and_waits_for_the_answer() {
        let mut a = app();
        a.on_key(key(KeyCode::Char('T')));
        for c in "scan 10.0.0.5".chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        a.on_key(key(KeyCode::Enter));
        let last = a.chat.msgs.last().unwrap();
        assert_eq!(
            (last.role, last.text.as_str()),
            (ChatRole::You, "scan 10.0.0.5")
        );
        assert!(a.chat.busy);
        assert_eq!(a.chat.req, 1);
        assert_eq!(a.chat.input.text(), "");

        // a second message while waiting is held back, and kept in the input
        let n = a.chat.msgs.len();
        for c in "and more".chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        a.on_key(key(KeyCode::Enter));
        assert_eq!(a.chat.msgs.len(), n);
        assert_eq!(a.chat.input.text(), "and more");
        assert!(a.status_line.contains("still waiting"), "{}", a.status_line);
    }

    #[test]
    fn an_installed_tool_becomes_a_plan_that_waits_for_enter() {
        let mut a = app();
        let nmap = tool_idx(&a, "nmap");
        a.status[nmap] = ToolStatus::Installed;
        a.on_key(key(KeyCode::Char('T')));
        // the AI's capitalisation doesn't matter, the catalog's spelling wins
        chat_says(&mut a, "scan it", &answer("NMAP", "-sV 10.0.0.5"));
        assert!(!a.chat.busy);
        let plan = a.chat.pending().expect("a plan is waiting").clone();
        assert_eq!(plan.name, "nmap");
        assert_eq!(plan.tool, nmap);
        assert_eq!(plan.args, "-sV 10.0.0.5");
        assert_eq!(plan.why, "because");
        assert!(a.status_line.contains("Enter runs the plan"));
        // nothing was started
        assert!(a.slots.iter().all(Option::is_none));
        assert!(a.chat.watch.is_none());
    }

    #[test]
    fn plans_that_cannot_run_say_why_and_never_run() {
        let mut a = app();
        let ffuf = tool_idx(&a, "ffuf");
        let nmap = tool_idx(&a, "nmap");
        a.status[ffuf] = ToolStatus::Missing;
        a.status[nmap] = ToolStatus::Installed;
        a.on_key(key(KeyCode::Char('T')));

        let blocked = |a: &App| match &a.chat.msgs.last().unwrap().plan.as_ref().unwrap().state {
            PlanState::Blocked(why) => why.clone(),
            other => panic!("expected a blocked plan, got {other:?}"),
        };
        chat_says(&mut a, "fuzz it", &answer("ffuf", "-u http://x/FUZZ"));
        assert!(blocked(&a).contains("isn't installed"), "{}", blocked(&a));
        assert!(a.chat.pending().is_none(), "Enter has nothing to run");

        a.status[ffuf] = ToolStatus::Conflict;
        chat_says(&mut a, "again", &answer("ffuf", "-u http://x/FUZZ"));
        assert!(blocked(&a).contains("different program"), "{}", blocked(&a));

        // args that don't parse (an open quote) never reach the launcher
        chat_says(&mut a, "scan", &answer("nmap", "-sV \"10.0.0.5"));
        assert!(
            blocked(&a).contains("can't read the args"),
            "{}",
            blocked(&a)
        );

        // a tool that isn't in the catalog is shown, not run
        let before = a.chat.msgs.len();
        chat_says(&mut a, "hack it", &answer("rm -rf", "/"));
        assert!(a.chat.msgs[before..].iter().all(|m| m.plan.is_none()));
        let note = &a.chat.msgs.last().unwrap();
        assert_eq!(note.role, ChatRole::Note);
        assert!(note.text.contains("isn't in your catalog"), "{}", note.text);
        assert!(a.slots.iter().all(Option::is_none), "nothing started");
    }

    #[test]
    fn answers_without_a_run_are_just_replies() {
        let mut a = app();
        a.on_key(key(KeyCode::Char('T')));
        chat_says(
            &mut a,
            "which host?",
            r#"{"reply": "Which host?", "run": null}"#,
        );
        let m = a.chat.msgs.last().unwrap();
        assert_eq!((m.role, m.text.as_str()), (ChatRole::Agent, "Which host?"));
        assert!(m.plan.is_none());
        // a model that ignores the JSON format is still heard
        chat_says(&mut a, "hello", "Just say what to scan.");
        assert_eq!(a.chat.msgs.last().unwrap().text, "Just say what to scan.");
        // a failed request is shown in the chat, and the chat is usable again
        for c in "x".chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        a.on_key(key(KeyCode::Enter));
        let req = a.chat.req;
        a.on_runner_event(AppEvent::Chat {
            req,
            result: Err("401 invalid api key".into()),
        });
        assert!(!a.chat.busy);
        assert!(a
            .chat
            .msgs
            .last()
            .unwrap()
            .text
            .contains("401 invalid api key"));
    }

    #[test]
    fn enter_on_an_empty_line_runs_the_plan_through_the_launcher_checks() {
        let mut a = app();
        let nmap = tool_idx(&a, "nmap");
        a.status[nmap] = ToolStatus::Installed;
        a.on_key(key(KeyCode::Char('T')));
        chat_says(&mut a, "scan", &answer("nmap", "-sV 10.0.0.5"));
        assert!(a.chat.pending().is_some());
        // the tool vanished from PATH since the plan was made: the launcher
        // refuses, which proves the normal checks still stand between the AI
        // and a process (and no process is started under test)
        a.status[nmap] = ToolStatus::Missing;
        a.on_key(key(KeyCode::Enter));
        assert!(a.chat.pending().is_none(), "the plan is used up");
        assert_eq!(
            a.chat.msgs[a.chat.msgs.len() - 2]
                .plan
                .as_ref()
                .unwrap()
                .state,
            PlanState::Ran
        );
        assert!(a.status_line.contains("not installed"), "{}", a.status_line);
        assert!(a.chat.msgs.last().unwrap().text.contains("not installed"));
        assert!(a.slots.iter().all(Option::is_none));
        // Enter again does nothing
        let n = a.chat.msgs.len();
        a.on_key(key(KeyCode::Enter));
        assert_eq!(a.chat.msgs.len(), n);
    }

    #[test]
    fn esc_skips_a_plan_and_a_new_message_makes_it_stale() {
        let mut a = app();
        let nmap = tool_idx(&a, "nmap");
        a.status[nmap] = ToolStatus::Installed;
        a.on_key(key(KeyCode::Char('T')));
        chat_says(&mut a, "scan", &answer("nmap", "-sV x"));
        a.on_key(key(KeyCode::Esc));
        assert!(a.chat.pending().is_none());
        assert!(a.chat.focused, "the first Esc only skipped the plan");
        assert!(a.chat.msgs.iter().any(|m| m
            .plan
            .as_ref()
            .is_some_and(|p| p.state == PlanState::Skipped)));

        chat_says(&mut a, "scan", &answer("nmap", "-sV y"));
        assert!(a.chat.pending().is_some());
        // typing something new instead: the old plan can no longer be run
        for c in "wait".chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        a.on_key(key(KeyCode::Enter));
        assert!(a.chat.pending().is_none());
        assert!(a
            .chat
            .msgs
            .iter()
            .filter_map(|m| m.plan.as_ref())
            .all(|p| p.state != PlanState::Ready));
    }

    #[test]
    fn autorun_still_goes_through_the_launcher_and_never_runs_a_blocked_plan() {
        let mut a = app();
        a.settings.chat_autorun = true;
        let nmap = tool_idx(&a, "nmap");
        let ffuf = tool_idx(&a, "ffuf");
        a.status[nmap] = ToolStatus::Installed;
        a.status[ffuf] = ToolStatus::Missing;
        a.on_key(key(KeyCode::Char('T')));
        // blocked: nothing happens, however autorun is set
        chat_says(&mut a, "fuzz", &answer("ffuf", "-u x"));
        assert!(a
            .chat
            .msgs
            .last()
            .unwrap()
            .plan
            .as_ref()
            .is_some_and(|p| matches!(p.state, PlanState::Blocked(_))));
        assert!(a.slots.iter().all(Option::is_none));
        // runnable: it goes straight to the launcher. All four terminals are
        // busy here, so the launcher asks which to replace and starts nothing
        for i in 0..SLOTS {
            fake(&mut a, i, "busy", true, 0);
        }
        chat_says(&mut a, "scan", &answer("nmap", "-sV 10.0.0.5"));
        assert_eq!(a.mode, Mode::SlotPick);
        assert!(!a.chat.focused, "the slot question needs the keyboard");
        assert!(a.chat.msgs.last().unwrap().text.contains("busy"));
        // answering it hands the keys back to the dashboard (Esc cancels)
        a.on_key(key(KeyCode::Esc));
        assert_eq!(a.mode, Mode::Browsing);
    }

    #[test]
    fn the_chat_says_when_a_run_it_started_ends() {
        let mut a = app();
        fake(&mut a, 0, "nmap", true, 0);
        let sid = a.slots[0].as_ref().unwrap().id;
        a.chat.watch = Some((sid, "nmap".into()));
        a.on_runner_event(AppEvent::Exited { sid, code: Some(2) });
        let m = a.chat.msgs.last().unwrap();
        assert_eq!(m.role, ChatRole::Note);
        assert!(m.text.contains("nmap exited with code 2"), "{}", m.text);
        assert!(a.chat.watch.is_none());
        // a run it didn't start stays out of the chat
        fake(&mut a, 1, "ffuf", true, 0);
        let other = a.slots[1].as_ref().unwrap().id;
        let n = a.chat.msgs.len();
        a.on_runner_event(AppEvent::Exited {
            sid: other,
            code: Some(0),
        });
        assert_eq!(a.chat.msgs.len(), n);
    }

    #[test]
    fn stale_answers_are_dropped_and_clear_starts_over() {
        let mut a = app();
        a.on_key(key(KeyCode::Char('T')));
        for c in "scan".chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        a.on_key(key(KeyCode::Enter));
        let old = a.chat.req;
        a.on_key(key(KeyCode::Esc)); // stop waiting
        assert!(!a.chat.busy);
        let n = a.chat.msgs.len();
        a.on_runner_event(AppEvent::Chat {
            req: old,
            result: Ok(answer("nmap", "-sV x")),
        });
        assert_eq!(a.chat.msgs.len(), n, "the late answer is ignored");

        // /clear works even while an answer is on its way: it empties the
        // transcript and the late answer is dropped
        for c in "hi".chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        a.on_key(key(KeyCode::Enter));
        assert!(a.chat.busy);
        let waiting = a.chat.req;
        for c in "/clear".chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        a.on_key(key(KeyCode::Enter));
        assert!(a.chat.msgs.is_empty() && !a.chat.busy);
        assert_ne!(a.chat.req, waiting);
        a.on_runner_event(AppEvent::Chat {
            req: waiting,
            result: Ok(answer("nmap", "-sV x")),
        });
        assert!(
            a.chat.msgs.is_empty(),
            "the answer to a cleared chat is dropped"
        );

        // /close hides the panel
        for c in "/close".chars() {
            a.on_key(key(KeyCode::Char(c)));
        }
        a.on_key(key(KeyCode::Enter));
        assert!(!a.chat.open && !a.chat.focused);
    }

    #[test]
    fn what_the_ai_is_told_is_the_session_with_secrets_masked() {
        let mut a = app();
        let nmap = tool_idx(&a, "nmap");
        a.status[nmap] = ToolStatus::Installed;
        a.console.push("$ nmap -sV 10.0.0.5");
        a.console
            .push("login with password=hunter2 on https://u:pw@host/");
        a.console.push("22/tcp open ssh");
        let ctx = a.chat_context();
        assert!(
            ctx.tools.iter().all(|t| t.name != "seclists-lite"),
            "datasets are left out"
        );
        let t = ctx.tools.iter().find(|t| t.name == "nmap").unwrap();
        assert_eq!(t.status, "installed");
        assert!(!t.what.is_empty() && !t.flags.is_empty());
        assert_eq!(ctx.ran.as_deref(), Some("nmap -sV 10.0.0.5"));
        assert!(ctx.tail.contains("22/tcp open ssh"));
        assert!(
            !ctx.tail.contains("hunter2") && !ctx.tail.contains(":pw@"),
            "{}",
            ctx.tail
        );
        // the user can switch redaction off
        a.settings.redact = crate::redact::Level::Off;
        assert!(a.chat_context().tail.contains("hunter2"));
        // the challenge's target is part of the context
        a.challenge = Some(crate::workspace::Workspace {
            dir: PathBuf::from("x"),
            id: "pico/rev".into(),
            info: crate::workspace::Info {
                name: "rev".into(),
                target: "10.10.10.3".into(),
                ..Default::default()
            },
        });
        assert_eq!(
            a.chat_context().challenge,
            Some(("rev".into(), "10.10.10.3".into()))
        );
    }

    /// Talks to the real AI provider: run by hand with
    /// `cargo test live_chat -- --ignored --nocapture`. Checks that a real
    /// model, given the real prompt, answers in the format the chat expects
    /// and picks a tool that exists in the catalog.
    #[test]
    #[ignore = "calls the real AI provider (uses your login); run by hand"]
    fn live_chat_round_trip() {
        let mut a = app();
        for name in ["nmap", "ffuf", "gobuster"] {
            let i = tool_idx(&a, name);
            a.status[i] = ToolStatus::Installed;
        }
        let provider = a.settings.agent;
        assert!(
            App::provider_problem(provider).is_none(),
            "{provider:?} isn't ready"
        );
        let ctx = a.chat_context();
        let prompt = chat::build_prompt(
            &ctx,
            &[],
            "scan 10.0.0.5 for open ports and tell me which services run on them",
        );
        println!("prompt: {} chars", prompt.chars().count());
        let (tx, rx) = std::sync::mpsc::channel();
        agent::chat(provider, a.effective_model(provider), prompt, 7, tx);
        let ev = rx
            .recv_timeout(std::time::Duration::from_secs(240))
            .expect("no answer in 4 minutes");
        let AppEvent::Chat { req, result } = ev else {
            panic!("expected a chat event");
        };
        assert_eq!(req, 7);
        let text = result.expect("the request failed");
        println!("raw answer: {text}");
        let reply = chat::parse_reply(&text);
        println!("reply: {reply:?}");
        let run = reply.run.expect("the model should have chosen a tool");
        let idx = a
            .tools
            .iter()
            .position(|t| t.name.eq_ignore_ascii_case(&run.tool))
            .unwrap_or_else(|| panic!("{:?} isn't in the catalog", run.tool));
        println!("tool: {} args: {}", a.tools[idx].name, run.args);
        assert!(
            args::split(&run.args).is_ok(),
            "args don't parse: {:?}",
            run.args
        );
        assert!(
            run.args.contains("10.0.0.5"),
            "the target is missing: {:?}",
            run.args
        );
    }

    #[test]
    fn the_chat_is_not_reachable_from_the_web_view() {
        // terminal only, on purpose: no web action opens it or sends to it
        let mut a = app();
        let (cmd_tx, _) = std::sync::mpsc::channel::<()>();
        drop(cmd_tx);
        assert!(!a.chat.open);
        for text in [r#"{"type":"chat","text":"scan"}"#, r#"{"type":"talk"}"#] {
            assert!(
                serde_json::from_str::<crate::web::WebAction>(text).is_err(),
                "{text} must not parse as a web action"
            );
        }
        a.on_key(key(KeyCode::Char('T')));
        assert!(a.chat.open);
    }

    // ------------------------------------------------------------ web view

    /// Parity: every action can be done from the terminal (a key in the `?`
    /// overlay) and from the web view (a data-action control).
    #[test]
    fn every_action_has_a_key_and_a_web_control() {
        let every = [
            Action::Run {
                tool: 0,
                args: String::new(),
                slot: None,
            },
            Action::Install(0),
            Action::Cancel(1),
            Action::Stdin(1, String::new()),
            Action::Eof(1),
            Action::Close(1),
            Action::Redetect,
            Action::HashId(String::new()),
            Action::Challenge(String::new()),
            Action::Admit(1, Role::Watch),
            Action::Remove(1),
            Action::NewKey,
            Action::Copy(1),
        ];
        assert_eq!(every.len(), Action::BINDINGS.len());
        for a in &every {
            let kind = a.kind();
            let (_, key) = Action::BINDINGS
                .iter()
                .find(|(k, _)| *k == kind)
                .unwrap_or_else(|| panic!("{kind}: no terminal key in Action::BINDINGS"));
            assert!(
                ui::help_rows()
                    .any(|(k, d)| k.split_whitespace().any(|w| w == *key) || d.contains(key)),
                "{kind}: key {key:?} isn't listed in the ? overlay"
            );
            assert!(
                web::INDEX_HTML.contains(&format!("data-action=\"{kind}\""))
                    || web::APP_JS.contains(&format!("data-action=\"{kind}\"")),
                "{kind}: no data-action control in the web page"
            );
        }
    }

    #[test]
    fn web_actions_go_through_the_same_path() {
        let mut a = app();
        a.on_web_action(
            WebAction::Hashid {
                hash: "5f4dcc3b5aa765d61d8327deb882cf99".into(),
            },
            Caller::Owner,
        );
        assert!(a.console.output.iter().any(|l| l.contains("MD5")));
        // unknown tools are reported, nothing runs
        a.on_web_action(
            WebAction::Run {
                tool: "no-such-tool".into(),
                args: String::new(),
                slot: None,
            },
            Caller::Owner,
        );
        assert!(a.console.output.last().unwrap().contains("unknown tool"));
        // a tool that isn't detected yet isn't launched, but the args land
        // in the shared history
        let name = a.tools[0].name.clone();
        a.on_web_action(
            WebAction::Run {
                tool: name.clone(),
                args: "-h".into(),
                slot: None,
            },
            Caller::Owner,
        );
        assert!(a.slots.iter().all(Option::is_none));
        assert!(a.status_line.contains("still detecting"));
        assert_eq!(a.history.list(&name), ["-h"]);
        // closing from the web closes even a running terminal (the page
        // asks first)
        fake(&mut a, 1, "nmap", true, 0);
        a.on_web_action(WebAction::Close { term: 2 }, Caller::Owner);
        assert!(a.slots[1].is_none());
    }

    #[test]
    fn all_busy_from_the_web_asks_the_page_not_the_terminal() {
        let mut a = app();
        for i in 0..SLOTS {
            fake(&mut a, i, "busy", true, 0);
        }
        let nmap = a.tools.iter().position(|t| t.name == "nmap").unwrap();
        a.run_in_slot(nmap, vec![], Source::Web(Caller::Owner), None);
        assert_eq!(a.mode, Mode::Browsing, "the terminal's picker stays closed");
        assert!(a.toasts.last().unwrap().text.contains("busy"));
    }

    #[test]
    fn start_menu_then_the_tour() {
        let mut a = app();
        assert_eq!(a.start_menu, Some(0));
        a.on_key_splash(KeyCode::Down);
        assert_eq!(a.start_menu, Some(1));
        a.on_key_splash(KeyCode::Down);
        assert_eq!(a.start_menu, Some(0)); // wraps
        a.on_key_splash(KeyCode::Char('x')); // other keys do nothing
        assert!(a.screen == Screen::Splash);
        a.on_key_splash(KeyCode::Enter);
        assert!(a.screen == Screen::Main && a.web.is_none());
        assert_eq!(a.tour, Some(0), "first launch: the tour follows");

        let mut b = app();
        b.set_start(StartMode::Terminal);
        b.settings.tour_done = true;
        assert_eq!(b.start_menu, None);
        b.on_key_splash(KeyCode::Char('x')); // any key enters
        assert!(b.screen == Screen::Main && b.tour.is_none());
    }

    #[test]
    fn exit_in_the_shown_terminal_replaces_the_running_status() {
        let mut a = app();
        fake(&mut a, 0, "nmap", true, 0);
        a.set_focus(1);
        a.status_line = "running nmap in terminal 1 (pid 1)".into();
        let sid = a.slots[0].as_ref().unwrap().id;
        a.on_runner_event(AppEvent::Exited { sid, code: Some(0) });
        assert!(a.status_line.contains("nmap finished in terminal 1"));
    }

    /// An app serving the LAN (on a free port, certificate in memory).
    fn lan_app() -> App {
        let mut a = app();
        a.screen = Screen::Main;
        a.web_lan = true;
        a.web_port = Some(0);
        a.start_web(false);
        assert!(
            a.web.as_ref().is_some_and(web::Server::lan),
            "LAN view didn't start"
        );
        a
    }

    /// Run one frame's worth of LAN bookkeeping.
    fn frame(a: &mut App) {
        a.sync_web();
        a.refresh_join_prompt(Instant::now());
    }

    fn past_grace(a: &mut App) {
        if let Some(p) = a.join_prompt.as_mut() {
            p.since = Instant::now() - JOIN_GRACE - Duration::from_millis(1);
        }
    }

    #[test]
    fn lan_mode_shows_the_key_and_asks_before_letting_anyone_in() {
        let mut a = lan_app();
        // W opens the L panel with the key; nothing secret goes to the console
        assert_eq!(a.lan_panel, Some(0));
        let jk = a.lan_key.clone().expect("a join key");
        assert!(a
            .console
            .output
            .iter()
            .any(|l| l.contains("certificate SHA-256")));
        assert!(!a.console.output.iter().any(|l| l.contains(&jk.code)));
        assert!(!a
            .console
            .output
            .iter()
            .any(|l| l.contains("?t=") && l.contains("network")));

        let id = a.web.as_ref().unwrap().add_test_guest("Taha");
        a.on_runner_event(AppEvent::Lan(LanEvent::JoinRequest(id)));
        assert!(a
            .console
            .output
            .last()
            .unwrap()
            .contains("\"Taha\" (192.168.1.23)"));
        // no prompt over the open panel, nor while typing
        frame(&mut a);
        assert!(a.join_prompt.is_none());
        a.on_key(key(KeyCode::Esc));
        a.mode = Mode::Filter;
        frame(&mut a);
        assert!(a.join_prompt.is_none());
        a.mode = Mode::Browsing;
        frame(&mut a);
        assert_eq!(a.join_prompt.as_ref().map(|p| p.guest.id), Some(id));
        // keys right after it appears are ignored (a fast j j Enter)
        a.on_key(key(KeyCode::Char('2')));
        assert!(a.join_prompt.is_some());
        past_grace(&mut a);
        a.on_key(key(KeyCode::Char('2')));
        assert!(a.join_prompt.is_none());
        let g = &a.web.as_ref().unwrap().guests()[0];
        assert_eq!(g.status, GuestStatus::In(Role::Control));
        assert!(a
            .console
            .output
            .last()
            .unwrap()
            .contains("is in (full control)"));

        // a run by that guest says who started it
        assert_eq!(
            a.source_label(Source::Web(Caller::Guest(id))),
            "web: \"Taha\" (192.168.1.23)"
        );
        assert_eq!(
            a.started_by(Source::Web(Caller::Guest(id))).unwrap(),
            "by \"Taha\" (192.168.1.23) from the LAN"
        );
        // ...but a guest can't manage guests, even if a request got through
        a.on_web_action(WebAction::Remove { guest: id }, Caller::Guest(id));
        a.on_web_action(WebAction::Newkey, Caller::Guest(id));
        assert_eq!(a.web.as_ref().unwrap().guests().len(), 1);
        assert_eq!(a.web.as_ref().unwrap().key().unwrap().code, jk.code);
        // the owner's browser can
        a.on_web_action(
            WebAction::Admit {
                guest: id,
                role: Role::Watch,
            },
            Caller::Owner,
        );
        assert_eq!(
            a.web.as_ref().unwrap().guests()[0].status,
            GuestStatus::In(Role::Watch)
        );
    }

    #[test]
    fn join_requests_can_wait_and_the_l_panel_decides() {
        let mut a = lan_app();
        a.lan_panel = None;
        let w = a.web.as_ref().unwrap();
        let first = w.add_test_guest("Sam");
        let second = w.add_test_guest("");
        frame(&mut a);
        assert_eq!(a.join_prompt.as_ref().unwrap().guest.id, first);
        // Esc: decide later; the next one comes up, the first doesn't again
        past_grace(&mut a);
        a.on_key(key(KeyCode::Esc));
        frame(&mut a);
        assert_eq!(a.join_prompt.as_ref().unwrap().guest.id, second);
        past_grace(&mut a);
        a.on_key(key(KeyCode::Down));
        a.on_key(key(KeyCode::Down));
        a.on_key(key(KeyCode::Enter)); // Deny
        frame(&mut a);
        assert!(a.join_prompt.is_none());
        assert_eq!(a.lan_guests.len(), 1);
        assert!(a.console.output.last().unwrap().contains("turned away"));

        // L: the waiting one is there; v lets them in, x removes them
        a.on_key(key(KeyCode::Char('L')));
        assert_eq!(a.lan_panel, Some(0));
        a.on_key(key(KeyCode::Char('v')));
        frame(&mut a);
        assert_eq!(a.lan_guests[0].status, GuestStatus::In(Role::Watch));
        a.on_key(key(KeyCode::Char('x')));
        frame(&mut a);
        assert!(a.lan_guests.is_empty());
        // n: a new key
        let old = a.lan_key.clone().unwrap().code;
        a.on_key(key(KeyCode::Char('n')));
        assert_ne!(a.lan_key.clone().unwrap().code, old);
        // w stops it all
        a.on_key(key(KeyCode::Esc));
        a.on_key(key(KeyCode::Char('w')));
        assert!(a.web.is_none() && a.lan_key.is_none() && a.lan_panel.is_none());
        // L without LAN mode explains
        a.on_key(key(KeyCode::Char('L')));
        assert!(a.lan_panel.is_none() && a.status_line.contains("LAN mode is off"));
    }

    #[test]
    fn redetect_during_a_pass_runs_again_after_it() {
        let mut a = app();
        a.detecting = true;
        a.apply(Action::Redetect, Source::Web(Caller::Owner));
        assert!(a.redetect_queued);
        // the stale pass's result is dropped and a fresh pass starts
        a.on_detected(vec![detect::Found::Missing; a.tools.len()]);
        assert!(a.detecting && !a.redetect_queued);
        assert!(a.status.iter().all(|s| *s == ToolStatus::Unknown));
    }

    #[test]
    fn web_state_lists_terminals_and_hello_the_tabs() {
        let mut a = app();
        fake(&mut a, 2, "ffuf", true, 0);
        let s: serde_json::Value = serde_json::from_str(&a.web_state_json()).unwrap();
        assert_eq!(s["status"].as_array().unwrap().len(), a.tools.len());
        let terms = s["terminals"].as_array().unwrap();
        assert_eq!(terms.len(), SLOTS + 1);
        assert_eq!(terms[0]["title"], "console");
        assert!(terms[1].is_null());
        assert_eq!(terms[3]["title"], "ffuf");
        assert_eq!(terms[3]["state"], "running");
        let h: serde_json::Value = serde_json::from_str(&a.web_hello()).unwrap();
        assert_eq!(h["tabs"][0]["label"], "All");
        assert_eq!(h["tabs"][1]["key"], "installed");
        assert_eq!(h["tools"].as_array().unwrap().len(), a.tools.len());
    }

    #[test]
    fn first_run_asks_and_a_no_installs_nothing_but_deletes_the_file() {
        let cat = crate::registry::parse(include_str!("../tools.yaml")).unwrap();
        let dir =
            std::env::temp_dir().join(format!("winsploit-test-{}-firstrun", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        let f = dir.join("first-run.yaml");
        std::fs::write(
            &f,
            "packs: [ctf]
",
        )
        .unwrap();
        let mut a = app();
        a.first_run = crate::firstrun::Pending::load_from(&f, &cat);
        assert!(a.first_run.is_some() && f.exists());
        // unrelated keys neither answer nor leak through to the dashboard
        a.on_key(key(KeyCode::Char('i')));
        assert!(a.first_run.is_some() && !a.installing && f.exists());
        a.on_key(key(KeyCode::Char('n')));
        assert!(a.first_run.is_none() && !f.exists());
        assert!(a.install_queue.is_empty() && !a.installing);
        let _ = std::fs::remove_dir_all(dir);
    }

    // ---------------------------------------------------------------- mouse

    type TestScreen = ratatui::Terminal<ratatui::backend::TestBackend>;

    /// Draw `a` on a `w` x `h` test screen, which records its hit rects.
    fn paint(a: &App, w: u16, h: u16) -> TestScreen {
        let mut t = ratatui::Terminal::new(ratatui::backend::TestBackend::new(w, h)).unwrap();
        t.draw(|f| ui::render(f, a)).unwrap();
        t
    }

    /// The cell where `text` first starts in screen rows `rows`.
    fn spot(t: &TestScreen, text: &str, rows: std::ops::Range<u16>) -> (u16, u16) {
        let buf = t.backend().buffer();
        for y in rows {
            let cells: Vec<&str> = (0..buf.area.width).map(|x| buf[(x, y)].symbol()).collect();
            let line = cells.concat();
            if let Some(at) = line.find(text) {
                // byte offset -> column
                let mut bytes = 0;
                for (x, c) in cells.iter().enumerate() {
                    if bytes == at {
                        return (x as u16, y);
                    }
                    bytes += c.len();
                }
            }
        }
        panic!("{text:?} isn't on screen");
    }

    fn mouse(a: &mut App, kind: MouseEventKind, (x, y): (u16, u16)) {
        a.on_mouse(MouseEvent {
            kind,
            column: x,
            row: y,
            modifiers: KeyModifiers::NONE,
        });
    }

    fn click(a: &mut App, at: (u16, u16)) {
        mouse(a, MouseEventKind::Down(MouseButton::Left), at);
    }

    #[test]
    fn mouse_selects_runs_and_scrolls() {
        let mut a = app();
        let nmap = a.tools.iter().position(|t| t.name == "nmap").unwrap();
        let t = paint(&a, 140, 40);
        let at = spot(&t, "nmap ", 3..30);
        assert!(at.0 < 50, "in the tool list: {at:?}");
        click(&mut a, at);
        assert_eq!(a.selected, nmap);
        assert!(a.recipe_view.is_none(), "one click only selects");
        // a double-click is Enter: nmap has recipes
        click(&mut a, at);
        assert_eq!(a.recipe_view, Some(0));
        a.recipe_view = None;
        // the wheel over the list moves the selection
        paint(&a, 140, 40);
        mouse(&mut a, MouseEventKind::ScrollDown, at);
        assert_ne!(a.selected, nmap);
        // over a terminal it scrolls that terminal
        for i in 0..200 {
            a.console.push(format!("line {i}"));
        }
        paint(&a, 140, 40);
        mouse(&mut a, MouseEventKind::ScrollUp, (100, 20));
        assert_eq!(a.console.scroll, 3);
        mouse(&mut a, MouseEventKind::ScrollDown, (100, 20));
        assert_eq!(a.console.scroll, 0);
    }

    #[test]
    fn mouse_picks_categories_and_terminals() {
        let mut a = app();
        let t = paint(&a, 200, 40);
        click(&mut a, spot(&t, "Web", 1..2));
        assert_eq!(a.cat_tabs[a.cat].label(), "Web");
        assert!(a.visible().contains(&a.selected));
        // terminal tabs
        fake(&mut a, 2, "ffuf", false, 1);
        let t = paint(&a, 200, 40);
        click(&mut a, spot(&t, "3 ffuf", 2..3));
        assert_eq!(a.focus, 3);
        click(&mut a, spot(&t, "0 console", 2..3));
        assert_eq!(a.focus, 0);
        // a grid cell
        fake(&mut a, 0, "nmap", false, 2);
        a.set_focus(3);
        a.grid = true;
        let t = paint(&a, 200, 40);
        click(&mut a, spot(&t, "1 nmap", 3..40));
        assert_eq!(a.focus, 1);
        assert!(a.grid);
    }

    #[test]
    fn mouse_presses_footer_hints_and_closes_overlays() {
        let mut a = app();
        let t = paint(&a, 160, 40);
        click(&mut a, spot(&t, "theme", 39..40));
        assert!(a.appearance.is_some());
        // a theme in the list is picked live
        let before = a.settings.theme;
        let cyan = crate::theme::ThemeName::Cyan;
        let t = paint(&a, 160, 40);
        click(&mut a, spot(&t, cyan.label(), 0..40));
        assert_eq!(a.settings.theme, cyan);
        // a click outside closes it, as Esc does (the old look comes back)
        click(&mut a, (0, 0));
        assert!(a.appearance.is_none());
        assert_eq!(a.settings.theme, before);
        // ? opens the keys, any click closes them
        let t = paint(&a, 160, 40);
        click(&mut a, spot(&t, "all keys", 39..40));
        assert!(a.show_help);
        paint(&a, 160, 40);
        click(&mut a, (80, 20));
        assert!(!a.show_help);
    }

    #[test]
    fn mouse_picks_overlay_entries() {
        let mut a = app();
        a.selected = a.tools.iter().position(|t| t.name == "nmap").unwrap();
        a.recipe_view = Some(0);
        let custom = a.tools[a.selected].recipes.len();
        let t = paint(&a, 160, 40);
        let at = spot(&t, "custom", 0..40);
        click(&mut a, at);
        assert_eq!(a.recipe_view, Some(custom), "the first click highlights");
        click(&mut a, at);
        assert!(a.recipe_view.is_none());
        assert_eq!(a.mode, Mode::Input, "the second one is Enter");
        a.mode = Mode::Browsing;
        // clicks inside the box but off an entry keep it open
        a.flags_view = Some(0);
        let t = paint(&a, 160, 40);
        click(&mut a, spot(&t, "no flags yet", 0..40));
        assert_eq!(a.flags_view, Some(0));
        click(&mut a, (0, 0));
        assert!(a.flags_view.is_none());
    }

    #[test]
    fn mouse_confirms_a_destructive_close() {
        let mut a = app();
        fake(&mut a, 0, "nmap", true, 0);
        a.set_focus(1);
        // the pane's own "x close" hint is the x key: it asks first
        let t = paint(&a, 160, 40);
        let x = spot(&t, "x close", 30..39);
        click(&mut a, x);
        assert_eq!(a.confirm, Some(Confirm::Close(1)));
        assert!(a.slots[0].is_some());
        // any other click cancels it
        paint(&a, 160, 40);
        click(&mut a, (100, 20));
        assert!(a.confirm.is_none());
        // the footer button confirms it
        a.press(KeyCode::Char('x'));
        let t = paint(&a, 160, 40);
        click(&mut a, spot(&t, "yes, close", 39..40));
        assert!(a.slots[0].is_none());
        assert!(a.confirm.is_none());
    }

    // ----------------------------------------------------------------- copy

    #[test]
    fn y_copies_the_shown_terminal() {
        let mut a = app();
        fake(&mut a, 1, "nmap", false, 1);
        for l in ["22/tcp open ssh", "25/tcp closed smtp", "80/tcp open http"] {
            a.slots[1].as_mut().unwrap().push(l);
        }
        a.set_focus(2);
        a.press(KeyCode::Char('y'));
        assert_eq!(a.status_line, "copied 3 lines from terminal 2 (nmap)");
        assert!(a.toasts.iter().any(|t| t.text == a.status_line));
        let (text, n, only) = a.output_to_copy(2).unwrap();
        assert_eq!((n, only), (3, None));
        assert_eq!(
            text,
            "22/tcp open ssh\r\n25/tcp closed smtp\r\n80/tcp open http"
        );
        // with a find active there, only the matching lines
        a.search = "OPEN".into();
        a.find_next(true);
        a.press(KeyCode::Char('y'));
        assert_eq!(
            a.status_line,
            "copied 2 lines matching 'OPEN' from terminal 2 (nmap)"
        );
        // the console, and an empty terminal
        a.console.output.clear();
        a.console.push(":: hi");
        a.apply(Action::Copy(0), Source::Terminal);
        assert_eq!(a.status_line, "copied 1 line from the console");
        a.apply(Action::Copy(4), Source::Terminal);
        assert_eq!(a.status_line, "nothing to copy in this terminal");
        // a browser never writes this machine's clipboard
        a.status_line.clear();
        a.apply(Action::Copy(2), Source::Web(Caller::Owner));
        assert!(a.status_line.is_empty());
    }

    #[test]
    fn the_title_bar_copy_button_copies() {
        let mut a = app();
        fake(&mut a, 0, "ffuf", false, 1);
        a.slots[0].as_mut().unwrap().push("admin [Status: 200]");
        a.set_focus(1);
        let t = paint(&a, 160, 40);
        click(&mut a, spot(&t, "y copy", 2..4));
        assert_eq!(a.status_line, "copied 1 line from terminal 1 (ffuf)");
        // in the grid each pane has its own
        fake(&mut a, 3, "nmap", false, 2);
        a.slots[3].as_mut().unwrap().push("22/tcp open");
        a.grid = true;
        let t = paint(&a, 160, 40);
        let (_, y) = spot(&t, "4 nmap", 20..40);
        click(&mut a, spot(&t, "y copy", y..y + 1));
        assert_eq!(a.status_line, "copied 1 line from terminal 4 (nmap)");
    }
}
