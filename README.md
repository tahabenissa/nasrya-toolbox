<div align="center">

```
 ██╗    ██╗ ██╗ ███╗   ██╗ ███████╗ ██████╗  ██╗       ██████╗  ██╗ ████████╗
 ██║    ██║ ██║ ████╗  ██║ ██╔════╝ ██╔══██╗ ██║      ██╔═══██╗ ██║ ╚══██╔══╝
 ██║ █╗ ██║ ██║ ██╔██╗ ██║ ███████╗ ██████╔╝ ██║      ██║   ██║ ██║    ██║
 ██║███╗██║ ██║ ██║╚██╗██║ ╚════██║ ██╔═══╝  ██║      ██║   ██║ ██║    ██║
 ╚███╔███╔╝ ██║ ██║ ╚████║ ███████║ ██║      ███████╗ ╚██████╔╝ ██║    ██║
  ╚══╝╚══╝  ╚═╝ ╚═╝  ╚═══╝ ╚══════╝ ╚═╝      ╚══════╝  ╚═════╝  ╚═╝    ╚═╝
```

### The native-Windows security toolbox, in your terminal.

**Find, install and run 45 curated pentest & CTF tools from one keyboard-driven dashboard.**<br>
No WSL. No VM. No guesswork about what actually works on Windows.

[![CI](https://github.com/tahabenissa/nasrya-toolbox/actions/workflows/ci.yml/badge.svg)](https://github.com/tahabenissa/nasrya-toolbox/actions/workflows/ci.yml)
[![Release](https://img.shields.io/github/v/release/tahabenissa/nasrya-toolbox?include_prereleases&sort=semver)](https://github.com/tahabenissa/nasrya-toolbox/releases)
![status](https://img.shields.io/badge/status-alpha-orange)
![platform](https://img.shields.io/badge/platform-Windows%2010%20%7C%2011-0078D6)
![rust](https://img.shields.io/badge/rust-1.87%2B-B7410E)
![tools](https://img.shields.io/badge/catalog-45%20tools-8A2BE2)
![tests](https://img.shields.io/badge/tests-82%20passing-brightgreen)
[![license](https://img.shields.io/badge/license-MIT-green)](LICENSE)

[Quick start](#-quick-start) •
[Features](#-features) •
[Catalog](#-the-catalog) •
[Usage](#-usage) •
[Adding tools](#-adding-a-tool) •
[Security model](#-security-model) •
[Architecture](#-architecture) •
[Roadmap](#-roadmap)

</div>

---

> [!WARNING]
> **Authorized use only.** `winsploit` is for authorized penetration testing, security research,
> CTFs and learning. Only point it at systems you own or have **explicit written permission** to
> test. Unauthorized scanning or attacking is illegal in most jurisdictions, and staying inside
> your rules of engagement is your responsibility.

---

## Why winsploit?

Kali is the home of security tooling, but plenty of people work on **Windows**, and the existing
options all make you compromise:

| Option | What's good | What's missing |
|---|---|---|
| **Commando VM / FLARE-VM** | Excellent, thorough | One-shot *VM provisioners*, not something you open every day |
| **PentestBox** | Right idea: native and portable | Abandoned since ~2016, unclear licensing |
| **UniGetUI** | Great package GUI | No security curation, no "run the tool" workflow |
| **winsploit** | Curated, native, installs **and** runs, MIT, actively developed | Alpha, see [Roadmap](#-roadmap) |

There is no actively maintained, cleanly licensed, native-Windows launcher that both *installs*
and *runs* security tools from one interface. `winsploit` fills that gap, and it's honest about what
really works on Windows and what needs Linux.

## ⚡ Quick start

**You need:** Windows 10/11, [Scoop](https://scoop.sh) (winsploit installs tools through it), and
[Rust 1.87+](https://rustup.rs) to build from source.

```powershell
# 1. Install Scoop (skip if you have it)
Set-ExecutionPolicy -ExecutionPolicy RemoteSigned -Scope CurrentUser
Invoke-RestMethod -Uri https://get.scoop.sh | Invoke-Expression

# 2. Build winsploit
git clone https://github.com/tahabenissa/nasrya-toolbox
cd nasrya-toolbox
cargo build --release

# 3. Launch the dashboard (a dev build finds the checkout's tools.yaml from any directory)
.\target\release\winsploit
```

Pick a tool with `↑/↓` (or `/` to filter), press `i` to install it and `Enter` to run it. Press
`?` any time to see every key.

> [!NOTE]
> **Prebuilt binaries:** tagging `v*` publishes a Windows x64 zip (`winsploit.exe` + `tools.yaml`) on
> the [Releases](https://github.com/tahabenissa/nasrya-toolbox/releases) page. The current
> `v0.1.0` release **predates** everything listed under [What's new](#-whats-new), so build from
> source for now. The binary is unsigned, so Smart App Control may ask you to allow it.

## ✨ Features

### 🎛️ A dashboard built for the keyboard
- **Browse by category** with live install status: `●` installed, `○` missing, `◐` conflict.
- **Instant filter (`/`)** by name, category or description. Words AND together.
- **A real line editor** for arguments: word jumps, word delete, paste, and per-tool **history**
  with `↑/↓`.
- **Key help overlay (`?`)** so the footer stays short.

### 📡 Live, interactive output
- stdout/stderr **stream into the output pane** in real time, color-coded.
- **Talk to running tools.** Each process gets its own input pipe, so you can answer prompts
  (`Ctrl+D` = end of input) without fighting the dashboard for keystrokes.
- **Scrollback** (`PgUp/PgDn/Home/End`) and **find in output** (`f`, `n`, `N`) with highlighting.
- **Survives messy output:** non-UTF-8 bytes don't stop the stream, and carriage-return progress
  bars and ANSI escape codes are cleaned up instead of smearing the pane.
- **`c` cancels** the whole process tree, not just the parent.

### 📦 One-key installs, done properly
- **Scoop** for native tools, with **missing buckets added automatically** the first time
  they're needed (`bucket/pkg` is pinned so the right package wins).
- **pipx** for Python tools. Each gets its own venv, and winsploit installs Python and pipx
  through Scoop first if they're missing.
- **winsploit's own Scoop bucket** for tools nobody else packages (gau, John jumbo, steghide, nikto,
  SecLists-lite). Each has a pinned URL and **SHA-256 hash**.
- **PATH changes picked up live** after an install, with no restart needed.
- Detection runs **in-process and off the UI thread**, so the dashboard never freezes.

### 🧠 Built-in helpers
- **Hash identifier** (`h` in the dashboard, or `winsploit hashid <hash>`). Works offline, knows
  37 hash formats, and gives the matching **`hashcat -m`** mode and **`john --format`** name.
- **Per-run log files** for every run and install: the command, all output (including what you
  typed), the exit code and timestamps. Ready to use as evidence in a report.

### 🧩 Config-driven, honest by design
- Every tool is a **YAML entry**, so you add one without touching code.
- **Four launch modes:** `stream` (CLI in the pane), `gui` (desktop apps), `web` (start a server,
  open the browser) and `dataset` (wordlists).
- **Impostor detection:** `verify:` catches same-named programs (ProjectDiscovery `httpx` vs the
  Python `httpx`, Sysinternals `strings` vs GNU `strings`) and refuses to launch the wrong one.
- **Per-tool caveats** (`note:`) are shown on every run, for example "needs Npcap + admin".

## 🧰 The catalog

**45 tools across 11 categories**, all chosen because they run well **natively** on Windows.

| Category | Tools |
|---|---|
| **Recon** | nmap · rustscan · gobuster · httpx · subfinder · amass · gau |
| **Web** | sqlmap · nuclei · ffuf · feroxbuster · katana · arjun · dirsearch · wfuzz · commix · nikto · ZAP 🖥️ |
| **Passwords** | hashcat · John the Ripper (jumbo) · hashid · NetExec |
| **Crypto** | openssl · RsaCtfTool · CyberChef 🌐 |
| **Forensics** | exiftool · strings (Sysinternals) · Sleuth Kit · Volatility 3 |
| **Reversing** | radare2 · upx · jadx · apktool · Detect It Easy · Cutter 🖥️ · x64dbg 🖥️ · dnSpy 🖥️ · Ghidra 🖥️ |
| **Pwn** | pwntools · ropper |
| **OSINT** | theHarvester · Sherlock |
| **Stego** | steghide |
| **Sniffing** | tshark |
| **Wordlists** | SecLists-lite 📚 (wired into gobuster, ffuf, feroxbuster, dirsearch, wfuzz and john) |

<sub>🖥️ desktop app (`launch: gui`) · 🌐 opens in your browser (`launch: web`) · 📚 dataset (`launch: dataset`)</sub>

### Honest scope

| ✅ Works great | ⚠️ Works, with notes | ❌ Use Linux instead |
|---|---|---|
| nmap, nuclei, ffuf, gobuster, httpx, subfinder, katana, radare2, Ghidra | **hashcat** needs GPU drivers · **tshark** needs Npcap and admin · Python tools need pipx (winsploit bootstraps it) | **Live Wi-Fi capture** (aircrack-ng: no monitor mode or injection) · THC-Hydra, WPScan, WhatWeb, masscan (degraded on Windows) |

> [!IMPORTANT]
> **Wireless, honestly:** live Wi-Fi monitor mode and packet injection **don't work** on Windows
> with normal adapters. Only *offline* cracking of files you've already captured is useful here.
> For real wireless work, use Linux with a supported USB adapter. That's why aircrack-ng is
> deliberately left out of the catalog.

## 🚀 Usage

```text
winsploit                    launch the interactive dashboard
winsploit list               list the catalog with install status (installed / missing / CONFLICT)
winsploit hashid <h> [<h>…]  identify one or more hashes (hashcat -m / john --format hints)
winsploit --help, -h         show help
winsploit --version, -V      show version

--catalog <path>          use a specific tools.yaml (works with or without `list`)
```

### Dashboard keys

| Key | Action |
|---|---|
| `↑` `↓` | Move selection |
| `/` | Filter by name, category or description (`Esc` clears) |
| `Enter` | Run the selected tool (edit the prefilled args, then `Enter`) |
| *while running* | Type a line to send to the tool's input · `Ctrl+D` = end of input |
| `i` | Install the selected tool (Scoop or pipx) |
| `r` | Re-detect what's installed |
| `PgUp` `PgDn` | Scroll the output · `Home` = top · `End` = follow live output |
| `f` · `n` · `N` | Find in output · older match · newer match |
| `c` | Cancel the running process (kills the whole tree) |
| `h` | Identify a hash (built in, offline) |
| `?` | Show all keys |
| `q` | Quit |

### Editing arguments

The args and hash prompts are real line editors:

| Keys | Action |
|---|---|
| `←` `→` · `Ctrl+←` `Ctrl+→` | Move by character · by word |
| `Home` `End` (or `Ctrl+A` `Ctrl+E`) | Start / end of line |
| `Del` · `Ctrl+Backspace` / `Ctrl+W` · `Ctrl+U` | Delete char · delete word · clear to start |
| `↑` `↓` | Cycle through args you've used with this tool (per session) |
| Paste | Whole command lines work |

**Quoting:** arguments split on spaces, and `"double"` or `'single'` quotes keep spaces together:

```text
-H "User-Agent: winsploit"     -w "C:\My Lists\big.txt"
```

Backslashes are literal (no need to double them) and `\"` is a literal quote. A quoted path can't
end in a backslash, so write `"C:\dir"`, not `"C:\dir\"`.

### Logs

Every run and install is also written to:

```text
%LOCALAPPDATA%\winsploit\logs\<UTC timestamp>-<tool>.log
```

The path is shown when the run starts, and nothing is lost when output scrolls past the in-memory
buffer.

### Extra Scoop buckets

Pressing `i` adds any bucket a tool needs the first time it's needed. To add them yourself:

```powershell
scoop bucket add security https://github.com/opsqw/scoop-security   # nuclei, httpx, subfinder, katana, ffuf
scoop bucket add extras                                             # Sysinternals, jadx, Ghidra, x64dbg, …
scoop bucket add winsploit https://github.com/tahabenissa/nasrya-toolbox   # gau, john, steghide, nikto, seclists-lite
```

## ➕ Adding a tool

Edit [`tools.yaml`](tools.yaml). No code changes needed, and the header of that file documents
every field.

<details open>
<summary><b>Native tool (Scoop)</b></summary>

```yaml
  - name: gobuster
    category: recon
    description: Directory / DNS / vhost brute-forcing
    exe: gobuster          # used to detect installation (PATH probe)
    scoop: gobuster        # Scoop package to install
    run_cmd: gobuster      # command actually launched
    args_hint: 'dir -u https://example.com -w "{app:seclists-lite}\common.txt"'
    bucket: security       # optional; main-bucket tools omit it
```

A bucket Scoop doesn't know by name also needs its URL in the top-level `buckets:` map, so winsploit
can add it on first install.
</details>

<details>
<summary><b>Python tool (pipx)</b></summary>

```yaml
  - name: arjun
    category: web
    description: HTTP parameter discovery
    exe: arjun
    install: pipx
    pipx: arjun            # or arjun==2.2, or git+https://github.com/owner/repo
    run_cmd: arjun
    args_hint: "-u https://example.com/endpoint"
```

Each tool gets its own venv. If pipx is missing, winsploit installs Python and pipx through Scoop
first.
</details>

<details>
<summary><b>Tool nobody packages (winsploit's own bucket)</b></summary>

Add a Scoop manifest to [`bucket/`](bucket) with a pinned `url` and `hash`, then set
`bucket: winsploit` in the tool's entry. Scoop handles the download, **SHA-256 verification**,
extraction and shims, and winsploit adds the bucket on first use. `cargo test` checks that every
manifest is well-formed and that every `bucket: winsploit` entry has one.
</details>

<details>
<summary><b>Desktop apps, web apps & datasets</b></summary>

```yaml
    launch: gui                         # Ghidra, x64dbg, …: opens in its own window
    # or
    launch: web                         # start run_cmd (if any, streamed and cancellable), then open url
    url: http://127.0.0.1:5001
    # or
    launch: dataset                     # wordlists: Enter lists the files and copies the folder path
```

**Placeholders**

| Placeholder | Expands to | Allowed in |
|---|---|---|
| `{scoop_app}` | This tool's Scoop folder (`<scoop>\apps\<pkg>\current`) | `exe`, `run_cmd`, `url` |
| `{app:NAME}` | Another installed Scoop app's folder | `exe`, `run_cmd`, `url`, `args_hint` |

Use `{scoop_app}` for apps Scoop doesn't put on PATH (`exe: '{scoop_app}\dnSpy.exe'`) or that are
just an HTML page (`url: '{scoop_app}\CyberChef.html'`).

> [!TIP]
> Write Windows paths in `'single quotes'`. Inside `"double quotes"`, YAML turns `\r`, `\x`, …
> into escape sequences.
</details>

<details>
<summary><b>Same-named impostors & caveats</b></summary>

```yaml
    verify: { args: "-version", contains: "projectdiscovery" }
    note: "Needs Npcap and an elevated terminal."
```

`verify:` runs the `exe` found on PATH (3 s timeout). If the output doesn't contain the text, the
tool shows as a conflict (`◐`) along with the path that won, and it **won't be launched**. `note:`
is shown every time the tool runs. Bad entries (a missing `scoop:`/`pipx:`, a web tool without
`url:`, …) are all reported together when the catalog loads.
</details>

## 🔒 Security model

winsploit runs tools against real targets, often from folders full of untrusted files. It's built for
that:

- **No implicit catalog loading from the current directory.** The catalog decides what `Enter`
  runs, so a malicious `tools.yaml` in a CTF folder, a loot dir or a cloned repo is **ignored**.
  winsploit tells you it skipped one, and you can opt in with `--catalog .\tools.yaml`.
- **Catalog resolution order:** `--catalog <path>`, then `%WINSPLOIT_CATALOG%`, then the one next to
  `winsploit.exe`, then the source checkout (dev builds), then `%APPDATA%\winsploit\tools.yaml`. The
  active path is shown in the dashboard.
- **Wrong binaries are blocked.** `verify:` checks catch same-named programs earlier on PATH.
- **Downloads are hash-pinned.** Every manifest in winsploit's bucket has a fixed URL and SHA-256.
- **Arguments are parsed, not shelled.** Quote-aware splitting builds a real argv; there's no
  command-string concatenation. Programs are resolved by PATH × PATHEXT (never the current
  directory), and `.cmd`/`.bat` shims go through Rust's escaped cmd.exe path.
- **Audit trail.** Every run and install leaves a timestamped log.

## 🏗️ Architecture

```mermaid
flowchart LR
    Y[tools.yaml] --> R[registry<br/>load + validate]
    R --> D[detect<br/>PATH × PATHEXT + verify]
    R --> I[install<br/>scoop · pipx · buckets]
    I --> E[envpath<br/>live PATH refresh]
    E --> D
    D --> A[app<br/>state machine]
    A --> U[ui<br/>ratatui dashboard]
    A --> X[runner<br/>spawn · stream · stdin · kill tree]
    X --> L[runlog<br/>per-run logs]
    X --> U
```

| Module | Responsibility |
|---|---|
| `main.rs` | CLI entry point: TUI, `list`, `hashid`, `--catalog` |
| `registry.rs` | Loads and validates the YAML catalog, resolves the catalog path, expands placeholders |
| `detect.rs` | PATH × PATHEXT detection (ground truth) and `verify:` impostor checks |
| `install.rs` | Install planning (Scoop / pipx, including Python and pipx bootstrap) |
| `scoop.rs` | Scoop buckets, package installs, app directories |
| `envpath.rs` | Picks up PATH changes made after winsploit started |
| `args.rs` | Quote-aware argument splitting |
| `editor.rs` | Args prompt line editor and per-tool history |
| `hashid.rs` | Built-in hash identifier |
| `runner.rs` | Spawn, stream, stdin pipe, detach (gui/web), kill tree, output cleaning |
| `runlog.rs` | Per-run log files |
| `app.rs` | State machine, launch/install control, scrollback, search, filter |
| `ui.rs` · `banner.rs` | Dashboard rendering, ASCII splash and wordmark |
| `bucket_tests.rs` | Validates every manifest in `bucket/` against the catalog |

### Development

```powershell
cargo test                      # 82 unit tests
cargo fmt --all -- --check
cargo clippy --all-targets
```

CI builds, lints and smoke-tests (`--version`, `list`) on `windows-latest` for every push and PR.
Pushing a `v*` tag builds and publishes a release zip.

## 🆕 What's new

<details open>
<summary><b>Unreleased (since v0.1.0)</b></summary>

**Catalog: 6 → 45 tools**
- Recon and web: gobuster, feroxbuster, httpx, subfinder, amass, katana, rustscan, gau, nikto.
- Reversing: radare2, upx, jadx, apktool, Detect It Easy, Cutter, x64dbg, dnSpy, Ghidra.
- Forensics, crypto, stego: exiftool, strings, Sleuth Kit, openssl, CyberChef, steghide.
- Python tools via pipx: arjun, dirsearch, wfuzz, commix, hashid, NetExec, RsaCtfTool,
  Volatility 3, ropper, pwntools, theHarvester, Sherlock.
- Passwords and data: John the Ripper (jumbo), ZAP, SecLists-lite wordlists.

**Engine**
- pipx installs and `gui` / `web` / `dataset` launch modes.
- winsploit's own hash-pinned Scoop bucket for unpackaged tools.
- `{scoop_app}` and `{app:NAME}` placeholders.
- Automatic Scoop bucket add, with `bucket/pkg` pinning.
- `verify:` impostor detection (`◐` conflict state).
- Detection runs in-process and off the UI thread, and PATH changes are picked up live.
- `.cmd`/`.bat` shims launch correctly.

**Runner and output**
- Each running tool gets its own stdin pipe, which fixes key races with the dashboard.
- Streaming survives non-UTF-8 output. CR progress bars and ANSI codes are cleaned.
- Output pane shows the exact tail with long lines, plus scrollback.
- Find in output (`f` / `n` / `N`).
- Per-run log files.

**Dashboard**
- Tool list filter (`/`), key help overlay (`?`) and a shorter footer.
- Args prompt with real line editing, history and paste.
- Built-in hash identifier (`h` / `winsploit hashid`).
- Tool-name column sized to the longest name.

**Security and hygiene**
- Never loads `tools.yaml` from the current directory implicitly.
- Quote-aware argument splitting.
- Crate renamed to `winsploit`, `Cargo.lock` tracked, `rust-version` declared, clippy clean.
- Security bucket moved to its new home, `opsqw/scoop-security`.
</details>

## 🗺️ Roadmap

- [x] Rich catalog: 45 tools across recon, web, passwords, crypto, forensics, stego, reversing, pwn, OSINT and wordlists
- [x] Scoop bucket auto-add · pipx installs · GUI / web / dataset launch modes · winsploit's own bucket
- [x] Scrollback, output search, per-run log files
- [x] Built-in hash identifier
- [ ] Per-engagement workspaces (scope list, authorization record, audit log)
- [ ] Scope enforcement before every launch (block, or override with a reason)
- [ ] Artifact collection (files a tool writes, e.g. `-oX` / `-o`, kept next to the run log)
- [ ] One-shot report generation (Markdown / HTML)
- [ ] New tagged release with prebuilt binary

## 🤝 Contributing

New tools are the easiest place to start: add a `tools.yaml` entry (and a `bucket/` manifest if
nobody packages the tool), run `cargo test`, and open a PR. Please only add tools that really
work natively on Windows, and use `note:` to document any caveats.

## 🙏 Credits & prior art

Inspired by the native-toolbox idea behind **PentestBox**, the provisioning model of
**Commando VM / FLARE-VM** and the package catalogs of the **Scoop** security buckets. Built with
[ratatui](https://ratatui.rs). Tool curation draws on
[awesome-pentest](https://github.com/enaqx/awesome-pentest).

## 📄 License

[MIT](LICENSE) © winsploit contributors. Each bundled or installed tool keeps its own license.
