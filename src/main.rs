//! winsploit — native-Windows security toolbox & command runner.
//!
//! Usage:
//!   winsploit [--catalog <path>]         launch the interactive dashboard (TUI)
//!       [--web | --no-web] [--port <n>]  open it in the browser too / skip the question
//!       [-C <folder>]                    work in <folder> (tools run there; `d` changes it)
//!   winsploit [--catalog <path>] list    print the tool catalog and exit
//!   winsploit hashid <hash>...           identify hash types
//!   winsploit --help                     show this help
//!   winsploit --version                  show version

mod agent;
mod app;
mod args;
mod banner;
mod detect;
mod editor;
mod envpath;
mod firewall;
mod firstrun;
mod flags;
mod folder;
mod hashid;
mod install;
mod project;
mod recipe;
mod registry;
mod runlog;
mod runner;
mod sandbox;
mod scoop;
mod scope;
mod session;
mod setup;
mod shortcut;
mod theme;
mod tls;
mod ui;
mod update;
mod web;
mod workspace;

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use app::App;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    // --catalog <path> / --catalog=<path>, --web, --no-web, --lan,
    // --port <n> and -C / --dir <folder> may appear anywhere.
    let mut catalog: Option<PathBuf> = None;
    let mut dir: Option<PathBuf> = None;
    let mut web = WebOpts {
        start: app::StartMode::Ask,
        port: None,
        lan: false,
    };
    let mut i = 0;
    while i < args.len() {
        if args[i] == "--lan" {
            args.remove(i);
            web.lan = true;
        } else if args[i] == "--web" || args[i] == "--no-web" {
            web.start = if args.remove(i) == "--web" {
                app::StartMode::Web
            } else {
                app::StartMode::Terminal
            };
        } else if args[i] == "--port" || args[i].starts_with("--port=") {
            let v = match args.remove(i).strip_prefix("--port=") {
                Some(v) => v.to_string(),
                None if i < args.len() => args.remove(i),
                None => String::new(),
            };
            match v.parse::<u16>() {
                Ok(p) if p > 0 => web.port = Some(p),
                _ => {
                    eprintln!("winsploit: --port needs a port number (1-65535)");
                    std::process::exit(2);
                }
            }
        } else if args[i] == "-C" || args[i] == "--dir" || args[i].starts_with("--dir=") {
            let v = match args.remove(i).strip_prefix("--dir=") {
                Some(v) => v.to_string(),
                None if i < args.len() => args.remove(i),
                None => String::new(),
            };
            if v.trim().is_empty() {
                eprintln!("winsploit: -C needs a folder");
                std::process::exit(2);
            }
            dir = Some(PathBuf::from(v));
        } else if let Some(v) = args[i].strip_prefix("--catalog=") {
            catalog = Some(PathBuf::from(v));
            args.remove(i);
        } else if args[i] == "--catalog" {
            args.remove(i);
            if i >= args.len() {
                eprintln!("winsploit: --catalog needs a path");
                std::process::exit(2);
            }
            catalog = Some(PathBuf::from(args.remove(i)));
        } else {
            i += 1;
        }
    }

    // -C: work in that folder, as if winsploit had been started there (tools
    // run in the working folder; `d` changes it later)
    if let Some(d) = &dir {
        match folder::initial(Some(d)) {
            Ok(path) => {
                if let Err(e) = std::env::set_current_dir(&path) {
                    eprintln!("winsploit: -C {}: {e}", path.display());
                    std::process::exit(2);
                }
            }
            Err(e) => {
                eprintln!("winsploit: {e}");
                std::process::exit(2);
            }
        }
    }

    match args.first().map(String::as_str) {
        Some("--help") | Some("-h") | Some("help") => {
            print_help();
            Ok(())
        }
        Some("--version") | Some("-V") => {
            println!("winsploit {VERSION}");
            Ok(())
        }
        Some("list") => list(catalog.as_deref()),
        Some("setup") => {
            let flags = &args[1..];
            if let Some(bad) = flags
                .iter()
                .find(|a| !["--buckets", "--pipx", "--path", "--shortcut"].contains(&a.as_str()))
            {
                eprintln!("winsploit: setup: unknown option '{bad}'");
                std::process::exit(2);
            }
            let buckets = flags.iter().any(|a| a == "--buckets");
            let pipx = flags.iter().any(|a| a == "--pipx");
            let path = flags.iter().any(|a| a == "--path");
            let shortcut = flags.iter().any(|a| a == "--shortcut");
            if path {
                add_self_to_path();
            }
            if shortcut {
                add_shortcuts();
            }
            if (path || shortcut) && !buckets && !pipx {
                return Ok(());
            }
            let Loaded { catalog, .. } = load_catalog(catalog.as_deref())?;
            if let Err(e) = setup::run(&catalog, buckets, pipx) {
                eprintln!("winsploit: setup: {e:#}");
                std::process::exit(1);
            }
            Ok(())
        }
        // Hidden: the installer's build step turns this into packs.iss.
        Some("packs") => {
            let Loaded { catalog, .. } = load_catalog(catalog.as_deref())?;
            if args.get(1).map(String::as_str) == Some("--iss") {
                print!("{}", firstrun::iss_include(&catalog));
            } else {
                for p in firstrun::pack_list(&catalog) {
                    println!("{:<10} {:<14} {}", p.id, p.label, p.count);
                }
            }
            Ok(())
        }
        Some("hashid") => {
            if args.len() < 2 {
                eprintln!("usage: winsploit hashid <hash> [<hash> ...]");
                std::process::exit(2);
            }
            for (i, h) in args[1..].iter().enumerate() {
                if args.len() > 2 {
                    if i > 0 {
                        println!();
                    }
                    println!("{h}");
                }
                for line in hashid::report(h) {
                    println!("{line}");
                }
            }
            Ok(())
        }
        Some(other) => {
            eprintln!("winsploit: unknown command '{other}'. Try `winsploit --help`.");
            std::process::exit(2);
        }
        None => run_tui(catalog.as_deref(), web),
    }
}

/// A loaded catalog plus where it came from.
struct Loaded {
    path: PathBuf,
    catalog: registry::Catalog,
    /// A `.\tools.yaml` that exists but wasn't used (only reported when the
    /// user didn't pick a catalog explicitly).
    ignored: Option<PathBuf>,
}

fn load_catalog(explicit: Option<&Path>) -> Result<Loaded> {
    let path = registry::find_catalog(explicit)?;
    let ignored = match (explicit, std::env::var_os("WINSPLOIT_CATALOG")) {
        (None, None) => registry::ignored_cwd_catalog(&path),
        _ => None,
    };
    let catalog = registry::load(&path)?;
    Ok(Loaded {
        path,
        catalog,
        ignored,
    })
}

fn list(explicit: Option<&Path>) -> Result<()> {
    let Loaded {
        path,
        catalog: cat,
        ignored,
    } = load_catalog(explicit)?;
    if let Some(ignored) = ignored {
        eprintln!(
            "winsploit: note: ignoring {} (catalogs are never loaded from the current directory \
             implicitly; use --catalog to opt in)",
            ignored.display()
        );
    }
    let tools = cat.tools;
    let name_w = tools
        .iter()
        .map(|t| t.name.chars().count())
        .max()
        .unwrap_or(0);
    let cat_w = tools
        .iter()
        .map(|t| t.categories.join(",").chars().count())
        .max()
        .unwrap_or(0);

    // Write through a locked handle and stop quietly if the reader goes away
    // (`winsploit list | Select-Object -First 5`), instead of panicking like
    // println! does on a closed pipe.
    // A shell opened before an install still has the old PATH; look at the
    // persisted one too, as the dashboard does on `r`.
    envpath::refresh();
    let probes: Vec<_> = tools
        .iter()
        .map(|t| (t.exe.clone(), t.verify.clone()))
        .collect();
    let found = detect::probe_all(&probes);
    let mut out = std::io::stdout().lock();
    let res = (|| -> std::io::Result<()> {
        writeln!(
            out,
            "winsploit — {} tools  ({})",
            tools.len(),
            path.display()
        )?;
        for (t, f) in tools.iter().zip(found) {
            let state = match &f {
                detect::Found::Installed(_) => "installed",
                detect::Found::Missing => "missing",
                detect::Found::Conflict(_) => "CONFLICT",
            };
            writeln!(
                out,
                "  {:<name_w$} [{:<cat_w$}] {:<9} {}",
                t.name,
                t.categories.join(","),
                state,
                t.description
            )?;
            if let detect::Found::Conflict(p) = f {
                writeln!(
                    out,
                    "  {:<name_w$}   ^ {} is a different program named '{}'",
                    "",
                    p.display(),
                    t.exe
                )?;
            }
        }
        out.flush()
    })();
    match res {
        Err(e) if e.kind() == std::io::ErrorKind::BrokenPipe => Ok(()),
        // Windows reports a closed pipe as ERROR_NO_DATA (232)
        Err(e) if e.raw_os_error() == Some(232) => Ok(()),
        other => other.context("writing tool list"),
    }
}

/// How the web view starts (from --web, --no-web, --lan and --port).
struct WebOpts {
    start: app::StartMode,
    port: Option<u16>,
    lan: bool,
}

fn run_tui(explicit: Option<&Path>, web: WebOpts) -> Result<()> {
    let Loaded {
        path,
        catalog,
        ignored,
    } = load_catalog(explicit)?;
    if catalog.tools.is_empty() {
        eprintln!("winsploit: no tools found in {}", path.display());
        return Ok(());
    }
    // the tab / window title says winsploit while it runs (the run loop keeps
    // the working folder in it); the shell's own title comes back on exit
    let old_title = shortcut::title();
    let mut terminal = ratatui::init();
    // Bracketed paste: a pasted command line arrives as one Event::Paste
    // instead of keystrokes that could trigger key bindings. Best-effort.
    use ratatui::crossterm::{event, execute};
    let _ = execute!(std::io::stdout(), event::EnableBracketedPaste);
    // Mouse capture: lets the agent icon in the header be clickable. The
    // trade-off is that the terminal's own click-drag text selection is
    // suppressed while the dashboard is up (the run log keeps the output).
    let _ = execute!(std::io::stdout(), event::EnableMouseCapture);
    // choices left by setup.exe: the dashboard asks before installing anything
    let first_run = firstrun::Pending::load(&catalog);
    let mut app = App::new(catalog, path, ignored);
    app.settings = theme::Settings::load();
    app.restore_challenge();
    app.first_run = first_run;
    app.set_start(web.start);
    app.web_port = web.port;
    app.web_lan = web.lan;
    let result = app.run(&mut terminal);
    let _ = execute!(std::io::stdout(), event::DisableMouseCapture);
    let _ = execute!(std::io::stdout(), event::DisableBracketedPaste);
    ratatui::restore();
    if let Some(t) = old_title {
        shortcut::set_title(&t);
    }
    result
}

/// `winsploit setup --path`: put the folder this winsploit.exe is in on the
/// user PATH, so typing `winsploit` works in any new terminal. From a source
/// checkout that's target\release: each `cargo build --release` then
/// updates the command, and the catalog is found in the checkout.
fn add_self_to_path() {
    let dir = match std::env::current_exe() {
        Ok(exe) => exe.parent().map(std::path::Path::to_path_buf),
        Err(_) => None,
    };
    let Some(dir) = dir else {
        eprintln!("winsploit: setup --path: can't tell where winsploit.exe is");
        std::process::exit(1);
    };
    if dir.ends_with(r"target\debug") {
        eprintln!(
            "note: this is a debug build; run `cargo build --release` and \
             `.\\target\\release\\winsploit.exe setup --path` for the fast one"
        );
    }
    match envpath::add_user_dir(&dir) {
        Ok(true) => println!(
            "added {} to your PATH.\nOpen a new terminal and type: winsploit",
            dir.display()
        ),
        Ok(false) => println!(
            "{} is already on your PATH: type winsploit in a new terminal",
            dir.display()
        ),
        Err(e) => {
            eprintln!("winsploit: setup --path: {e:#}");
            std::process::exit(1);
        }
    }
}

/// `winsploit setup --shortcut`: a Windows Terminal profile with the cube
/// icon and a Start menu entry for this winsploit.exe.
fn add_shortcuts() {
    let exe = match std::env::current_exe() {
        Ok(e) => e,
        Err(e) => {
            eprintln!("winsploit: setup --shortcut: can't tell where winsploit.exe is: {e}");
            std::process::exit(1);
        }
    };
    match shortcut::install(&exe) {
        Ok(done) => {
            println!(
                "Start menu: {}\n  search \"{}\" or right-click it > Pin to taskbar / Start",
                done.start_menu.display(),
                shortcut::NAME
            );
            println!(
                "Windows Terminal profile: {}\n  {} in Terminal's v menu, with the cube on its tab",
                done.fragment.display(),
                shortcut::NAME
            );
            if !done.via_terminal {
                println!("  (Windows Terminal isn't installed: the shortcut starts winsploit.exe directly)");
            }
            println!(
                "To remove them, delete those two files (and {}).",
                done.icon.display()
            );
        }
        Err(e) => {
            eprintln!("winsploit: setup --shortcut: {e:#}");
            std::process::exit(1);
        }
    }
}

fn print_help() {
    println!(
        "{}\n\
winsploit {VERSION} — native-Windows security toolbox & command runner\n\n\
USAGE:\n\
    winsploit [--catalog <path>]         launch the interactive dashboard\n\
    winsploit [--catalog <path>] list    list the tool catalog with install status\n\
    winsploit hashid <hash>...           identify hash types (+ hashcat / john modes)\n\
    winsploit --help                     show this help\n\
    winsploit --version                  show version\n\
    winsploit setup --path               put this winsploit.exe on your PATH (no admin)\n\
    winsploit setup --shortcut           Start menu entry + Windows Terminal profile, with the cube icon\n\n\
WORKING FOLDER (where tools run, like a shell's current folder):\n\
    winsploit -C <folder>                start in <folder> (default: where you start winsploit)\n\
    In the dashboard: d changes it, like cd: Tab completes, .. ~ - and %VARS% work,\n\
    a file's path means its folder. Switching challenge (C) moves you into its folder.\n\n\
WEB VIEW (the same live session in your browser; the terminal stays in charge):\n\
    winsploit --web                      start it and open it in the browser\n\
    winsploit --no-web                   terminal only (skips the start-up question)\n\
    winsploit --port <n>                 serve on port <n> (default 7878, else a free port)\n\
    winsploit --lan                      serve your local network too, over HTTPS: guests need a\n\
                                         one-time join key and your OK, as watchers or with full control\n\
    In the dashboard: w starts / stops it, W serves your LAN, L manages guests, o opens it in the browser.\n\n\
CATALOG (tools.yaml) is looked up in this order:\n\
    --catalog <path>, then %WINSPLOIT_CATALOG%, then next to winsploit.exe,\n\
    then the source checkout (dev builds), then %APPDATA%\\winsploit\\tools.yaml.\n\
    It is never read from the current directory unless you pass --catalog.",
        banner::LOGO
    );
}

#[cfg(test)]
mod bucket_tests;
#[cfg(test)]
mod fuzz;
