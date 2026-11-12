# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

Winsploit (formerly nasrya; the GitHub repo is `tahabenissa/winsploit`) is a native-Windows
ratatui TUI that detects, installs (via Scoop / pipx) and runs pentest tools listed in `tools.yaml`.
Windows-only: detection, installs, process trees and paths all assume Windows.

## Commands

`cargo` lives in `%USERPROFILE%\.cargo\bin`, which may not be on the Bash tool's PATH (use `~/.cargo/bin/cargo`).

```powershell
cargo build                                   # debug build -> target\debug\winsploit.exe
cargo run                                     # dashboard (needs a real interactive terminal)
cargo run -- list                             # non-interactive: catalog + install status
cargo run -- hashid <hash>                    # non-interactive: built-in hash identifier
cargo run -- --catalog .\tools.yaml           # use a specific catalog
cargo test                                    # all unit tests
cargo test ui::tests::categories_get_their_own_row   # one test (substring of the path works too)
cargo fmt --all
cargo clippy --all-targets -- -D warnings     # kept clean, though CI only runs clippy/fmt as advisory
$env:WINSPLOIT_SNAP_DIR = "C:\tmp\snaps"; cargo test snapshots -- --ignored   # TUI screens as HTML pages, for screenshots
node site/scripts/app-icon.mjs                # regenerate assets/winsploit.ico (embedded by build.rs)
```

CI (`.github/workflows/ci.yml`, windows-latest) hard-gates only on `cargo build --release` plus a smoke
test of `--version` and `list`. Pushing a `v*` tag runs `release.yml`.

A running `winsploit.exe` locks `target\debug\winsploit.exe`, so `cargo build` fails with
"failed to remove file" until that instance is closed.

With the GNU toolchain (`stable-x86_64-pc-windows-gnu`), `build.rs` (windres) fails in a checkout whose
path has a space (`cc1.exe: fatal error: ... out: No such file or directory`). Build into a
space-free target dir instead: `$env:CARGO_TARGET_DIR = "C:\tmp\winsploit-target"`.

## Architecture

**Single-threaded event loop with background workers.** `App::run` (app.rs) redraws, drains an
`mpsc` channel of `runner::AppEvent`s, then polls the keyboard for 80 ms. Everything slow runs on
other threads and reports back through that channel:
- `AppEvent::Output { sid, line }` / `Exited { sid, code }` from spawned processes (runner.rs)
- `AppEvent::Detected(..)` from the off-thread detection pass (detect.rs)

Events are routed by **session id**, not by slot. A `Session` (session.rs) is one terminal: output
buffer, stdin pipe, scroll/search state, run log. `App` holds the always-open `console` (id
`CONSOLE` = 0, which takes installs, hash ids and messages) plus `slots: [Option<Session>; SLOTS]` (4
tool terminals). Ids are never reused, so late output from a replaced terminal is simply dropped.
`App::focus` is a *position* (0 = console, 1..=4 = slot).

**Rendering is a pure function of `App`.** `ui::render` reads state and never mutates it, except the
`Cell` in `Session::view` that it fills so scroll math knows the pane size, and `App::hits` (plus
`agent_icon`), where it records what it drew where. `App::on_mouse` hit-tests those rects and turns a
click into the key it stands for (or an `Action`), so mouse, keys and web share one path. Input `Mode`s, the
overlays (`appearance`, `show_help`, `tour`) and `confirm` (second press for destructive keys) all
live in `App`, and `on_key` dispatches by whichever is active (tour → appearance → help → mode).

**All styling goes through `theme.rs`.** `Settings` (saved to `%APPDATA%\winsploit\settings.yaml`)
resolves to a `Theme`: a semantic `Palette` (accent/ok/err/warn/info/dim/…), a `Glyphs` set (Unicode
or ASCII) and a border set. Don't hardcode colors or glyphs in ui.rs. Two invariants:
- When `NO_COLOR` is set, crossterm strips *all* colors including backgrounds (`Theme::mono` /
  `App::mono`). Any state shown only by a background color disappears, so use `Theme::selected()` /
  `pill()`, which fall back to reverse video.
- ASCII glyph mode must not emit box-drawing chars or symbol icons (a test checks this). Gate fancy glyphs
  on `th.glyph.icons` or put them in `Glyphs`.

**CTF layer.** `workspace.rs` (challenge folders under `%USERPROFILE%\ctf`, ids `<event>/<challenge>`
that only resolve to scanned folders), `recipe.rs` (placeholders `{target}` `{file}` `{out}`
`{wordlist}`, written unquoted, each filled as one quoted argument) and `flags.rs` (flag
scanner). Logs and flags follow the active challenge (`App::logs_dir`, `App::flags_path`).
Tests must not touch the real clipboard, `%APPDATA%` or `%USERPROFILE%\ctf`: use a temp
`ctf_root`, and `cfg!(test)` guards like `catch_flags` / `set_challenge`.

**The catalog (`tools.yaml`) is the extension point.** Adding a tool needs no code. registry.rs
parses it, folds the legacy `category:` into `categories:` (primary first), expands placeholders
(`{scoop_app}`, `{app:NAME}`), and reports every validation problem at once. `launch:` (stream / gui
/ web / dataset) decides how Enter runs a tool. `verify:` marks a same-named impostor on PATH as a
`Conflict`, and conflicts are never launched. Tools unpackaged elsewhere get a hash-pinned Scoop
manifest in `bucket/`, and `bucket_tests.rs` validates those manifests against the catalog.

**Security invariants (see README "Security model"); keep them intact:**
- `registry::find_catalog` never loads `tools.yaml` from the current directory implicitly. Order:
  `--catalog`, `%WINSPLOIT_CATALOG%`, next to the exe, the source checkout (dev builds), then
  `%APPDATA%\winsploit\tools.yaml`.
- Arguments are split quote-aware into a real argv (args.rs). Never build a shell command string.
- Executables resolve via PATH × PATHEXT, never the cwd. `.cmd`/`.bat` shims go through Rust's
  escaped cmd.exe path.
- Each run and install writes a timestamped log (runlog.rs, under `%LOCALAPPDATA%\winsploit\logs`).
- Secrets stay out of the pane's `$` line and the log (redact.rs, `redact:` setting, default
  `commands`): show a tool's command line through `redact::command` (it applies the catalog's
  `secret_args:`), never a raw `args::join(&args)`. Typed input at a password prompt is masked in
  `Session::send`. Tool output is left verbatim unless the level is `all`, since it is evidence.
- The AI never runs anything itself (agent.rs, chat.rs): it names a catalog tool and args, which
  winsploit parses with `args::split` and runs through `App::launch`. The `claude` CLI is called
  isolated (`agent::claude_args`: `--tools ""`, `--strict-mcp-config`, no hooks, scratch folder,
  own system prompt) because the prompt carries untrusted tool output and the CLI would otherwise
  inherit the user's permissions and hooks (a live test once let a Stop hook edit a file). Keep that
  fail-closed. The side chat (`T`, `Chat` in app.rs) is terminal only, like Sandbox runs: no
  `WebAction` may open it or send to it. `chat-autorun` stays off by default.
- `scope.rs` is a pure matcher (#64) that nothing calls yet; keep it free of UI and I/O so the
  scope-enforcement work (#56, #66) can build on it. Its doc comment lists the decisions (apex vs
  wildcard, exclusions win, `Unknown` means "needs DNS").
- The web view (web.rs) is off by default, binds 127.0.0.1 unless `--lan`, needs the per-session
  token/cookie, checks Host (IP literals only) and Origin + JSON on POSTs, and only accepts
  `WebAction`s. Never add an endpoint that runs a command line or serves arbitrary files.
  LAN mode is HTTPS only (tls.rs; plain HTTP is accepted from loopback only, the LAN gets a
  redirect). The owner token works only from a loopback peer and never goes into the LAN URL.
  Guests come in with a one-time join key **plus** the owner's approval, and roles are enforced
  server-side: watchers get 403 on actions, and guest management (`admit` / `remove` / `newkey`)
  is owner-only. Owner-only data (key, guest list) goes in a frame that is never sent to guests.
- Windows Sandbox runs (`S`, sandbox.rs) map the tool and sample **read-only**, with one
  writable output folder; network, clipboard, vGPU, audio, camera and printers are off
  (network can be turned on per run). `check_sample_dir` refuses the profile itself, AppData,
  Scoop, Program Files and Windows. Anything that ends up on the logon `cmd.exe` line must pass
  `cmd_safe`. Windows Sandbox can't run in CI, so this is covered by unit tests on the plan
  and `.wsb` output; test a real launch by hand on Windows Pro.
  Sandbox runs are terminal-only on purpose (not an `app::Action`): the web view must never
  choose host folders to map into a VM.

**Terminal and web share one code path.** Keys and web requests both become an `app::Action`
and go through `App::apply`; `sync_web` mirrors sessions, toasts and state to the browser once
per frame. A new user-facing action needs a `kind`, a key in `Action::BINDINGS` (listed in the
`?` overlay) and a `data-action="<kind>"` control in `src/web/index.html`; the
`every_action_has_a_key_and_a_web_control` test enforces it. The page (index.html, app.css,
app.js) is embedded with `include_str!`, so rebuild after editing it.

## Testing conventions

- UI tests render into ratatui's `TestBackend` via the `draw(&app, w, h)` / `row(..)` / `screen(..)`
  helpers in `ui.rs` tests, and build `App` from the real `tools.yaml` (`include_str!`). Changes to
  visible strings or layout usually mean updating those assertions.
- Property/fuzz tests (args, recipe, scope, redact) use the test-only `fuzz.rs`: a seeded `Rng`
  plus a corpus of nasty strings, with no extra dependency. A failing case reproduces from its
  seed and prints the offending input.
- Fake sessions in tests keep `pid: None`, so `kill()` never runs `taskkill`.
- Tests must not write the real `%APPDATA%`. `App::finish_tour` skips `settings.save()` under
  `cfg!(test)`; follow that pattern.

## Repo conventions

- **Find a skill before a new process or feature.** Before starting a new kind of work (a new
  feature, an audit, a review, a release step…), run `/find-skills` to look for a skill that fits
  the workflow, unless one is already installed for it. Check it the way find-skills says
  (install count, source, and read its `SKILL.md` before installing), install it
  (`npx skills add <owner/repo> --skill <name> -g -a claude-code -y --copy`), then run it for the
  task. If nothing fits, say so and carry on without one.
- **Pull before you change anything.** Several people push straight to `main`, so at the start of
  every task, before reading code to plan an edit, run `git pull --rebase origin main`, and read
  `git log --oneline -5` to see what landed. If the working tree has uncommitted changes, don't
  stash or discard them; ask first. Pull again right before pushing, and if the rebase conflicts,
  keep both sides' work and rerun `cargo test` before pushing. Never force-push.
- Commit and push directly to `main` (no PR branches).
- Commit messages: a short imperative subject, then a wrapped body explaining what changed and why
  (see `git log`).
