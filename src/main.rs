//! winsploit — native-Windows security toolbox & command runner.
//!
//! Usage:
//!   winsploit [--catalog <path>]         launch the interactive dashboard (TUI)
//!   winsploit [--catalog <path>] list    print the tool catalog and exit
//!   winsploit hashid <hash>...           identify hash types
//!   winsploit --help                     show this help
//!   winsploit --version                  show version

mod app;
mod args;
mod banner;
mod detect;
mod editor;
mod envpath;
mod hashid;
mod install;
mod registry;
mod runlog;
mod runner;
mod scoop;
mod ui;

use std::io::Write;
use std::path::{Path, PathBuf};

use anyhow::{Context, Result};
use app::App;

const VERSION: &str = env!("CARGO_PKG_VERSION");

fn main() -> Result<()> {
    let mut args: Vec<String> = std::env::args().skip(1).collect();

    // --catalog <path> / --catalog=<path> may appear anywhere.
    let mut catalog: Option<PathBuf> = None;
    let mut i = 0;
    while i < args.len() {
        if let Some(v) = args[i].strip_prefix("--catalog=") {
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
        None => run_tui(catalog.as_deref()),
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
        .map(|t| t.category.chars().count())
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
        writeln!(out, "winsploit — {} tools  ({})", tools.len(), path.display())?;
        for (t, f) in tools.iter().zip(found) {
            let state = match &f {
                detect::Found::Installed(_) => "installed",
                detect::Found::Missing => "missing",
                detect::Found::Conflict(_) => "CONFLICT",
            };
            writeln!(
                out,
                "  {:<name_w$} [{:<cat_w$}] {:<9} {}",
                t.name, t.category, state, t.description
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

fn run_tui(explicit: Option<&Path>) -> Result<()> {
    let Loaded {
        path,
        catalog,
        ignored,
    } = load_catalog(explicit)?;
    if catalog.tools.is_empty() {
        eprintln!("winsploit: no tools found in {}", path.display());
        return Ok(());
    }
    let mut terminal = ratatui::init();
    // Bracketed paste: a pasted command line arrives as one Event::Paste
    // instead of keystrokes that could trigger key bindings. Best-effort.
    use ratatui::crossterm::{event, execute};
    let _ = execute!(std::io::stdout(), event::EnableBracketedPaste);
    let result = App::new(catalog, path, ignored).run(&mut terminal);
    let _ = execute!(std::io::stdout(), event::DisableBracketedPaste);
    ratatui::restore();
    result
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
    winsploit --version                  show version\n\n\
CATALOG (tools.yaml) is looked up in this order:\n\
    --catalog <path>, then %WINSPLOIT_CATALOG%, then next to winsploit.exe,\n\
    then the source checkout (dev builds), then %APPDATA%\\winsploit\\tools.yaml.\n\
    It is never read from the current directory unless you pass --catalog.",
        banner::LOGO
    );
}

#[cfg(test)]
mod bucket_tests;
