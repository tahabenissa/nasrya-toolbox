//! Main dashboard rendering. Every colour, glyph and border comes from the
//! active `Theme` (see theme.rs), so the `t` overlay can restyle it live.

use std::time::Instant;

use ratatui::crossterm::event::KeyCode;
use ratatui::layout::{Alignment, Constraint, Direction, Layout, Margin, Rect};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, Clear, List, ListItem, ListState, Paragraph, Wrap};
use ratatui::Frame;

use crate::app::{
    AgentView, App, Appearance, CatTab, ChallengeField, Confirm, Hit, JoinPrompt, Layer, Mode,
    ProjectUi, ProviderPick, ToastKind, ToolStatus, JOIN_CHOICES, JOIN_GRACE, TOUR_STEPS,
};
use crate::banner;
use crate::registry::{self, Launch, Sandbox};
use crate::session::{RunState, Session, SLOTS};
use crate::theme::{Backdrop, Field, OverlayPos, Theme, ThemeName};
use crate::web::{GuestStatus, KeyState};

pub fn render(f: &mut Frame, app: &App) {
    // this frame's clickable spots, recorded as things are drawn
    app.hits.borrow_mut().clear();
    let mut th = app.settings.theme();
    th.mono = app.mono;
    let th = th;
    let area = f.area();
    let chunks = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(1), // header
            Constraint::Length(1), // category bar, full width
            Constraint::Min(3),    // body
            Constraint::Length(1), // footer / input
        ])
        .split(area);

    render_header(f, app, &th, chunks[0]);
    render_categories(f, app, &th, chunks[1]);

    let body = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([Constraint::Percentage(36), Constraint::Percentage(64)])
        .split(chunks[2]);

    // left: tool list, card for the selected tool
    let left = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Min(3),
            Constraint::Length(if body[0].height >= 15 { 6 } else { 0 }),
        ])
        .split(body[0]);
    render_tool_list(f, app, &th, left[0]);
    render_tool_card(f, app, &th, left[1]);

    // right: terminal tabs, then one terminal or the 2x2 grid
    let right = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Length(1), Constraint::Min(3)])
        .split(body[1]);
    render_terminal_tabs(f, app, &th, right[0]);
    render_terminals(f, app, &th, right[1]);
    render_footer(f, app, &th, chunks[3]);
    // toasts sit under the overlays, the tour over everything
    render_toasts(f, app, &th, chunks[2]);
    if let Some(ap) = &app.appearance {
        render_appearance(f, app, ap, &th, area);
    } else if let Some(pick) = &app.provider_pick {
        render_provider_picker(f, app, pick, &th, area);
    } else if let Some(ag) = &app.agent {
        render_agent(f, app, ag, &th, area);
    } else if let Some(cur) = app.flags_view {
        render_flags(f, app, cur, &th, area);
    } else if let Some(cur) = app.challenge_view {
        render_challenges(f, app, cur, &th, area);
    } else if let Some(cur) = app.recipe_view {
        render_recipes(f, app, cur, &th, area);
    } else if let Some(pui) = &app.project_ui {
        render_project_ui(f, app, pui, &th, area);
    } else if app.show_help {
        render_help(f, &th, area);
    } else if let Some(cur) = app.lan_panel {
        render_lan_panel(f, app, cur, &th, area);
    }
    // a guest asking to join comes over the dashboard, under the tour
    if let Some(p) = &app.join_prompt {
        render_join_prompt(f, app, p, &th, area);
    }
    if let Some(p) = &app.first_run {
        render_first_run(f, &th, area, &p.names);
    }
    if let Some(step) = app.tour {
        render_tour(f, &th, area, step);
    }
}

/// One titled section of the `?` overlay.
pub struct HelpGroup {
    /// shown before the heading when symbol icons are on
    pub icon: &'static str,
    pub title: &'static str,
    /// (keys, what they do); add a new key as a row in its group
    pub rows: &'static [(&'static str, &'static str)],
}

/// Every key binding, grouped, shown by `?`.
pub const HELP: &[HelpGroup] = &[
    HelpGroup {
        icon: "◆",
        title: "Navigate",
        rows: &[
            ("↑ ↓  j k", "move in the tool list"),
            (
        "← →  [ ]",
        "switch category: All · Installed · OSINT … Wordlists",
    ),
            ("/", "filter tools by name / category / description"),
        ],
    },
    HelpGroup {
        icon: "▶",
        title: "Run & terminals",
        rows: &[
            (
        "Enter",
        "run the tool: pick a recipe (if it has some), edit the args, run · dataset: list it",
    ),
            (
        "0-4  Tab",
        "show the console (0) or terminal 1-4 · Tab cycles",
    ),
            ("g", "grid: all four terminals at once"),
            ("s", "send a line to the shown tool's input · Ctrl+D = EOF"),
            (
        "S",
        "run the tool in Windows Sandbox: sample read-only, network off (Tab)",
    ),
            ("c", "stop the shown tool (its output stays)"),
            (
        "x",
        "close the shown terminal (twice if its tool is running)",
    ),
        ],
    },
    HelpGroup {
        icon: "≡",
        title: "Output",
        rows: &[
            ("PgUp PgDn", "scroll the terminal · Home top · End follow"),
            ("f  n  N", "find in the output · older / newer match"),
            ("y", "copy the shown terminal's output (only the find matches, after f)"),
        ],
    },
    HelpGroup {
        icon: "⚑",
        title: "CTF",
        rows: &[
            (
        "C",
        "challenge: new / switch · target, flag format, files, notes (%USERPROFILE%\\ctf)",
    ),
            (
        "F",
        "flags caught in tool output: list, Enter copies (each is also copied and saved)",
    ),
            ("h", "identify a hash (built in)"),
            ("d", "working folder (where tools run), like cd"),
        ],
    },
    HelpGroup {
        icon: "✚",
        title: "Tools",
        rows: &[
            (
        "i",
        "install the selected tool (scoop / pipx), in the console",
    ),
            ("r", "re-detect installed tools (picks up PATH changes)"),
            (
                "p",
                "projects: open/create a working folder — tools run there ({proj} = its path)",
            ),
        ],
    },
    HelpGroup {
        icon: "✦",
        title: "Agent",
        rows: &[
            ("a", "agent: AI reads the output and suggests next steps"),
            (
                "A",
                "choose the agent's AI (claude / gemini / deepseek / openai / ollama)",
            ),
        ],
    },
    HelpGroup {
        icon: "☍",
        title: "Web / LAN",
        rows: &[
            (
        "w  W",
        "web view: this session in a browser (W: on your LAN)",
    ),
            ("o", "open the web view in your browser"),
            (
        "L",
        "LAN guests: join key · let in (v watch, f full) · x remove · n new key",
    ),
        ],
    },
    HelpGroup {
        icon: "⚙",
        title: "App",
        rows: &[
            ("t", "appearance: theme, borders, glyphs, overlays"),
            ("?", "this help"),
            ("q  Esc", "quit (twice if tools are still running)"),
        ],
    },
];

/// The "in prompts" line kept under the groups.
pub const HELP_PROMPTS: (&str, &str) = (
    "in prompts",
    "← → Home End · Ctrl+← → words · Ctrl+W / Ctrl+U delete · ↑ ↓ history",
);

/// Every (keys, description) row, flattened.
pub fn help_rows() -> impl Iterator<Item = &'static (&'static str, &'static str)> {
    HELP.iter()
        .flat_map(|g| g.rows.iter())
        .chain(std::iter::once(&HELP_PROMPTS))
}

// ------------------------------------------------------------------- mouse

/// Remember that `r` is `h`, for the mouse (`App::on_mouse`). Later calls
/// sit on top of earlier ones.
fn hit(app: &App, r: Rect, h: Hit) {
    if !r.is_empty() {
        app.hits.borrow_mut().push((r, h));
    }
}

/// Line `line` of `inner` (the inside of a box) is `h`, if it's on screen.
fn hit_line(app: &App, inner: Rect, line: usize, h: Hit) {
    let y = inner.y as usize + line;
    if y < inner.bottom() as usize {
        let r = Rect {
            y: y as u16,
            height: 1,
            ..inner
        };
        hit(app, r, h);
    }
}

/// The inside of a bordered box.
fn inside(r: Rect) -> Rect {
    r.inner(Margin::new(1, 1))
}

/// One line of spans, some of them clickable.
#[derive(Default)]
struct HitLine {
    spans: Vec<Span<'static>>,
    /// (first column, width, what it is)
    hits: Vec<(usize, usize, Hit)>,
    /// columns used so far
    x: usize,
}

impl HitLine {
    fn push(&mut self, s: Span<'static>) {
        self.x += s.width();
        self.spans.push(s);
    }

    /// Everything pushed since column `from` is `h`.
    fn hit_since(&mut self, from: usize, h: Hit) {
        if self.x > from {
            self.hits.push((from, self.x - from, h));
        }
    }

    fn push_hit(&mut self, s: Span<'static>, h: Hit) {
        let from = self.x;
        self.push(s);
        self.hit_since(from, h);
    }

    /// Draw it at `area`, recording the clickable parts that fit.
    fn render(self, f: &mut Frame, app: &App, area: Rect) {
        for &(from, w, h) in &self.hits {
            let x = area.x as usize + from;
            if x >= area.right() as usize {
                continue;
            }
            let w = w.min(area.right() as usize - x);
            let r = Rect {
                x: x as u16,
                y: area.y,
                width: w as u16,
                height: 1,
            };
            hit(app, r, h);
        }
        f.render_widget(Paragraph::new(Line::from(self.spans)), area);
    }
}

// ---------------------------------------------------------------- overlays

/// Where an overlay of `w` x `h` goes: centered, or docked full-width just
/// above the footer the way Claude Code shows its pickers. Also fades the
/// dashboard behind it when the backdrop is set to dim.
fn overlay_area(f: &mut Frame, th: &Theme, area: Rect, w: u16, h: u16) -> Rect {
    let h = h.min(area.height);
    let popup = match th.overlay {
        OverlayPos::Centered => centered(area, w, h),
        OverlayPos::Docked => {
            // keep the footer row visible; leave the header if there's room
            let h = h.min(area.height.saturating_sub(1).max(1));
            Rect {
                x: area.x,
                y: (area.y + area.height).saturating_sub(1 + h).max(area.y),
                width: area.width,
                height: h,
            }
        }
    };
    dim_backdrop(f, th, area);
    f.render_widget(Clear, popup);
    popup
}

/// A `w` x `h` box in the middle of `area` (clamped to fit).
fn centered(area: Rect, w: u16, h: u16) -> Rect {
    let w = w.min(area.width);
    let h = h.min(area.height);
    Rect {
        x: area.x + area.width.saturating_sub(w) / 2,
        y: area.y + area.height.saturating_sub(h) / 2,
        width: w,
        height: h,
    }
}

/// Fade everything already drawn, if the backdrop setting asks for it.
fn dim_backdrop(f: &mut Frame, th: &Theme, area: Rect) {
    if th.backdrop != Backdrop::Dim {
        return;
    }
    let buf = f.buffer_mut();
    for y in area.top()..area.bottom() {
        for x in area.left()..area.right() {
            let cell = &mut buf[(x, y)];
            cell.set_fg(th.pal.dim);
            cell.set_bg(Color::Reset);
            cell.modifier = Modifier::DIM;
        }
    }
}

/// A solid label (prompt names, a pending confirm). Reverse video when
/// colours are stripped, or the background would vanish.
fn pill(th: &Theme, bg: Color) -> Style {
    if th.mono {
        Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
    } else {
        th.emph(Style::default().fg(th.pal.ink).bg(bg))
    }
}

/// The frame every overlay shares: accent border, bold title, hint bottom right.
fn overlay_block<'a>(th: &Theme, title: &'a str, hint: &'a str) -> Block<'a> {
    th.block(th.pal.accent)
        .title(Span::styled(format!(" {title} "), th.strong(th.pal.accent)))
        .title_bottom(
            Line::from(Span::styled(format!(" {hint} "), th.fg(th.pal.dim)))
                .alignment(Alignment::Right),
        )
}

/// "X wants to join": watch only / full control / deny. The choices stay
/// dim for the first moment, while keys are ignored.
fn render_join_prompt(f: &mut Frame, app: &App, p: &JoinPrompt, th: &Theme, area: Rect) {
    let pal = &th.pal;
    let g = &p.guest;
    let ready = p.since.elapsed() >= JOIN_GRACE;
    let who = if g.name.is_empty() {
        "Someone".to_string()
    } else {
        format!("\"{}\"", g.name)
    };
    let mut lines = vec![
        Line::raw(""),
        Line::from(Span::styled(
            format!("  {who} wants to join this session"),
            th.strong(pal.fg),
        )),
        Line::from(Span::styled(
            format!("  {} · {} · used your join key", g.ip, g.agent),
            th.fg(pal.dim),
        )),
        Line::raw(""),
    ];
    let first = lines.len();
    for (i, (label, desc)) in JOIN_CHOICES.iter().enumerate() {
        let on = i == p.cursor;
        let pointer = if on { th.glyph.pointer } else { "  " };
        let style = if !ready {
            th.fg(pal.dim)
        } else if on {
            th.strong(if i == 2 { pal.err } else { pal.accent })
        } else {
            th.fg(pal.fg)
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {pointer}{} ", i + 1), style),
            Span::styled(format!("{label:<14}"), style),
            Span::styled(desc.to_string(), th.fg(pal.dim)),
        ]));
    }
    lines.push(Line::raw(""));
    let w = lines.iter().map(|l| l.width()).max().unwrap_or(40) as u16 + 4;
    let h = lines.len() as u16 + 2;
    let popup = overlay_area(f, th, area, w, h);
    hit(app, popup, Hit::Popup(Layer::Join));
    for i in 0..JOIN_CHOICES.len() {
        hit_line(app, inside(popup), first + i, Hit::Row(Layer::Join, i));
    }
    f.render_widget(
        Paragraph::new(lines).block(overlay_block(
            th,
            "LAN guest",
            "1-3 or ↑↓ Enter · Esc decide later",
        )),
        popup,
    );
}

/// `L`: the LAN view's address, certificate, join key and guests.
fn render_lan_panel(f: &mut Frame, app: &App, cur: usize, th: &Theme, area: Rect) {
    let pal = &th.pal;
    let label = |s: &str| Span::styled(format!("  {s:<13}"), th.fg(pal.dim));
    let mut lines = vec![Line::raw("")];
    lines.push(Line::from(vec![
        label("address"),
        Span::styled(
            app.lan_url
                .clone()
                .unwrap_or_else(|| "no LAN address found".into()),
            th.strong(pal.accent),
        ),
    ]));
    if let Some(fp) = app.web.as_ref().and_then(|w| w.fingerprint()) {
        lines.push(Line::from(vec![
            label("certificate"),
            Span::styled(
                format!("SHA-256 {}…", &fp[..fp.len().min(47)]),
                th.fg(pal.fg),
            ),
        ]));
    }
    let mut key = vec![label("join key")];
    match &app.lan_key {
        Some(k) if k.state == KeyState::Live => {
            let left = k
                .expires
                .saturating_duration_since(Instant::now())
                .as_secs();
            key.push(Span::styled(k.code.clone(), pill(th, pal.accent)));
            key.push(Span::styled(
                format!("  one use · {}:{:02} left", left / 60, left % 60),
                th.fg(pal.dim),
            ));
        }
        Some(k) => {
            key.push(Span::styled(
                format!("{} ({})", k.code, k.state.label()),
                th.fg(pal.dim),
            ));
            key.push(Span::styled("  n makes a new one", th.fg(pal.warn)));
        }
        None => key.push(Span::styled("none", th.fg(pal.dim))),
    }
    lines.push(Line::from(key));
    let mut fw = vec![label("firewall")];
    match app.fw_port {
        Some(p) => {
            fw.push(Span::styled(
                format!("{} open", crate::firewall::rule_name(p)),
                th.fg(pal.ok),
            ));
            fw.push(Span::styled("  c closes it", th.fg(pal.dim)));
        }
        None if app.fw_note.is_empty() => fw.push(Span::styled("no rule open", th.fg(pal.dim))),
        None => fw.push(Span::styled(
            "not managed (see the console)",
            th.fg(pal.warn),
        )),
    }
    lines.push(Line::from(fw));
    lines.push(Line::raw(""));

    let guests = &app.lan_guests;
    lines.push(Line::from(Span::styled(
        format!("  guests ({})", guests.len()),
        th.strong(pal.fg),
    )));
    if guests.is_empty() {
        lines.push(Line::from(Span::styled(
            "    nobody yet: give someone the address and the key",
            th.fg(pal.dim),
        )));
    }
    let name_w = guests
        .iter()
        .map(|g| g.name.chars().count())
        .max()
        .unwrap_or(0)
        .max(4);
    let cur = cur.min(guests.len().saturating_sub(1));
    let first = lines.len();
    for (i, g) in guests.iter().enumerate() {
        let on = i == cur;
        let pointer = if on { th.glyph.pointer } else { "  " };
        let name = if g.name.is_empty() { "-" } else { &g.name };
        let (status, color) = match g.status {
            GuestStatus::Waiting => ("waiting".to_string(), pal.warn),
            GuestStatus::In(r) => (r.label().to_string(), pal.ok),
            _ => ("gone".to_string(), pal.dim),
        };
        let online = match (g.status, g.online) {
            (GuestStatus::In(_), 0) => " · offline",
            (GuestStatus::In(_), _) => " · online",
            _ => "",
        };
        lines.push(Line::from(vec![
            Span::styled(format!("  {pointer}"), th.fg(pal.accent)),
            Span::styled(
                format!("{name:<name_w$}  "),
                if on { th.strong(pal.fg) } else { th.fg(pal.fg) },
            ),
            Span::styled(format!("{:<15} ", g.ip.to_string()), th.fg(pal.dim)),
            Span::styled(format!("{:<20} ", g.agent), th.fg(pal.dim)),
            Span::styled(status, th.strong(color)),
            Span::styled(online, th.fg(pal.dim)),
        ]));
    }
    lines.push(Line::raw(""));
    let w = lines.iter().map(|l| l.width()).max().unwrap_or(40) as u16 + 4;
    let h = lines.len() as u16 + 2;
    let popup = overlay_area(f, th, area, w.max(64), h);
    hit(app, popup, Hit::Popup(Layer::Lan));
    for i in 0..guests.len() {
        hit_line(app, inside(popup), first + i, Hit::Row(Layer::Lan, i));
    }
    f.render_widget(
        Paragraph::new(lines).block(overlay_block(
            th,
            "LAN view",
            "↑↓ pick · v watch · f full · x remove · n new key · Esc close",
        )),
        popup,
    );
}

/// Word-wrap `text` to `width` columns (at least one word per line).
fn wrap_words(text: &str, width: usize) -> Vec<String> {
    let mut lines: Vec<String> = Vec::new();
    for word in text.split_whitespace() {
        match lines.last_mut() {
            Some(l) if l.chars().count() + 1 + word.chars().count() <= width => {
                l.push(' ');
                l.push_str(word);
            }
            _ => lines.push(word.to_string()),
        }
    }
    lines
}

/// The `?` lines for one group, wrapped to `width`.
fn help_group_lines(th: &Theme, g: &HelpGroup, key_w: usize, width: usize) -> Vec<Line<'static>> {
    let p = &th.pal;
    let title = if th.glyph.icons {
        format!(" {} {} ", g.icon, g.title)
    } else {
        format!(" {} ", g.title)
    };
    // a pill (reverse video under NO_COLOR) so headings never rely on color alone
    let head = if th.mono {
        th.selected()
    } else {
        th.strong(p.accent).add_modifier(Modifier::UNDERLINED)
    };
    let mut lines = vec![Line::from(Span::styled(title, head))];
    let desc_w = width.saturating_sub(key_w + 3).max(12);
    for (k, d) in g.rows {
        for (i, part) in wrap_words(d, desc_w).into_iter().enumerate() {
            let key = if i == 0 { k.to_string() } else { String::new() };
            lines.push(Line::from(vec![
                Span::styled(format!(" {key:<key_w$}  "), th.strong(p.accent)),
                Span::styled(part, th.fg(p.fg)),
            ]));
        }
    }
    lines
}

fn render_help(f: &mut Frame, th: &Theme, area: Rect) {
    let p = &th.pal;
    let key_w = help_rows()
        .map(|(k, _)| k.chars().count())
        .max()
        .unwrap_or(0);
    let avail = area.width.saturating_sub(4) as usize;
    let two = avail >= 110;
    let col_w = if two {
        (avail - 2) / 2
    } else {
        avail.clamp(30, 100)
    };
    let blocks: Vec<Vec<Line>> = HELP
        .iter()
        .map(|g| help_group_lines(th, g, key_w, col_w))
        .collect();
    let mut lines: Vec<Line> = Vec::new();
    if two {
        // fill the left column until it holds about half the lines
        let total: usize = blocks.iter().map(|b| b.len() + 1).sum();
        let (mut left, mut right): (Vec<Line>, Vec<Line>) = (Vec::new(), Vec::new());
        for b in blocks {
            let col = if left.len() * 2 < total {
                &mut left
            } else {
                &mut right
            };
            if !col.is_empty() {
                col.push(Line::raw(""));
            }
            col.extend(b);
        }
        for i in 0..left.len().max(right.len()) {
            let mut spans = left.get(i).map(|l| l.spans.clone()).unwrap_or_default();
            let used: usize = spans.iter().map(|s| s.content.chars().count()).sum();
            spans.push(Span::raw(" ".repeat((col_w + 2).saturating_sub(used))));
            if let Some(r) = right.get(i) {
                spans.extend(r.spans.clone());
            }
            lines.push(Line::from(spans));
        }
    } else {
        for b in blocks {
            if !lines.is_empty() {
                lines.push(Line::raw(""));
            }
            lines.extend(b);
        }
    }
    lines.push(Line::raw(""));
    let (pk, pd) = HELP_PROMPTS;
    lines.push(Line::from(vec![
        Span::styled(format!(" {pk}  "), th.strong(p.accent)),
        Span::styled(pd.to_string(), th.fg(p.dim)),
    ]));
    let w = lines.iter().map(|l| l.width()).max().unwrap_or(40) as u16 + 3;
    let h = lines.len() as u16 + 2;
    let popup = overlay_area(f, th, area, w, h);
    f.render_widget(
        Paragraph::new(lines).block(overlay_block(th, "keys", "any key to close")),
        popup,
    );
}

/// The agent-provider picker (`A`): choose which AI the `a` key talks to, and
/// set a model override per provider (`m` edits, `r` resets to default).
fn render_provider_picker(f: &mut Frame, app: &App, pick: &ProviderPick, th: &Theme, area: Rect) {
    let p = &th.pal;
    let current = app.settings.agent;
    let label_w = crate::agent::Provider::ALL
        .iter()
        .map(|x| x.label().chars().count())
        .max()
        .unwrap_or(0);

    let mut lines: Vec<Line> = vec![Line::raw("")];
    for (i, prov) in crate::agent::Provider::ALL.iter().enumerate() {
        let sel = i == pick.cursor;
        let is_current = *prov == current;
        let marker = if sel { "›" } else { " " };
        let dot = if is_current { "●" } else { " " };
        let ready = matches!(prov.availability(), crate::agent::Availability::Ready);
        let note_color = if ready { p.ok } else { p.dim };
        let name_style = if sel {
            th.strong(p.accent)
        } else if is_current {
            th.fg(p.fg)
        } else {
            th.fg(p.dim)
        };
        let model = app.effective_model(*prov);
        lines.push(Line::from(vec![
            Span::styled(format!(" {marker} "), th.strong(p.accent)),
            Span::styled(format!("{dot} "), th.fg(p.accent)),
            Span::styled(format!("{:<label_w$}  ", prov.label()), name_style),
            Span::styled(prov.status_note(), th.fg(note_color)),
        ]));
        // model line under each provider
        if let (true, Some(ed)) = (sel, pick.editing.as_ref()) {
            lines.push(Line::from(vec![
                Span::styled("      model: ", th.fg(p.dim)),
                Span::styled(ed.text().to_string(), th.strong(p.accent)),
                Span::styled("▏", th.strong(p.accent)),
            ]));
        } else {
            lines.push(Line::from(vec![
                Span::styled("      model: ", th.fg(p.dim)),
                Span::styled(model, th.fg(if sel { p.fg } else { p.dim })),
            ]));
        }
    }
    lines.push(Line::raw(""));
    let hint = if pick.editing.is_some() {
        "  type the model · Enter save · Esc cancel · empty = default"
    } else {
        "  ● current · m edit model · r reset · Claude uses its CLI login, the rest read an API key"
    };
    lines.push(Line::from(Span::styled(hint, th.fg(p.dim))));

    let w = (lines.iter().map(|l| l.width()).max().unwrap_or(40) as u16 + 4).min(area.width);
    let h = (lines.len() as u16 + 2).min(area.height);
    let popup = overlay_area(f, th, area, w, h);
    hit(app, popup, Hit::Popup(Layer::Provider));
    // each provider takes two lines (name, model), after one blank
    for i in 0..crate::agent::Provider::ALL.len() {
        for line in [1 + 2 * i, 2 + 2 * i] {
            hit_line(app, inside(popup), line, Hit::Row(Layer::Provider, i));
        }
    }
    let bottom = if pick.editing.is_some() {
        "Enter save · Esc cancel"
    } else {
        "↑↓ · Enter choose · m model · Esc"
    };
    f.render_widget(
        Paragraph::new(lines).block(overlay_block(th, "agent provider", bottom)),
        popup,
    );
}

/// The agent overlay (`a`): the AI's summary, findings, and a cursor over the
/// suggested next steps.
fn render_agent(f: &mut Frame, app: &App, ag: &AgentView, th: &Theme, area: Rect) {
    let p = &th.pal;
    let a = &ag.analysis;
    let mut lines: Vec<Line> = Vec::new();

    if a.summary.trim().is_empty() {
        lines.push(Line::from(Span::styled("(no summary)", th.fg(p.dim))));
    } else {
        lines.push(Line::from(Span::styled(a.summary.clone(), th.fg(p.fg))));
    }

    if !a.findings.is_empty() {
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled("findings", th.strong(p.dim))));
        for fdg in &a.findings {
            lines.push(Line::from(vec![
                Span::styled(" • ", th.fg(p.accent)),
                Span::styled(fdg.clone(), th.fg(p.fg)),
            ]));
        }
    }

    lines.push(Line::raw(""));
    if a.suggestions.is_empty() {
        lines.push(Line::from(Span::styled(
            "no tool suggestions — press Esc to close",
            th.fg(p.dim),
        )));
    } else {
        lines.push(Line::from(Span::styled(
            "suggested next steps",
            th.strong(p.accent),
        )));
        for (i, s) in a.suggestions.iter().enumerate() {
            let sel = i == ag.cursor;
            let name_style = if sel {
                th.strong(p.accent)
            } else {
                th.fg(p.fg)
            };
            lines.push(Line::from(vec![
                Span::styled(if sel { " › " } else { "   " }, th.strong(p.accent)),
                Span::styled(format!("{} ", s.tool), name_style),
                Span::styled(s.args.clone(), th.fg(p.dim)),
            ]));
            if !s.rationale.trim().is_empty() {
                lines.push(Line::from(vec![
                    Span::raw("     "),
                    Span::styled(s.rationale.clone(), th.fg(p.dim)),
                ]));
            }
        }
    }

    let w = 78.min(area.width);
    // wrapping can grow lines, so leave headroom rather than clip tightly
    let h = (lines.len() as u16 + 6).min(area.height);
    let popup = overlay_area(f, th, area, w, h);
    hit(app, popup, Hit::Popup(Layer::Agent));
    f.render_widget(
        Paragraph::new(lines)
            .wrap(Wrap { trim: true })
            .block(overlay_block(
                th,
                "agent",
                "↑↓ pick · Enter run · Esc close",
            )),
        popup,
    );
}

/// The project manager overlay (`p`): New / Open / Recent, a folder browser,
/// and a name prompt for a new project.
fn render_project_ui(f: &mut Frame, app: &App, ui: &ProjectUi, th: &Theme, area: Rect) {
    let p = &th.pal;
    let mut lines: Vec<Line> = vec![Line::raw("")];
    let (title, hint): (&str, &str) = match ui {
        ProjectUi::Menu { cursor } => {
            let recents = app.pstore.existing_recents();
            let mut rows: Vec<(String, Option<String>)> = vec![
                ("＋  New project…".into(), None),
                ("📂  Open a folder…".into(), None),
            ];
            for r in &recents {
                let name = r
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_else(|| r.display().to_string());
                rows.push((format!("•  {name}"), Some(r.display().to_string())));
            }
            for (i, (label, sub)) in rows.iter().enumerate() {
                let sel = i == *cursor;
                lines.push(Line::from(vec![
                    Span::styled(if sel { " › " } else { "   " }, th.strong(p.accent)),
                    Span::styled(
                        label.clone(),
                        if sel { th.strong(p.accent) } else { th.fg(p.fg) },
                    ),
                ]));
                if let Some(path) = sub {
                    lines.push(Line::from(Span::styled(format!("      {path}"), th.fg(p.dim))));
                }
            }
            if recents.is_empty() {
                lines.push(Line::raw(""));
                lines.push(Line::from(Span::styled(
                    "   no recent projects yet",
                    th.fg(p.dim),
                )));
            }
            ("project", "↑↓ · Enter · Esc close")
        }
        ProjectUi::Browse {
            dir,
            entries,
            cursor,
            new,
        } => {
            lines.push(Line::from(Span::styled(
                format!(" {}", dir.display()),
                th.fg(p.dim),
            )));
            lines.push(Line::raw(""));
            // row 0 = parent
            let up_sel = *cursor == 0;
            lines.push(Line::from(vec![
                Span::styled(if up_sel { " › " } else { "   " }, th.strong(p.accent)),
                Span::styled("..  (up)", if up_sel { th.strong(p.accent) } else { th.fg(p.dim) }),
            ]));
            // show a window of entries around the cursor so long lists fit
            let total = entries.len();
            let max_rows = (area.height as usize).saturating_sub(10).max(5);
            let start = (cursor.saturating_sub(1))
                .saturating_sub(max_rows / 2)
                .min(total.saturating_sub(max_rows));
            for (i, e) in entries.iter().enumerate().skip(start).take(max_rows) {
                let sel = i + 1 == *cursor;
                let name = e
                    .file_name()
                    .map(|n| n.to_string_lossy().into_owned())
                    .unwrap_or_default();
                lines.push(Line::from(vec![
                    Span::styled(if sel { " › " } else { "   " }, th.strong(p.accent)),
                    Span::styled(
                        format!("{name}/"),
                        if sel { th.strong(p.accent) } else { th.fg(p.fg) },
                    ),
                ]));
            }
            lines.push(Line::raw(""));
            let action = if *new {
                "o = create a new project in THIS folder"
            } else {
                "o = open THIS folder as the project"
            };
            lines.push(Line::from(Span::styled(format!("   {action}"), th.fg(p.accent))));
            ("choose a folder", "↑↓ move · Enter open dir · o select · Esc back")
        }
        ProjectUi::Name { parent, editor } => {
            lines.push(Line::from(Span::styled(
                format!(" new project in {}", parent.display()),
                th.fg(p.dim),
            )));
            lines.push(Line::raw(""));
            lines.push(Line::from(vec![
                Span::styled("   name: ", th.fg(p.dim)),
                Span::styled(editor.text().to_string(), th.strong(p.accent)),
                Span::styled("▏", th.strong(p.accent)),
            ]));
            ("name the project", "Enter create · Esc back")
        }
    };

    let w = 72.min(area.width);
    let h = (lines.len() as u16 + 2).min(area.height);
    let popup = overlay_area(f, th, area, w, h);
    f.render_widget(
        Paragraph::new(lines).block(overlay_block(th, title, hint)),
        popup,
    );
}

/// The `t` overlay: settings on the left, a live sample of the dashboard
/// on the right (stacked when narrow). Everything, including this panel,
/// is already drawn with the previewed settings.
fn render_appearance(f: &mut Frame, app: &App, ap: &Appearance, th: &Theme, area: Rect) {
    let p = &th.pal;
    let s = &app.settings;
    let g = th.glyph;

    let label_w = Field::ALL
        .iter()
        .map(|x| x.label().chars().count())
        .max()
        .unwrap_or(0);
    let mut rows: Vec<Line> = vec![Line::raw("")];
    for (i, field) in Field::ALL.iter().enumerate() {
        let on = i == ap.cursor;
        let cursor = if on { g.pointer } else { "  " };
        let value = s.value(*field);
        let mut spans = vec![
            Span::styled(format!(" {cursor}"), th.fg(p.accent)),
            Span::styled(
                format!("{:<label_w$}  ", field.label()),
                if on { th.strong(p.fg) } else { th.fg(p.dim) },
            ),
        ];
        if on {
            spans.push(Span::styled(format!("{} ", g.left), th.fg(p.accent)));
            spans.push(Span::styled(value, th.strong(p.accent)));
            spans.push(Span::styled(format!(" {}", g.right), th.fg(p.accent)));
        } else {
            spans.push(Span::styled(format!("  {value}"), th.fg(p.fg)));
        }
        rows.push(Line::from(spans));
    }
    rows.push(Line::raw(""));
    // the theme list, numbered like Claude Code's /theme
    for (i, t) in ThemeName::ALL.iter().enumerate() {
        let cur = *t == s.theme;
        rows.push(Line::from(vec![
            Span::styled(
                format!("   {} ", i + 1),
                if cur {
                    th.strong(p.accent)
                } else {
                    th.fg(p.dim)
                },
            ),
            Span::styled(
                format!("{}{}", t.label(), if cur { "  ✓" } else { "" }),
                if cur { th.fg(p.fg) } else { th.fg(p.dim) },
            ),
        ]));
    }

    let preview = preview_lines(th);
    let left_w = rows.iter().map(|l| l.width()).max().unwrap_or(30) as u16 + 2;
    let right_w = preview.iter().map(|l| l.width()).max().unwrap_or(30) as u16 + 4;
    let side_by_side = area.width >= left_w + right_w + 4;
    let w = if side_by_side {
        left_w + right_w + 3
    } else {
        left_w.max(right_w) + 2
    };
    let h = if side_by_side {
        rows.len().max(preview.len() + 2) as u16 + 2
    } else {
        (rows.len() + preview.len() + 2) as u16 + 2
    };
    let popup = overlay_area(f, th, area, w, h);
    let block = overlay_block(
        th,
        "appearance",
        "↑↓ choose · ←→ change · 1-8 theme · Enter save · Esc cancel",
    );
    let inner = block.inner(popup);
    f.render_widget(block, popup);

    let parts = Layout::default()
        .direction(if side_by_side {
            Direction::Horizontal
        } else {
            Direction::Vertical
        })
        .constraints([
            Constraint::Length(if side_by_side {
                left_w
            } else {
                rows.len() as u16
            }),
            Constraint::Min(0),
        ])
        .split(inner);
    f.render_widget(Paragraph::new(rows), parts[0]);
    // after one blank line: the settings, a blank line, then the themes
    // (rows past the settings count on as themes)
    hit(app, popup, Hit::Popup(Layer::Appearance));
    let fields = Field::ALL.len();
    for i in 0..fields {
        hit_line(app, parts[0], 1 + i, Hit::Row(Layer::Appearance, i));
    }
    for j in 0..ThemeName::ALL.len() {
        let row = Hit::Row(Layer::Appearance, fields + j);
        hit_line(app, parts[0], fields + 2 + j, row);
    }

    let sample = th
        .block(p.border)
        .title(Span::styled(" preview ", th.fg(p.dim)));
    let sample_area = if side_by_side {
        Rect {
            x: parts[1].x + 1,
            y: parts[1].y + 1,
            width: right_w.min(parts[1].width.saturating_sub(2)),
            height: (preview.len() as u16 + 2).min(parts[1].height.saturating_sub(1)),
        }
    } else {
        parts[1]
    };
    f.render_widget(Paragraph::new(preview).block(sample), sample_area);
}

/// A few rows that exercise every palette slot.
fn preview_lines(th: &Theme) -> Vec<Line<'static>> {
    let p = &th.pal;
    let g = th.glyph;
    let tool = |icon: &'static str, c, name: &'static str, cat: &'static str| {
        Line::from(vec![
            Span::styled(format!("{icon} "), th.fg(c)),
            Span::styled(format!("{name:<9}"), th.strong(p.fg)),
            Span::styled(format!(" {cat}"), th.fg(p.dim)),
        ])
    };
    vec![
        Line::from(vec![
            Span::styled(g.pointer, th.selected()),
            Span::styled("nmap      recon    ", th.selected()),
        ]),
        tool(g.installed, p.ok, "ffuf", "web"),
        tool(g.missing, p.dim, "sqlmap", "web"),
        tool(g.conflict, p.warn, "john", "cracking"),
        Line::raw(""),
        Line::from(Span::styled(
            "$ nmap -sV scanme.nmap.org",
            line_style(th, "$ "),
        )),
        Line::from(Span::styled("22/tcp open  ssh", th.fg(p.fg))),
        Line::from(vec![
            Span::styled("80/tcp ", th.fg(p.fg)),
            Span::styled("open", Style::default().fg(p.fg).bg(p.hit_bg)),
            Span::styled("  http", th.fg(p.fg)),
        ]),
        Line::from(Span::styled("> y", line_style(th, "> "))),
        Line::from(Span::styled(
            "[err] host seems down",
            line_style(th, "[err]"),
        )),
        Line::from(Span::styled(":: exit 0 · 4.2s", line_style(th, "::"))),
        Line::from(vec![
            Span::styled(format!("{} running ", g.spinner[0]), th.strong(p.warn)),
            Span::styled("pid 4242", th.fg(p.dim)),
        ]),
    ]
}

// --------------------------------------------------------------- dashboard

fn render_header(f: &mut Frame, app: &App, th: &Theme, area: Rect) {
    let p = &th.pal;
    // "● web :7878 · 1 viewer │" in front of the counts while it's on
    let web = app.web.as_ref().map(|w| {
        let who = match w.viewers() {
            0 => "no viewers".to_string(),
            1 => "1 viewer".to_string(),
            n => format!("{n} viewers"),
        };
        let place = if w.lan() { "lan" } else { "web" };
        // LAN: the live join key (this header is never mirrored to the web)
        // and anyone waiting
        let mut lan = String::new();
        if w.lan() {
            match &app.lan_key {
                Some(k) if k.state == KeyState::Live => lan.push_str(&format!(" · key {}", k.code)),
                _ => lan.push_str(" · no key (L)"),
            }
            let waiting = app
                .lan_guests
                .iter()
                .filter(|g| g.status == GuestStatus::Waiting)
                .count();
            if waiting > 0 {
                lan.push_str(&format!(" · {waiting} waiting (L)"));
            }
        }
        (
            format!("{place} :{}{lan} · {who} ", w.port()),
            w.viewers() > 0,
        )
    });
    let web_w = web
        .as_ref()
        .map_or(0, |(t, _)| t.chars().count() as u16 + 4);
    let cols = Layout::default()
        .direction(Direction::Horizontal)
        .constraints([
            Constraint::Min(10),
            Constraint::Length(10),         // agent icon
            Constraint::Length(52 + web_w), // install stats (+ web indicator)
        ])
        .split(area);

    let ctx = app
        .challenge
        .as_ref()
        .map(|w| (w.title(), w.info.target.clone()));
    // the name always shows; with a challenge, its title too
    let keep = if ctx.is_some() { 3 } else { 1 };
    // wordmark (+ challenge), plus the open project's name like VS Code's folder
    let mut left = banner::wordmark(th, ctx);
    if let Some(proj) = &app.project {
        left.spans
            .push(Span::styled(format!("  ▪ {}", proj.name), th.strong(p.accent)));
    }
    f.render_widget(
        Paragraph::new(banner::fit(left, cols[0].width, keep)),
        cols[0],
    );

    // Clickable agent icon: a filled chip, a braille spinner while it works.
    const SPIN: [char; 10] = ['⠋', '⠙', '⠹', '⠸', '⠼', '⠴', '⠦', '⠧', '⠇', '⠏'];
    let mark = if app.agent_busy {
        SPIN[(app.tick as usize / 2) % SPIN.len()]
    } else {
        '✦'
    };
    let chip = Line::from(Span::styled(
        format!(" {mark} agent "),
        th.strong(p.accent).add_modifier(Modifier::REVERSED),
    ))
    .alignment(Alignment::Right);
    f.render_widget(Paragraph::new(chip), cols[1]);
    app.agent_icon.set(Some(cols[1]));

    let count = |st: ToolStatus| app.status.iter().filter(|s| **s == st).count();
    let installed = count(ToolStatus::Installed);
    let missing = count(ToolStatus::Missing);

    let mut stats = Vec::new();
    if let Some((text, live)) = web {
        stats.push(Span::styled(
            th.glyph.installed,
            th.fg(if live { p.ok } else { p.dim }),
        ));
        stats.push(Span::styled(format!(" {text}"), th.fg(p.accent)));
        stats.push(Span::styled("│ ", th.fg(p.border)));
    }
    let running = app.running_count();
    if running > 0 {
        let spin = th.glyph.spinner;
        let frame = spin[(app.tick / 2 % spin.len() as u64) as usize];
        stats.push(Span::styled(
            format!("{frame} {running} running  "),
            th.strong(p.warn),
        ));
    }
    // labelled, so nobody has to guess what the dots mean
    stats.extend([
        Span::styled(th.glyph.installed, th.fg(p.ok)),
        Span::styled(format!(" {installed} ready  "), th.fg(p.fg)),
        Span::styled(th.glyph.missing, th.fg(p.dim)),
        Span::styled(format!(" {missing} to get "), th.fg(p.fg)),
        Span::styled(format!("· {} tools ", app.tools.len()), th.fg(p.dim)),
    ]);
    let stats = Line::from(stats).alignment(Alignment::Right);
    f.render_widget(Paragraph::new(stats), cols[2]);
}

/// The monochrome symbol in front of a category chip (unicode glyph set
/// only): one cell wide, coloured like its label.
fn cat_icon(tab: &CatTab) -> &'static str {
    match tab {
        CatTab::All => "≡",
        CatTab::Installed => "✓",
        CatTab::Cat(id) => match id.as_str() {
            "osint" => "◎",
            "recon" => "⌖",
            "network" => "⇄",
            "web" => "⊕",
            "exploit" => "↯",
            "passwords" => "⊛",
            "reversing" => "⟲",
            "pwn" => "▲",
            "mobile" => "▯",
            "forensics" => "⌕",
            "crypto" => "⌗",
            "stego" => "◈",
            "wordlists" => "▤",
            _ => "◇",
        },
    }
}

/// The category bar: a full-width row of chips with how many tools each
/// shows (filter applied), scrolled so the active one is in view, with
/// markers when chips are hidden off either edge.
fn render_categories(f: &mut Frame, app: &App, th: &Theme, area: Rect) {
    let p = &th.pal;
    let g = th.glyph;
    let sep = if g.icons { " │ " } else { " | " };
    let chips: Vec<Line> = app
        .cat_tabs
        .iter()
        .enumerate()
        .map(|(i, t)| {
            let n = app.visible_in(t).len();
            let icon = if g.icons {
                format!("{} ", cat_icon(t))
            } else {
                String::new()
            };
            if i == app.cat {
                // text, not a bar: the solid bar belongs to the selected tool
                let on = th.strong(p.accent).add_modifier(Modifier::UNDERLINED);
                Line::from(Span::styled(format!("{icon}{} {n}", t.label()), on))
            } else {
                let name = if n == 0 { th.fg(p.dim) } else { th.fg(p.fg) };
                Line::from(vec![
                    Span::styled(format!("{icon}{}", t.label()), name),
                    Span::styled(format!(" {n}"), th.fg(p.dim)),
                ])
            }
        })
        .collect();
    if chips.is_empty() {
        return;
    }
    let w: Vec<usize> = chips.iter().map(Line::width).collect();
    let sep_w = sep.chars().count();
    let run = |a: usize, b: usize| w[a..=b].iter().sum::<usize>() + sep_w * (b - a);
    // two columns on each side for the overflow markers
    let avail = (area.width as usize).saturating_sub(4);
    let active = app.cat.min(chips.len() - 1);
    let mut start = 0;
    while start < active && run(start, active) > avail {
        start += 1;
    }
    let mut end = active.max(start);
    while end + 1 < chips.len() && run(start, end + 1) <= avail {
        end += 1;
    }

    // a click on a chip opens it; on an overflow marker, steps that way
    let mut line = HitLine::default();
    if start > 0 {
        let more = Span::styled(format!("{} ", g.more_l), th.fg(p.dim));
        line.push_hit(more, Hit::Key(KeyCode::Left));
    } else {
        line.push(Span::raw("  "));
    }
    for (i, chip) in chips.into_iter().enumerate().take(end + 1).skip(start) {
        if i > start {
            line.push(Span::styled(sep, th.fg(p.dim)));
        }
        let from = line.x;
        for s in chip.spans {
            line.push(s);
        }
        line.hit_since(from, Hit::Category(i));
    }
    if end + 1 < app.cat_tabs.len() {
        let more = Span::styled(format!(" {}", g.more_r), th.fg(p.dim));
        line.push_hit(more, Hit::Key(KeyCode::Right));
    }
    line.render(f, app, area);
}

/// Icon, its colour, and the short badge shown at the end of a list row.
fn status_look(th: &Theme, st: ToolStatus) -> (&'static str, Color, &'static str) {
    let p = &th.pal;
    let g = th.glyph;
    let fancy = g.icons;
    match st {
        ToolStatus::Installed => (g.installed, p.ok, if fancy { "✓ ready" } else { "ready" }),
        // not installed is an offer, not an error: no red
        ToolStatus::Missing => (g.missing, p.dim, "+ get"),
        ToolStatus::Unknown => (g.unknown, p.dim, if fancy { "…" } else { "..." }),
        ToolStatus::Conflict => (g.conflict, p.warn, "! PATH"),
    }
}

/// `s` cut to `max` columns, with an ellipsis when something was dropped.
fn clip(s: &str, max: usize, fancy: bool) -> String {
    if s.chars().count() <= max {
        return s.to_string();
    }
    if !fancy {
        return s.chars().take(max).collect();
    }
    let mut out: String = s.chars().take(max.saturating_sub(1)).collect();
    if max > 0 {
        out.push('…');
    }
    out
}

fn render_tool_list(f: &mut Frame, app: &App, th: &Theme, area: Rect) {
    // the wheel scrolls the list anywhere over it (rows are added on top)
    hit(app, area, Hit::ToolList);
    let p = &th.pal;
    let g = th.glyph;
    let tab = &app.cat_tabs[app.cat];
    // Pad names to the longest one so the category column stays aligned.
    let name_w = app
        .tools
        .iter()
        .map(|t| t.name.chars().count())
        .max()
        .unwrap_or(0);
    let glow_w = g.glow_l.chars().count();
    let content_w = (area.width.saturating_sub(2) as usize).saturating_sub(glow_w * 2);
    let visible = app.visible();
    let items: Vec<ListItem> = visible
        .iter()
        .map(|&i| {
            let t = &app.tools[i];
            let st = app.status[i];
            let (icon, color, badge) = status_look(th, st);
            // its other categories (the one being browsed goes without saying)
            let others: Vec<&str> = t
                .categories
                .iter()
                .map(String::as_str)
                .filter(|c| !matches!(tab, CatTab::Cat(id) if id == c))
                .collect();
            let name = format!("{:<name_w$}", t.name);
            let tail = if i == app.selected {
                match st {
                    ToolStatus::Installed if g.icons => "⏎ run",
                    ToolStatus::Installed => "Enter run",
                    ToolStatus::Conflict => "! PATH",
                    _ => "i install",
                }
            } else {
                badge
            };
            // "▸ ● name " before it, " badge " after it
            let room = content_w.saturating_sub(5 + name_w + tail.chars().count() + 2);
            let others = clip(&others.join(" "), room, g.icons);

            if i == app.selected {
                // the glow bar: one solid "you are here", with what Enter does
                let hint = tail;
                let left = format!("{} {icon} {name} {others}", g.arrow);
                let used = Span::raw(left.as_str()).width() + hint.chars().count() + 1;
                let pad = " ".repeat(content_w.saturating_sub(used).max(1));
                return ListItem::new(Line::from(vec![
                    Span::styled(g.glow_l, th.fg(p.accent)),
                    Span::styled(format!("{left}{pad}{hint} "), th.selected()),
                    Span::styled(g.glow_r, th.fg(p.accent)),
                ]));
            }

            let left_w = Span::raw(format!("  {icon} {name} {others}")).width();
            let used = left_w + badge.chars().count() + 1;
            let pad = " ".repeat(content_w.saturating_sub(used).max(1));
            ListItem::new(Line::from(vec![
                Span::raw(" ".repeat(glow_w + 2)),
                Span::styled(icon, th.fg(color)),
                Span::raw(" "),
                Span::styled(name, th.strong(p.fg)),
                Span::styled(format!(" {others}"), th.fg(p.dim)),
                Span::raw(pad),
                Span::styled(badge, th.fg(color)),
            ]))
        })
        .collect();

    let sel_pos = visible.iter().position(|&i| i == app.selected);
    let pos = match sel_pos {
        Some(i) => format!(" {}/{} ", i + 1, visible.len()),
        None => format!(" 0/{} ", visible.len()),
    };
    let mut title = vec![Span::styled(
        format!(" {} ", tab.label()),
        th.strong(p.accent),
    )];
    if !app.filter.trim().is_empty() {
        title.push(Span::styled(
            format!("/ {} ", app.filter.trim()),
            th.fg(p.warn),
        ));
    }
    let block = th
        .block(p.border)
        .title(Line::from(title))
        .title_bottom(Line::from(Span::styled(pos, th.fg(p.dim))).alignment(Alignment::Right));

    if items.is_empty() {
        // what happened and where to look, instead of an empty box
        let elsewhere = app.visible_in(&CatTab::All).len();
        let filter = app.filter.trim();
        let face = if g.icons { "∅" } else { "(none)" };
        let what = if !filter.is_empty() {
            format!("nothing for \"{filter}\" in {}", tab.label())
        } else if matches!(tab, CatTab::Installed) {
            "nothing installed yet".to_string()
        } else {
            format!("nothing in {} yet", tab.label())
        };
        let mut lines = vec![
            Line::raw(""),
            Line::from(Span::styled(face, th.strong(p.accent))),
            Line::raw(""),
            Line::from(Span::styled(what, th.fg(p.fg))),
        ];
        if !filter.is_empty() && elsewhere > 0 {
            lines.push(Line::from(Span::styled(
                format!("{elsewhere} match(es) in All · ← → to look · Esc clears"),
                th.fg(p.info),
            )));
        } else if matches!(tab, CatTab::Installed) {
            lines.push(Line::from(Span::styled(
                "pick a tool and press i to install it",
                th.fg(p.info),
            )));
        }
        f.render_widget(
            Paragraph::new(lines)
                .alignment(Alignment::Center)
                .wrap(ratatui::widgets::Wrap { trim: true })
                .block(block),
            area,
        );
        return;
    }

    // the selected row draws its own bar, so no highlight style or symbol
    let list = List::new(items).block(block);
    let mut state = ListState::default();
    state.select(sel_pos);
    f.render_stateful_widget(list, area, &mut state);
    // rendering scrolled the list: row k shows tool offset + k
    let first = state.offset();
    let rows = inside(area);
    for (k, &i) in visible
        .iter()
        .skip(first)
        .take(rows.height as usize)
        .enumerate()
    {
        hit_line(app, rows, k, Hit::Tool(i));
    }
}

/// What the highlighted tool is: its categories (main one first), whether
/// it's installed, how it runs, and what it does.
fn render_tool_card(f: &mut Frame, app: &App, th: &Theme, area: Rect) {
    if area.height == 0 {
        return;
    }
    let p = &th.pal;
    let g = th.glyph;
    let t = &app.tools[app.selected];
    let mut cats = Vec::new();
    for (k, c) in t.categories.iter().enumerate() {
        if k > 0 {
            cats.push(Span::styled(" · ", th.fg(p.dim)));
        }
        let label = registry::category_label(c);
        cats.push(if k == 0 {
            Span::styled(label, th.strong(p.accent))
        } else {
            Span::styled(label, th.fg(p.fg))
        });
    }
    let how = match t.launch {
        Launch::Stream => "runs in a terminal",
        Launch::Gui => "desktop app",
        Launch::Web if t.run_cmd.trim().is_empty() => "opens in the browser",
        Launch::Web => "server + browser",
        Launch::Dataset => "wordlist data",
    };
    let enter = if g.icons { "⏎" } else { "Enter" };
    let state = match app.status[app.selected] {
        ToolStatus::Installed => vec![
            Span::styled(format!("{} ready", g.installed), th.fg(p.ok)),
            Span::styled(" · ", th.fg(p.dim)),
            Span::styled(enter, th.strong(p.accent)),
            Span::styled(format!(" {how}"), th.fg(p.dim)),
        ],
        // an invitation, in neutral colours, with the key to press
        ToolStatus::Missing => vec![
            Span::styled(
                format!("{} not installed · press ", g.missing),
                th.fg(p.dim),
            ),
            Span::styled("i", th.strong(p.accent)),
            Span::styled(" to install", th.fg(p.dim)),
        ],
        ToolStatus::Unknown => vec![Span::styled(
            format!("{} checking…", g.unknown),
            th.fg(p.dim),
        )],
        ToolStatus::Conflict => vec![Span::styled(
            format!("{} name conflict on PATH", g.conflict),
            th.fg(p.warn),
        )],
    };
    let mut state = state;
    match t.sandbox {
        Sandbox::Never => {}
        Sandbox::Optional => {
            state.push(Span::styled(" · ", th.fg(p.dim)));
            state.push(Span::styled("S", th.strong(p.accent)));
            state.push(Span::styled(" sandbox", th.fg(p.dim)));
        }
        // it runs the sample: say so
        Sandbox::Recommended => {
            state.push(Span::styled(" · ", th.fg(p.dim)));
            state.push(Span::styled("S", th.strong(p.accent)));
            state.push(Span::styled(" sandbox (recommended)", th.fg(p.warn)));
        }
    }
    let mut lines = vec![Line::from(cats), Line::from(state)];
    // where it runs (the working folder), for tools that start a process
    // here; above the description, which may wrap, and shortened from the
    // left so it never wraps out of the panel itself
    if matches!(t.launch, Launch::Stream | Launch::Gui) {
        let room = (area.width as usize).saturating_sub(2 + "in ".len() + " · d change".len());
        lines.push(Line::from(vec![
            Span::styled("in ", th.fg(p.dim)),
            Span::styled(
                crate::folder::fit(&crate::folder::short(&app.cwd), room.max(8)),
                th.fg(p.info),
            ),
            Span::styled(" · ", th.fg(p.dim)),
            Span::styled("d", th.strong(p.accent)),
            Span::styled(" change", th.fg(p.dim)),
        ]));
    }
    lines.push(Line::from(Span::styled(t.description.clone(), th.fg(p.fg))));
    let block = th
        .block(p.border)
        .title(Span::styled(format!(" {} ", t.name), th.strong(p.fg)));
    f.render_widget(
        Paragraph::new(lines)
            .block(block)
            .wrap(ratatui::widgets::Wrap { trim: true }),
        area,
    );
}

/// Browser-style tabs: `┌0 console┐ 1 nmap ⠋  2 ffuf ✓•  3 +  4 +`
fn render_terminal_tabs(f: &mut Frame, app: &App, th: &Theme, area: Rect) {
    let p = &th.pal;
    let g = th.glyph;
    let spin = g.spinner;
    let frame = spin[(app.tick / 2 % spin.len() as u64) as usize];
    // a click on a tab brings that terminal to the front
    let mut line = HitLine::default();
    line.push(Span::raw(" "));
    for pos in 0..=SLOTS {
        let from = line.x;
        let sess = app.session(pos);
        let label = match sess {
            Some(s) => format!("{pos} {}", s.title),
            None => format!("{pos} +"),
        };
        if pos == app.focus {
            let mut on = th.strong(p.accent);
            if th.mono {
                on = on.add_modifier(Modifier::UNDERLINED);
            }
            line.push(Span::styled(format!("{}{label}{}", g.tab_l, g.tab_r), on));
        } else {
            let style = if sess.is_some() {
                th.fg(p.fg)
            } else {
                th.fg(p.dim)
            };
            line.push(Span::styled(format!(" {label} "), style));
        }
        if let Some(s) = sess {
            match s.state {
                RunState::Running => line.push(Span::styled(frame, th.fg(p.warn))),
                RunState::Idle => {}
                _ if s.failed() => line.push(Span::styled(g.failed, th.fg(p.err))),
                _ => line.push(Span::styled(g.done, th.fg(p.ok))),
            }
            if s.unread {
                line.push(Span::styled(g.unread, th.strong(p.info)));
            }
        }
        line.hit_since(from, Hit::Term(pos));
        line.push(Span::raw(" "));
    }
    if !app.grid && app.slots.iter().filter(|s| s.is_some()).count() > 1 {
        line.push_hit(
            Span::styled("g grid", th.fg(p.dim)),
            Hit::Key(KeyCode::Char('g')),
        );
    }
    line.render(f, app, area);
}

fn render_terminals(f: &mut Frame, app: &App, th: &Theme, area: Rect) {
    if !app.grid {
        render_session(f, app, th, area, app.focus, false);
        return;
    }
    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
        .split(area);
    for (r, row) in rows.iter().enumerate() {
        let cols = Layout::default()
            .direction(Direction::Horizontal)
            .constraints([Constraint::Percentage(50), Constraint::Percentage(50)])
            .split(*row);
        for (c, cell) in cols.iter().enumerate() {
            render_session(f, app, th, *cell, 1 + r * 2 + c, true);
        }
    }
}

/// One terminal (or an empty slot) at position `pos`.
fn render_session(f: &mut Frame, app: &App, th: &Theme, area: Rect, pos: usize, grid: bool) {
    let p = &th.pal;
    let focused = pos == app.focus;
    // a click brings it to the front (grid), the wheel scrolls it
    hit(app, area, Hit::Term(pos));
    let Some(s) = app.session(pos) else {
        let block = th
            .block(if grid && focused { p.accent } else { p.border })
            .title(Span::styled(format!(" {pos} free "), th.fg(p.dim)));
        let inner = block.inner(area);
        f.render_widget(block, area);
        render_free_slot(f, th, inner, pos);
        return;
    };

    let mut title = vec![Span::styled(
        if pos == 0 {
            " console ".to_string()
        } else {
            format!(" {pos} {} ", s.title)
        },
        th.strong(p.accent),
    )];
    if !s.subtitle.is_empty() {
        title.push(Span::styled(format!("· {} ", s.subtitle), th.fg(p.dim)));
    }

    // Lines are hard-wrapped here (not by Paragraph) so we know exactly how
    // many rows the tail takes: with word-wrap, long lines used to push the
    // newest output below the pane.
    let inner_height = area.height.saturating_sub(2) as usize;
    let inner_width = area.width.saturating_sub(2) as usize;
    let (rows, max_scroll) = visible_rows_at(&s.output, inner_width, inner_height, s.scroll);
    // the active challenge's own flag format is highlighted too
    let extra: Vec<String> = app
        .challenge
        .as_ref()
        .map(|w| w.info.flag_format.clone())
        .into_iter()
        .filter(|f| !f.is_empty())
        .collect();
    s.view.set((inner_height, max_scroll, inner_width));
    let text: Vec<Line> = if s.output.is_empty() {
        vec![Line::from(Span::styled(
            "no output yet",
            th.fg(p.dim).add_modifier(Modifier::ITALIC),
        ))]
    } else {
        rows.into_iter()
            .map(|(i, offset, chunk)| {
                let mut style = line_style(th, &s.output[i]);
                if s.search_hit == Some(i) {
                    style = th.emph(style.bg(p.hit_bg));
                }
                let flag = th
                    .strong(p.accent)
                    .add_modifier(Modifier::UNDERLINED | Modifier::BOLD);
                Line::from(flag_spans(&s.output[i], offset, chunk, style, flag, &extra))
            })
            .collect()
    };

    let mut bottom = state_line(app, th, s);
    if s.scroll > 0 {
        bottom.push_span(Span::styled(
            format!(" ↑ {} rows up · End to follow ", s.scroll.min(max_scroll)),
            th.fg(p.info),
        ));
    }
    let border = if s.running() {
        p.warn
    } else if grid && focused {
        p.accent
    } else {
        p.border
    };
    let bottom_w = bottom.width() as u16;
    let title_w: u16 = title.iter().map(|s| s.width() as u16).sum();
    let mut block = th.block(border).title(title).title_bottom(bottom);
    // top right: a copy button (the y key), where the title leaves room
    const COPY: &str = " y copy ";
    let copy_w = COPY.len() as u16;
    let copy = !s.output.is_empty() && area.width >= title_w + copy_w + 3;
    if copy {
        block = block.title(
            Line::from(vec![
                Span::styled(" y", th.strong(p.accent)),
                Span::styled(" copy ", th.fg(p.dim)),
            ])
            .alignment(Alignment::Right),
        );
    }
    // bottom right: key hints, each one clickable
    let mut keys: &[(&str, char)] = &[];
    if focused && pos > 0 && !grid {
        keys = if s.running() {
            &[(" s input ·", 's'), (" c stop ·", 'c'), (" x close ", 'x')]
        } else {
            &[(" x close ", 'x')]
        };
        let hint: String = keys.iter().map(|k| k.0).collect();
        block = block
            .title_bottom(Line::from(Span::styled(hint, th.fg(p.dim))).alignment(Alignment::Right));
    }
    f.render_widget(Paragraph::new(text).block(block), area);
    if copy {
        let r = Rect::new(area.right() - 1 - copy_w, area.y, copy_w, 1);
        hit(app, r, Hit::Copy(pos));
    }
    let hint_w: u16 = keys.iter().map(|k| k.0.chars().count() as u16).sum();
    // only where the state on the left doesn't cover them
    if hint_w > 0 && area.height >= 2 && area.width >= bottom_w + hint_w + 3 {
        let mut x = area.right() - 1 - hint_w;
        for &(text, key) in keys {
            let w = text.chars().count() as u16;
            let r = Rect::new(x, area.bottom() - 1, w, 1);
            hit(app, r, Hit::Key(KeyCode::Char(key)));
            x += w;
        }
    }
}

/// Bottom-left of a terminal: what its process is doing.
fn state_line(app: &App, th: &Theme, s: &Session) -> Line<'static> {
    let p = &th.pal;
    match s.state {
        RunState::Running => {
            let spin = th.glyph.spinner;
            let frame = spin[(app.tick / 2 % spin.len() as u64) as usize];
            let pid = s.pid.map(|p| format!("pid {p} ")).unwrap_or_default();
            Line::from(vec![
                Span::styled(format!(" {frame} running "), th.strong(p.warn)),
                Span::styled(pid, th.fg(p.dim)),
            ])
        }
        RunState::Idle => Line::from(Span::styled(" idle ", th.fg(p.dim))),
        RunState::Killed => Line::from(Span::styled(" stopped ", th.fg(p.err))),
        RunState::Exited(code) => {
            let text = match code {
                Some(c) => format!(" exit {c} "),
                None => " ended ".to_string(),
            };
            let c = if s.failed() { p.err } else { p.ok };
            Line::from(Span::styled(text, th.fg(c)))
        }
    }
}

/// Split one wrapped row into spans, drawing any flag in it with `flag`.
/// `offset` is where the row starts in the source line, in characters.
fn flag_spans(
    source: &str,
    offset: usize,
    chunk: String,
    base: Style,
    flag: Style,
    extra: &[String],
) -> Vec<Span<'static>> {
    let hits = crate::flags::find_with(source, extra);
    if hits.is_empty() {
        return vec![Span::styled(chunk, base)];
    }
    let chars: Vec<char> = chunk.chars().collect();
    let mut spans = Vec::new();
    let mut run = String::new();
    let mut run_flag = false;
    for (k, ch) in chars.iter().enumerate() {
        let at = offset + k;
        let in_flag = hits.iter().any(|h| at >= h.start && at < h.end);
        if in_flag != run_flag && !run.is_empty() {
            spans.push(Span::styled(
                std::mem::take(&mut run),
                if run_flag { flag } else { base },
            ));
        }
        run_flag = in_flag;
        run.push(*ch);
    }
    if !run.is_empty() {
        spans.push(Span::styled(run, if run_flag { flag } else { base }));
    }
    spans
}

/// Hard-wrap `output` to `width` columns and return the `height` rows that
/// are visible `scroll` rows above the bottom, as (source line index, text),
/// plus the largest useful scroll value. Only walks as far back as needed.
#[cfg(test)]
pub fn visible_rows(
    output: &[String],
    width: usize,
    height: usize,
    scroll: usize,
) -> (Vec<(usize, String)>, usize) {
    let (rows, max) = visible_rows_at(output, width, height, scroll);
    (rows.into_iter().map(|(i, _, t)| (i, t)).collect(), max)
}

/// `visible_rows`, plus where each row starts in its source line (in
/// characters), so a highlight can follow a flag across wrapped rows.
pub fn visible_rows_at(
    output: &[String],
    width: usize,
    height: usize,
    scroll: usize,
) -> (Vec<(usize, usize, String)>, usize) {
    let width = width.max(1);
    let need = scroll + height;
    // rows collected bottom-up; past `need` rows we only count (for max_scroll)
    let mut rows: Vec<(usize, usize, String)> = Vec::new();
    let mut total_rows = 0usize;
    for (i, line) in output.iter().enumerate().rev() {
        let chars: Vec<char> = line.chars().collect();
        let n = chars.len().div_ceil(width).max(1);
        total_rows += n;
        if rows.len() < need {
            let chunks: Vec<String> = if chars.is_empty() {
                vec![String::new()]
            } else {
                chars.chunks(width).map(|c| c.iter().collect()).collect()
            };
            for (k, chunk) in chunks.into_iter().enumerate().rev() {
                rows.push((i, k * width, chunk));
            }
        }
    }
    let max_scroll = total_rows.saturating_sub(height);
    let scroll = scroll.min(max_scroll);
    let visible: Vec<(usize, usize, String)> = rows
        .into_iter()
        .skip(scroll)
        .take(height)
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    (visible, max_scroll)
}

/// Colour for an output line, by its prefix (applies to all its wrapped rows).
fn line_style(th: &Theme, l: &str) -> Style {
    let p = &th.pal;
    if l.starts_with("[err]") {
        th.fg(p.err)
    } else if l.starts_with("> ") {
        th.fg(p.info)
    } else if l.starts_with("$ ") {
        th.strong(p.warn)
    } else if l.starts_with("──") || l.starts_with("::") {
        th.fg(p.dim).add_modifier(Modifier::ITALIC)
    } else {
        th.fg(p.fg)
    }
}

fn render_footer(f: &mut Frame, app: &App, th: &Theme, area: Rect) {
    let p = &th.pal;
    let arrow = th.glyph.arrow;
    let content = match app.mode {
        Mode::SlotPick => {
            let mut spans = vec![
                Span::styled(
                    format!(" all {SLOTS} terminals busy {arrow} "),
                    pill(th, p.warn),
                ),
                Span::raw(" "),
            ];
            for pos in 1..=SLOTS {
                if let Some(s) = app.session(pos) {
                    spans.push(Span::styled(format!("{pos}"), th.strong(p.accent)));
                    spans.push(Span::styled(format!(" {}  ", s.title), th.fg(p.fg)));
                }
            }
            spans.push(Span::styled(
                "press a number to stop that tool and use its terminal · Esc cancel",
                th.fg(p.dim),
            ));
            Line::from(spans)
        }
        Mode::Input
        | Mode::HashInput
        | Mode::StdinInput
        | Mode::Filter
        | Mode::Search
        | Mode::SandboxInput
        | Mode::FolderInput
        | Mode::Challenge(_)
        | Mode::Recipe(_) => {
            let net_help;
            let folder_help;
            let args_help;
            let (name, help) = match app.mode {
                Mode::Recipe(crate::recipe::Slot::Target) => (
                    "target",
                    "  IP, host, URL or nc host port · saved in the challenge · Enter · Esc cancel",
                ),
                Mode::Recipe(crate::recipe::Slot::File) => (
                    "file",
                    "  the challenge file to use (copied into the challenge) · Enter · Esc cancel",
                ),
                Mode::Recipe(_) => ("wordlist", "  path to the wordlist · Enter · Esc cancel"),
                Mode::Challenge(ChallengeField::Event) => {
                    ("event", "  e.g. picoCTF 2026 · Enter next · Esc cancel")
                }
                Mode::Challenge(ChallengeField::Name) => {
                    ("challenge", "  e.g. rev crackme1 · Enter next · Esc cancel")
                }
                Mode::Challenge(ChallengeField::Target) => (
                    "target",
                    "  IP, host, URL or nc host port (optional) · Enter · Esc cancel",
                ),
                Mode::Challenge(ChallengeField::Flag) => (
                    "flag format",
                    "  e.g. ACME{ (optional: known formats already work) · Enter · Esc cancel",
                ),
                Mode::Challenge(ChallengeField::File) => (
                    "add file",
                    "  a challenge file to copy in (optional) · Enter · Esc done",
                ),
                Mode::SandboxInput => {
                    net_help = format!(
                        "  sample file or folder (read-only) · Tab network: {} · Enter start · Esc cancel",
                        if app.sandbox_net { "ON" } else { "off" }
                    );
                    ("sandbox", net_help.as_str())
                }
                Mode::FolderInput => {
                    // cycling through Tab matches: which one is shown
                    let tab = match &app.folder_tab {
                        Some((_, m, i)) => {
                            format!("{}/{} Tab next · Shift+Tab back", i + 1, m.len())
                        }
                        None => "Tab completes · .. ~ -".to_string(),
                    };
                    folder_help = format!(
                        "  now {} · {tab} · Enter go · Esc cancel",
                        crate::folder::fit(&crate::folder::short(&app.cwd), 32)
                    );
                    ("folder", folder_help.as_str())
                }
                Mode::HashInput => ("hash", "  Enter identify · Esc cancel"),
                Mode::StdinInput => ("stdin", "  Enter send · Ctrl+D end input · Esc close"),
                Mode::Filter => (
                    "filter",
                    "  name / category / text · Enter keep · Esc clear",
                ),
                Mode::Search => ("find", "  Enter find in output · then n / N · Esc cancel"),
                _ => {
                    // say where it will run, like a shell prompt
                    args_help = format!(
                        "  in {} · Enter run · ↑↓ history · Esc cancel",
                        crate::folder::fit(&crate::folder::short(&app.cwd), 32)
                    );
                    ("args", args_help.as_str())
                }
            };
            let label = format!(" {name} {arrow} ");
            let label_w = label.chars().count() as u16 + 1; // + the space after it
                                                            // Drop the help text when space is tight; the line itself matters more.
            let help_w = help.chars().count() as u16;
            let show_help = area.width > label_w + help_w + 20;
            let text_w = area
                .width
                .saturating_sub(label_w + if show_help { help_w } else { 0 })
                .max(1);
            let (visible, col) = app.editor.view(text_w as usize);
            // no terminal cursor while an overlay sits on top
            if app.appearance.is_none() && !app.show_help {
                f.set_cursor_position((area.x + label_w + col as u16, area.y));
            }
            let mut spans = vec![
                Span::styled(label, pill(th, p.warn)),
                Span::raw(" "),
                Span::styled(format!("{visible:<w$}", w = text_w as usize), th.fg(p.fg)),
            ];
            if show_help {
                spans.push(Span::styled(help.to_string(), th.fg(p.dim)));
            }
            Line::from(spans)
        }
        Mode::Browsing => {
            let key = |k: &'static str| Span::styled(k, th.fg(p.accent));
            let sep = |d: &'static str| Span::styled(d, th.fg(p.dim));
            let mut line = HitLine::default();
            // plain text: only a pending "press again" gets a solid label,
            // with a button that confirms it
            if let Some(c) = app.confirm {
                let yes = match c {
                    Confirm::Close(_) => " yes, close ",
                    Confirm::Quit => " yes, quit ",
                };
                line.push(Span::raw(" "));
                line.push_hit(Span::styled(yes, pill(th, p.warn)), Hit::Confirm);
                line.push(Span::styled(
                    format!(" {} ", app.status_line),
                    pill(th, p.err),
                ));
            } else {
                line.push(Span::styled(format!(" {}", app.status_line), th.fg(p.fg)));
            }
            line.push(Span::raw("  "));
            // each hint presses its key when clicked
            let hint = |k, d, code| ([key(k), sep(d)], Hit::Key(code));
            let hints = [
                hint("Enter", " run · ", KeyCode::Enter),
                hint("i", " install · ", KeyCode::Char('i')),
                hint("←→", " category · ", KeyCode::Right),
                hint("/", " filter · ", KeyCode::Char('/')),
                hint("0-4", " terminals · ", KeyCode::Tab),
                hint("t", " theme · ", KeyCode::Char('t')),
            ];
            let mut help = hint("?", " all keys · ", KeyCode::Char('?'));
            help.0[0] = Span::styled("?", th.strong(p.accent));
            let tail = [help, hint("q", " quit", KeyCode::Char('q'))];
            // the web hint only when it fits: "q quit" must stay visible
            let web = hint(
                "w",
                if app.web.is_some() {
                    " web on · "
                } else {
                    " web · "
                },
                KeyCode::Char('w'),
            );
            let width = |h: &[([Span; 2], Hit)]| {
                h.iter()
                    .flat_map(|(s, _)| s.iter())
                    .map(Span::width)
                    .sum::<usize>()
            };
            let fits = line.x + width(&hints) + width(std::slice::from_ref(&web)) + width(&tail)
                <= area.width as usize;
            let mut all: Vec<([Span; 2], Hit)> = hints.into();
            if fits {
                all.push(web);
            }
            all.extend(tail);
            for (spans, h) in all {
                let from = line.x;
                for s in spans {
                    line.push(s);
                }
                line.hit_since(from, h);
            }
            line.render(f, app, area);
            return;
        }
    };
    f.render_widget(Paragraph::new(content), area);
}

/// The inside of a free terminal: a little terminal drawing and what to
/// do next, or one line when the pane is small (grid cells).
fn render_free_slot(f: &mut Frame, th: &Theme, inner: Rect, pos: usize) {
    let p = &th.pal;
    let fancy = th.glyph.icons;
    let hint = if fancy {
        "⏎ runs the selected tool here"
    } else {
        "Enter runs the selected tool here"
    };
    let art: [&str; 3] = if fancy {
        ["╭──────────╮", "│ >_       │", "╰──────────╯"]
    } else {
        ["+----------+", "| >_       |", "+----------+"]
    };
    let mut lines: Vec<Line> = Vec::new();
    let big = inner.height >= 7 && inner.width >= 36;
    let body = if big { 6 } else { 1 };
    for _ in 0..inner.height.saturating_sub(body) / 2 {
        lines.push(Line::raw(""));
    }
    if big {
        for a in art {
            lines.push(Line::from(Span::styled(a, th.fg(p.border))));
        }
        lines.push(Line::raw(""));
        lines.push(Line::from(Span::styled(
            format!("terminal {pos} is free"),
            th.strong(p.fg),
        )));
        lines.push(Line::from(Span::styled(hint, th.fg(p.dim))));
    } else {
        lines.push(Line::from(Span::styled(
            format!("{pos} · free · {hint}"),
            th.fg(p.dim),
        )));
    }
    f.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(ratatui::widgets::Wrap { trim: true }),
        inner,
    );
}

/// Corner notifications, newest at the bottom, bottom-right of the body.
fn render_toasts(f: &mut Frame, app: &App, th: &Theme, area: Rect) {
    let p = &th.pal;
    let mut bottom = area.bottom();
    for t in app.toasts.iter().rev() {
        if bottom < area.y + 3 || area.width < 6 {
            break;
        }
        let text = format!(" {} ", t.text);
        let w = (Span::raw(text.as_str()).width() as u16 + 2).min(area.width);
        bottom -= 3;
        let r = Rect {
            x: area.right().saturating_sub(w + 1).max(area.x),
            y: bottom,
            width: w,
            height: 3,
        };
        let edge = match t.kind {
            ToastKind::Ok => p.ok,
            ToastKind::Err => p.err,
            ToastKind::Info => p.info,
        };
        f.render_widget(Clear, r);
        f.render_widget(
            Paragraph::new(Line::from(Span::styled(text, th.fg(p.fg)))).block(th.block(edge)),
            r,
        );
    }
}

/// The recipe picker: the selected tool's recipes, filled with what's
/// known; missing values show as `{target}` gaps (asked for on Enter).
fn render_recipes(f: &mut Frame, app: &App, cur: usize, th: &Theme, area: Rect) {
    let p = &th.pal;
    let g = th.glyph;
    let t = &app.tools[app.selected];
    let v = app.recipe_values(false);
    let name_w = t
        .recipes
        .iter()
        .map(|r| r.name.chars().count())
        .max()
        .unwrap_or(0)
        .max(6);
    let n = t.recipes.len() + 1;
    let mut lines: Vec<Line> = Vec::new();
    for row in 0..n {
        let on = row == cur.min(n - 1);
        let (name, args, gaps) = match t.recipes.get(row) {
            Some(r) => {
                let missing = crate::recipe::fill(&r.args, &v).err().unwrap_or_default();
                (
                    r.name.clone(),
                    crate::recipe::fill_partial(&r.args, &v),
                    missing,
                )
            }
            None => (
                "custom".to_string(),
                t.args_hint.clone().unwrap_or_default(),
                vec![],
            ),
        };
        // a preview: the challenge folder shortens to … (the prompt gets
        // the full path)
        let args = match &app.challenge {
            Some(w) => args.replace(&w.dir.display().to_string(), "…"),
            None => args,
        };
        let mut spans = vec![
            Span::styled(if on { g.pointer } else { "  " }, th.fg(p.accent)),
            Span::styled(
                format!("{name:<name_w$}"),
                if on { th.selected() } else { th.strong(p.fg) },
            ),
            Span::styled(format!("  {args}"), th.fg(p.dim)),
        ];
        if !gaps.is_empty() {
            let names: Vec<&str> = gaps.iter().map(|s| s.name()).collect();
            spans.push(Span::styled(
                format!("  asks for {}", names.join(", ")),
                th.fg(p.warn),
            ));
        }
        lines.push(Line::from(spans));
    }
    let w = lines.iter().map(|l| l.width()).max().unwrap_or(40) as u16 + 4;
    let h = lines.len() as u16 + 2;
    let popup = overlay_area(f, th, area, w.max(50), h);
    hit(app, popup, Hit::Popup(Layer::Recipes));
    for row in 0..n {
        hit_line(app, inside(popup), row, Hit::Row(Layer::Recipes, row));
    }
    let title = format!("{} · recipes", t.name);
    f.render_widget(
        Paragraph::new(lines).block(overlay_block(
            th,
            &title,
            "↑↓ choose · Enter fill & edit · Esc",
        )),
        popup,
    );
}

/// The `C` overlay: every challenge under the root, the active one marked.
fn render_challenges(f: &mut Frame, app: &App, cur: usize, th: &Theme, area: Rect) {
    let p = &th.pal;
    let g = th.glyph;
    let mut lines: Vec<Line> = Vec::new();
    match &app.challenge {
        Some(w) => {
            let files = w.files();
            let mut spans = vec![
                Span::styled(" active  ", th.fg(p.dim)),
                Span::styled(w.title(), th.strong(p.fg)),
            ];
            if !w.info.target.is_empty() {
                spans.push(Span::styled(format!("  {}", w.info.target), th.fg(p.info)));
            }
            spans.push(Span::styled(
                format!(
                    "  · {} file{}{}",
                    files.len(),
                    if files.len() == 1 { "" } else { "s" },
                    if w.info.flag_format.is_empty() {
                        String::new()
                    } else {
                        format!(" · flag {}{{…}}", w.info.flag_format)
                    }
                ),
                th.fg(p.dim),
            ));
            lines.push(Line::from(spans));
        }
        None => lines.push(Line::from(Span::styled(
            " no active challenge: logs and flags go to the default folder",
            th.fg(p.dim),
        ))),
    }
    lines.push(Line::raw(""));
    if app.challenge_list.is_empty() {
        lines.push(Line::from(Span::styled(
            format!(
                " no challenges yet · n creates one in {}",
                app.ctf_root.display()
            ),
            th.fg(p.dim),
        )));
    }
    let n = app.challenge_list.len();
    let first = lines.len();
    for (row, w) in app.challenge_list.iter().enumerate() {
        let on = row == cur.min(n.saturating_sub(1));
        let active = app.challenge.as_ref().is_some_and(|a| a.id == w.id);
        let mut spans = vec![
            Span::styled(if on { g.pointer } else { "  " }, th.fg(p.accent)),
            Span::styled(w.title(), if on { th.selected() } else { th.fg(p.fg) }),
        ];
        if !w.info.target.is_empty() {
            spans.push(Span::styled(format!("  {}", w.info.target), th.fg(p.dim)));
        }
        if active {
            spans.push(Span::styled(
                format!("  {} active", g.installed),
                th.fg(p.ok),
            ));
        }
        lines.push(Line::from(spans));
    }
    let w = lines.iter().map(|l| l.width()).max().unwrap_or(40) as u16 + 4;
    let h = lines.len().min(22) as u16 + 2;
    let popup = overlay_area(f, th, area, w.max(60), h);
    hit(app, popup, Hit::Popup(Layer::Challenges));
    for row in 0..n {
        hit_line(
            app,
            inside(popup),
            first + row,
            Hit::Row(Layer::Challenges, row),
        );
    }
    f.render_widget(
        Paragraph::new(lines).block(overlay_block(
            th,
            "challenges",
            "↑↓ · Enter switch · n new · t target · l flag format · f add file · m notes · o folder · x leave · Esc",
        )),
        popup,
    );
}

/// The `F` overlay: every flag caught this session, newest first.
fn render_flags(f: &mut Frame, app: &App, cur: usize, th: &Theme, area: Rect) {
    let p = &th.pal;
    let g = th.glyph;
    let mut lines: Vec<Line> = Vec::new();
    if app.flags.is_empty() {
        lines.push(Line::from(Span::styled(
            " no flags yet: anything like picoCTF{…} or HTB{…} in a tool's output lands here",
            th.fg(p.dim),
        )));
    }
    let n = app.flags.len();
    for (row, fl) in app.flags.iter().rev().enumerate() {
        let on = row == cur.min(n.saturating_sub(1));
        let time = fl.at.get(11..16).unwrap_or("");
        let mut spans = vec![
            Span::styled(if on { g.pointer } else { "  " }, th.fg(p.accent)),
            Span::styled(
                fl.text.clone(),
                if on {
                    th.selected()
                } else if fl.known {
                    th.strong(p.accent)
                } else {
                    th.fg(p.fg)
                },
            ),
        ];
        if !fl.known {
            spans.push(Span::styled("  likely", th.fg(p.warn)));
        }
        spans.push(Span::styled(
            format!("  {} · {time}", fl.source),
            th.fg(p.dim),
        ));
        lines.push(Line::from(spans));
    }
    let w = lines.iter().map(|l| l.width()).max().unwrap_or(40) as u16 + 4;
    let h = lines.len().min(20) as u16 + 2;
    let popup = overlay_area(f, th, area, w.max(50), h);
    hit(app, popup, Hit::Popup(Layer::Flags));
    for row in 0..n {
        hit_line(app, inside(popup), row, Hit::Row(Layer::Flags, row));
    }
    let title = format!("flags · {n}");
    f.render_widget(
        Paragraph::new(lines).block(overlay_block(
            th,
            &title,
            "↑↓ choose · Enter copy · Esc close",
        )),
        popup,
    );
}

/// What each step of the welcome tour says.
pub const TOUR: [&str; TOUR_STEPS] = [
    "↑ ↓ move through the tools · ← → switch category",
    "⏎ runs a tool in its own terminal · i installs it",
    "/ filters · ? shows every key · t changes the look",
];

/// Setup's tool packs: every tool that would be installed, then y / n.
fn render_first_run(f: &mut Frame, th: &Theme, area: Rect, names: &[String]) {
    let p = &th.pal;
    let w = area.width.saturating_sub(6).clamp(30, 76);
    let inner = w.saturating_sub(4) as usize;
    let mut lines = vec![
        Line::from(Span::styled(
            format!("Install these {} tools?", names.len()),
            th.strong(p.fg),
        )),
        Line::raw(""),
    ];
    let mut row = String::new();
    for n in names {
        if !row.is_empty() && row.chars().count() + n.chars().count() + 2 > inner {
            lines.push(Line::from(Span::styled(
                std::mem::take(&mut row),
                th.fg(p.fg),
            )));
        }
        if !row.is_empty() {
            row.push_str(", ");
        }
        row.push_str(n);
    }
    lines.push(Line::from(Span::styled(row, th.fg(p.fg))));
    lines.push(Line::raw(""));
    lines.push(Line::from(Span::styled(
        "You picked these packs in setup. They install one by one in the console.",
        th.fg(p.dim),
    )));
    let h = lines.len() as u16 + 2;
    dim_backdrop(f, th, area);
    let card = centered(area, w, h.min(area.height));
    f.render_widget(Clear, card);
    f.render_widget(
        Paragraph::new(lines)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(overlay_block(th, "tool packs", "y install · n skip")),
        card,
    );
}

/// The first-run welcome card, always centered, on top of everything.
fn render_tour(f: &mut Frame, th: &Theme, area: Rect, step: usize) {
    let p = &th.pal;
    let g = th.glyph;
    let step = step.min(TOUR_STEPS - 1);
    let text = TOUR[step];
    let mut dots = Vec::new();
    for i in 0..TOUR_STEPS {
        if i > 0 {
            dots.push(Span::raw(" "));
        }
        dots.push(if i == step {
            Span::styled(g.installed, th.strong(p.accent))
        } else {
            Span::styled(g.missing, th.fg(p.dim))
        });
    }
    let lines = vec![
        Line::raw(""),
        Line::from(Span::styled(text, th.strong(p.fg))),
        Line::raw(""),
        Line::from(dots),
    ];
    let hint = if step + 1 == TOUR_STEPS {
        "⏎ let's go · ← back · Esc skip"
    } else {
        "⏎ next · ← back · Esc skip"
    };
    let w = (Span::raw(text).width() as u16 + 8).max(54);
    dim_backdrop(f, th, area);
    let card = centered(area, w, 8);
    f.render_widget(Clear, card);
    f.render_widget(
        Paragraph::new(lines)
            .alignment(Alignment::Center)
            .wrap(ratatui::widgets::Wrap { trim: true })
            .block(overlay_block(th, "welcome to winsploit", hint)),
        card,
    );
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::registry;
    use ratatui::backend::TestBackend;
    use ratatui::Terminal;
    use std::path::PathBuf;

    fn app() -> App {
        let cat = registry::parse(include_str!("../tools.yaml")).unwrap();
        App::new(cat, PathBuf::from("tools.yaml"), None)
    }

    fn draw(app: &App, w: u16, h: u16) -> Terminal<TestBackend> {
        let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
        t.draw(|f| render(f, app)).unwrap();
        t
    }

    fn row(t: &Terminal<TestBackend>, y: u16) -> String {
        let buf = t.backend().buffer();
        (0..buf.area.width)
            .map(|x| buf[(x, y)].symbol().to_string())
            .collect()
    }

    #[test]
    fn renders_at_common_and_tiny_sizes() {
        let a = app();
        for (w, h) in [(120, 40), (80, 24), (40, 10), (10, 4)] {
            draw(&a, w, h);
        }
    }

    #[test]
    fn provider_picker_lists_every_ai() {
        let mut a = app();
        a.provider_pick = Some(ProviderPick {
            cursor: 0,
            editing: None,
        });
        for (w, h) in [(40, 10), (10, 4)] {
            draw(&a, w, h);
        }
        let t = draw(&a, 120, 30);
        let screen: String = (0..30).map(|y| row(&t, y)).collect::<Vec<_>>().join("\n");
        for k in [
            "Claude", "Gemini", "DeepSeek", "OpenAI", "Ollama", "model:", "sonnet",
        ] {
            assert!(screen.contains(k), "provider picker is missing {k:?}");
        }
    }

    #[test]
    fn project_menu_and_browser_render() {
        let mut a = app();
        // the New/Open/Recent menu
        a.project_ui = Some(crate::app::ProjectUi::Menu { cursor: 0 });
        for (w, h) in [(40, 10), (10, 4)] {
            draw(&a, w, h);
        }
        let t = draw(&a, 120, 30);
        let menu: String = (0..30).map(|y| row(&t, y)).collect::<Vec<_>>().join("\n");
        assert!(menu.contains("New project"), "menu missing New project");
        assert!(menu.contains("Open"), "menu missing Open");

        // the folder browser
        a.project_ui = Some(crate::app::ProjectUi::Browse {
            dir: std::path::PathBuf::from("C:/ctf"),
            entries: vec![std::path::PathBuf::from("C:/ctf/boxA")],
            cursor: 0,
            new: false,
        });
        let t = draw(&a, 120, 30);
        let brw: String = (0..30).map(|y| row(&t, y)).collect::<Vec<_>>().join("\n");
        assert!(brw.contains("boxA"), "browser missing the subfolder");
        assert!(brw.contains(".."), "browser missing the parent row");
    }

    #[test]
    fn agent_overlay_shows_summary_and_suggestions() {
        let mut a = app();
        a.agent = Some(crate::app::AgentView {
            analysis: crate::agent::Analysis {
                summary: "two services exposed".into(),
                findings: vec!["ssh open".into()],
                suggestions: vec![crate::agent::Suggestion {
                    tool: "nuclei".into(),
                    args: "-u http://t".into(),
                    rationale: "scan the web port".into(),
                }],
            },
            cursor: 0,
        });
        // tiny sizes must not panic either
        for (w, h) in [(40, 10), (10, 4)] {
            draw(&a, w, h);
        }
        let t = draw(&a, 120, 30);
        let screen: String = (0..30).map(|y| row(&t, y)).collect::<Vec<_>>().join("\n");
        for k in ["two services exposed", "ssh open", "nuclei", "Enter run"] {
            assert!(screen.contains(k), "agent overlay is missing {k:?}");
        }
    }

    #[test]
    fn args_prompt_shows_text_and_places_cursor() {
        let mut a = app();
        a.mode = Mode::Input;
        a.editor.set("-sV scanme.nmap.org");
        a.editor.home();
        let mut t = draw(&a, 100, 20);
        let footer = row(&t, 19);
        assert!(footer.contains("args ▸"), "{footer}");
        assert!(footer.contains("-sV scanme.nmap.org"), "{footer}");
        // cursor sits on the first character of the text
        let label_w = " args ▸ ".chars().count() as u16 + 1;
        assert_eq!(t.get_cursor_position().unwrap(), (label_w, 19).into());
    }

    #[test]
    fn long_args_scroll_to_keep_the_cursor_visible() {
        let mut a = app();
        a.mode = Mode::Input;
        let long = format!("-u https://example.com/{}END", "x".repeat(200));
        a.editor.set(&long);
        let mut t = draw(&a, 60, 12);
        let footer = row(&t, 11);
        assert!(footer.contains("END"), "{footer}");
        assert!(t.get_cursor_position().unwrap().x < 60);
    }

    fn lines(v: &[&str]) -> Vec<String> {
        v.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn tail_accounts_for_wrapped_rows() {
        // the last line is 2.5x the width: the newest rows must be visible
        let out = lines(&["one", "two", &"x".repeat(25)]);
        let (rows, max) = visible_rows(&out, 10, 3, 0);
        let text: Vec<&str> = rows.iter().map(|(_, s)| s.as_str()).collect();
        assert_eq!(text, ["xxxxxxxxxx", "xxxxxxxxxx", "xxxxx"]);
        assert_eq!(max, 2); // 5 rows total, 3 visible
    }

    #[test]
    fn scrolling_up_and_clamping() {
        let out = lines(&["a", "b", "c", "d", "e"]);
        let at = |s| {
            visible_rows(&out, 10, 2, s)
                .0
                .into_iter()
                .map(|(_, t)| t)
                .collect::<Vec<_>>()
        };
        assert_eq!(at(0), ["d", "e"]);
        assert_eq!(at(1), ["c", "d"]);
        assert_eq!(at(3), ["a", "b"]);
        assert_eq!(at(99), ["a", "b"]); // clamped to the top
        assert_eq!(visible_rows(&out, 10, 2, 0).1, 3);
    }

    #[test]
    fn empty_lines_and_indices() {
        let out = lines(&["", "abcdef"]);
        let (rows, _) = visible_rows(&out, 4, 5, 0);
        assert_eq!(
            rows,
            [
                (0, "".to_string()),
                (1, "abcd".to_string()),
                (1, "ef".to_string())
            ]
        );
    }

    #[test]
    fn filter_narrows_the_list() {
        let mut a = app();
        a.filter = "reversing".into();
        let vis = a.visible();
        assert!(vis.len() > 3);
        assert!(vis.iter().all(|&i| a.tools[i].in_category("reversing")));
        // words AND together, across name/category/description
        a.filter = "web brute".into();
        let names: Vec<&str> = a
            .visible()
            .iter()
            .map(|&i| a.tools[i].name.as_str())
            .collect();
        assert!(names.contains(&"dirsearch"), "{names:?}");
        assert!(!names.contains(&"nmap"));
        // renders with a filter, including one that matches nothing
        draw(&a, 100, 20);
        a.filter = "zzz-no-match".into();
        assert!(a.visible().is_empty());
        draw(&a, 100, 20);
    }

    #[test]
    fn help_overlay_renders_and_lists_every_browsing_key() {
        let mut a = app();
        a.show_help = true;
        for (w, h) in [(100, 30), (40, 10)] {
            draw(&a, w, h);
        }
        let t = draw(&a, 120, 40);
        let screen: String = (0..40).map(|y| row(&t, y)).collect::<Vec<_>>().join("\n");
        for k in [
            "filter",
            "install",
            "find in the output",
            "identify a hash",
            "any key to close",
            "Navigate",
            "Output",
            "in prompts",
        ] {
            assert!(screen.contains(k), "help is missing {k:?}");
        }
        // narrow terminals fall back to one column: headings stack
        let t = draw(&a, 80, 70);
        let screen: String = (0..70).map(|y| row(&t, y)).collect::<Vec<_>>().join("\n");
        assert!(
            screen.contains("Navigate") && screen.contains("in prompts"),
            "{screen}"
        );
    }

    #[test]
    fn help_headings_have_icons_and_ascii_mode_has_none() {
        let mut a = app();
        a.show_help = true;
        let t = draw(&a, 120, 30);
        let screen: String = (0..30).map(|y| row(&t, y)).collect::<Vec<_>>().join("\n");
        assert!(screen.contains("◆ Navigate"), "{screen}");
        a.settings.glyphs = crate::theme::GlyphSet::Ascii;
        let t = draw(&a, 120, 30);
        let screen: String = (0..30).map(|y| row(&t, y)).collect::<Vec<_>>().join("\n");
        assert!(screen.contains("Navigate"), "{screen}");
        for g in HELP {
            assert!(
                !screen.contains(g.icon),
                "ascii help still draws {:?}",
                g.icon
            );
        }
    }

    fn guest(id: u64, name: &str, status: crate::web::GuestStatus) -> crate::web::GuestInfo {
        crate::web::GuestInfo {
            id,
            name: name.into(),
            ip: [192, 168, 1, 23].into(),
            agent: "Chrome on Windows".into(),
            status,
            online: 1,
        }
    }

    #[test]
    fn lan_panel_and_join_prompt_render() {
        use crate::web::{GuestStatus, KeyInfo, KeyState, Role};
        let mut a = app();
        a.lan_url = Some("https://192.168.1.5:7878".into());
        a.lan_key = Some(KeyInfo {
            code: "K7F-29Q".into(),
            state: KeyState::Live,
            expires: Instant::now() + std::time::Duration::from_secs(600),
        });
        a.lan_guests = vec![
            guest(1, "Taha", GuestStatus::Waiting),
            guest(2, "", GuestStatus::In(Role::Control)),
        ];
        a.lan_panel = Some(0);
        let t = screen(&draw(&a, 120, 30), 30);
        assert!(t.contains("https://192.168.1.5:7878"));
        assert!(t.contains("K7F-29Q") && t.contains("left"));
        assert!(t.contains("Taha") && t.contains("waiting"));
        assert!(t.contains("full control") && t.contains("online"));
        // a used key says how to get another
        a.lan_key.as_mut().unwrap().state = KeyState::Used;
        let t = screen(&draw(&a, 120, 30), 30);
        assert!(t.contains("n makes a new one"));
        // small terminals don't panic
        draw(&a, 40, 10);

        a.lan_panel = None;
        a.join_prompt = Some(crate::app::JoinPrompt {
            guest: guest(1, "Taha", GuestStatus::Waiting),
            cursor: 0,
            since: Instant::now(),
        });
        let t = screen(&draw(&a, 120, 30), 30);
        assert!(t.contains("\"Taha\" wants to join"));
        assert!(t.contains("192.168.1.23") && t.contains("Chrome on Windows"));
        assert!(t.contains("Watch only") && t.contains("Full control") && t.contains("Deny"));
        draw(&a, 40, 10);
    }

    fn screen(t: &Terminal<TestBackend>, h: u16) -> String {
        (0..h).map(|y| row(t, y)).collect::<Vec<_>>().join("\n")
    }

    /// A terminal in slot `slot` with no real process behind it.
    fn put(a: &mut App, slot: usize, name: &str, out: &[&str], state: RunState) {
        let mut s = Session::new(100 + slot as u64, name, "desc");
        for l in out {
            s.push(*l);
        }
        s.state = state;
        a.slots[slot] = Some(s);
    }

    #[test]
    fn category_bar_list_and_card() {
        let mut a = app();
        let t = draw(&a, 140, 40);
        let sc = screen(&t, 40);
        assert!(sc.contains("All 45"), "{sc}");
        assert!(sc.contains("OSINT"), "{sc}");
        // the card names the selected tool's main category
        let first = &a.tools[a.selected];
        assert!(sc.contains(&registry::category_label(first.primary())));
        // the active tab scrolls into view even on a narrow screen
        a.cat = a.cat_tabs.len() - 1;
        let t = draw(&a, 80, 24);
        assert!(row(&t, 1).contains("Wordlists"), "{}", row(&t, 1));
        // an empty category under a filter says where the matches are
        a.cat = a
            .cat_tabs
            .iter()
            .position(|t| t.label() == "Stego")
            .unwrap();
        a.filter = "nmap".into();
        let t = draw(&a, 120, 30);
        assert!(screen(&t, 30).contains("in All"));
    }

    #[test]
    fn terminals_tabs_single_and_grid() {
        let mut a = app();
        put(
            &mut a,
            0,
            "nmap",
            &["22/tcp open"],
            RunState::Exited(Some(0)),
        );
        put(&mut a, 2, "ffuf", &["[err] boom"], RunState::Killed);
        a.slots[2].as_mut().unwrap().unread = true;
        a.focus = 1;
        let t = draw(&a, 140, 30);
        let sc = screen(&t, 30);
        for k in [
            "0 console",
            "1 nmap",
            "3 ffuf",
            "4 +",
            "22/tcp open",
            "exit 0",
            "x close",
        ] {
            assert!(sc.contains(k), "missing {k:?}:\n{sc}");
        }
        assert!(
            sc.contains('✓') && sc.contains('✗') && sc.contains('•'),
            "{sc}"
        );
        a.grid = true;
        let t = draw(&a, 140, 30);
        let sc = screen(&t, 30);
        for k in ["22/tcp open", "[err] boom", "2 free", "stopped"] {
            assert!(sc.contains(k), "grid missing {k:?}:\n{sc}");
        }
        for (w, h) in [(80, 24), (40, 10), (10, 4)] {
            draw(&a, w, h);
        }
    }

    #[test]
    fn slot_pick_footer_and_confirm() {
        let mut a = app();
        for i in 0..SLOTS {
            put(&mut a, i, &format!("t{i}"), &[], RunState::Running);
        }
        a.mode = Mode::SlotPick;
        let t = draw(&a, 160, 20);
        let footer = row(&t, 19);
        assert!(
            footer.contains("terminals busy") && footer.contains("Esc cancel"),
            "{footer}"
        );
        a.mode = Mode::Browsing;
        a.confirm = Some(crate::app::Confirm::Quit);
        draw(&a, 100, 20);
    }

    #[test]
    fn no_color_selection_is_reverse_video() {
        let mut a = app();
        a.mono = true;
        let t = draw(&a, 140, 40);
        let buf = t.backend().buffer();
        // row 3 = first list row (header, categories, list border above it);
        // x = 5 is inside the bar, past the glow on the left
        assert!(
            buf[(5, 3)].modifier.contains(Modifier::REVERSED),
            "{}",
            row(&t, 3)
        );
    }

    #[test]
    fn missing_tools_are_not_drawn_as_errors() {
        let mut a = app();
        a.status = vec![ToolStatus::Missing; a.tools.len()];
        let err = crate::theme::Palette::of(a.settings.theme).err;
        let t = draw(&a, 140, 40);
        let buf = t.backend().buffer();
        // the left pane: tool list and card
        for y in 2..39 {
            for x in 0..50 {
                assert_ne!(buf[(x, y)].fg, err, "red at ({x},{y}): {}", row(&t, y));
            }
        }
    }

    #[test]
    fn categories_get_their_own_row() {
        let mut a = app();
        let t = draw(&a, 140, 40);
        assert!(row(&t, 1).contains("All 45"), "{}", row(&t, 1));
        assert!(!row(&t, 1).contains("console"), "{}", row(&t, 1));
        assert!(row(&t, 2).contains("console"), "{}", row(&t, 2));
        // chips hidden on the right are flagged
        let t = draw(&a, 80, 24);
        assert!(row(&t, 1).trim_end().ends_with('›'), "{}", row(&t, 1));
        // and on the left once scrolled to the end
        a.cat = a.cat_tabs.len() - 1;
        let t = draw(&a, 80, 24);
        assert!(row(&t, 1).trim_start().starts_with('‹'), "{}", row(&t, 1));
    }

    #[test]
    fn toasts_and_tour_render_at_any_size() {
        let mut a = app();
        a.toast(ToastKind::Ok, "✓ nmap finished in terminal 1");
        a.toast(ToastKind::Err, "✗ ffuf exited 1");
        let t = draw(&a, 140, 40);
        let sc = screen(&t, 40);
        assert!(
            sc.contains("nmap finished") && sc.contains("ffuf exited"),
            "{sc}"
        );
        for step in 0..TOUR_STEPS {
            a.tour = Some(step);
            let t = draw(&a, 140, 40);
            assert!(screen(&t, 40).contains("welcome to winsploit"));
        }
        for (w, h) in [(80, 24), (40, 10), (10, 4)] {
            draw(&a, w, h);
        }
    }

    #[test]
    fn long_rows_keep_their_badge_inside_the_pane() {
        let mut a = app();
        a.status = vec![ToolStatus::Missing; a.tools.len()];
        let t = draw(&a, 140, 40);
        // the left pane is 50 wide: its right border is at x = 49
        for y in 4..30 {
            let r: String = row(&t, y).chars().take(50).collect();
            // the full badge is there, inside the border
            assert!(r.contains("+ get ") && r.ends_with('│'), "row {y}: {r}");
        }
    }

    #[test]
    fn flags_are_highlighted_even_when_wrapped() {
        let mut a = app();
        // a narrow terminal: the flag wraps onto the next row
        let pad = "x".repeat(60);
        put(
            &mut a,
            0,
            "strings",
            &[&format!("{pad} picoCTF{{wr4pped_r0w_fl4g}} tail")],
            RunState::Exited(Some(0)),
        );
        a.focus = 1;
        let t = draw(&a, 140, 30);
        let buf = t.backend().buffer();
        let mut underlined = String::new();
        for y in 0..30 {
            for x in 0..140 {
                let cell = &buf[(x, y)];
                if cell.modifier.contains(Modifier::UNDERLINED) && x > 50 {
                    underlined.push_str(cell.symbol());
                }
            }
        }
        assert!(
            underlined.contains("picoCTF{wr4pped_r0w_fl4g}"),
            "{underlined}"
        );
        assert!(!underlined.contains("tail"));
    }

    #[test]
    fn recipe_picker_shows_filled_args_and_gaps() {
        let mut a = app();
        a.selected = a.tools.iter().position(|t| t.name == "nmap").unwrap();
        a.recipe_view = Some(0);
        let sc = screen(&draw(&a, 160, 30), 30);
        assert!(
            sc.contains("nmap · recipes") && sc.contains("custom"),
            "{sc}"
        );
        assert!(sc.contains("asks for target"), "{sc}");
    }

    #[test]
    fn header_and_picker_show_the_challenge() {
        let root = std::env::temp_dir()
            .join("winsploit-ui-tests")
            .join("challenges");
        let _ = std::fs::remove_dir_all(&root);
        let ws =
            crate::workspace::Workspace::create(&root, "picoCTF 2026", "rev 1", "10.0.0.5", "")
                .unwrap();
        let mut a = app();
        a.ctf_root = root.clone();
        a.challenge = Some(ws);
        a.challenge_list = crate::workspace::list(&root);
        let t = draw(&a, 140, 30);
        assert!(
            row(&t, 0).contains("picoCTF 2026 / rev 1"),
            "{}",
            row(&t, 0)
        );
        assert!(row(&t, 0).contains("10.0.0.5"));
        a.challenge_view = Some(0);
        let sc = screen(&draw(&a, 140, 30), 30);
        assert!(sc.contains("challenges") && sc.contains("active"), "{sc}");
        // nothing yet: say where they'll go
        a.challenge = None;
        a.challenge_list.clear();
        assert!(screen(&draw(&a, 140, 30), 30).contains("no challenges yet"));
    }

    #[test]
    fn flags_overlay_lists_newest_first() {
        let mut a = app();
        for (t, known) in [("flag{first}", true), ("HACK{s3c0nd}", false)] {
            a.flags.push(crate::app::Flag {
                text: t.into(),
                known,
                source: "nmap".into(),
                at: "2026-11-07 17:04:21 UTC".into(),
            });
        }
        a.flags_view = Some(0);
        let t = draw(&a, 120, 30);
        let sc = screen(&t, 30);
        let first = sc.find("HACK{s3c0nd}").expect("newest listed");
        let second = sc.find("flag{first}").expect("oldest listed");
        assert!(first < second, "newest first:\n{sc}");
        assert!(sc.contains("likely") && sc.contains("17:04") && sc.contains("flags · 2"));
        // and an empty list explains itself
        a.flags.clear();
        assert!(screen(&draw(&a, 120, 30), 30).contains("no flags yet"));
    }

    #[test]
    fn hash_prompt_label() {
        let mut a = app();
        a.mode = Mode::HashInput;
        let t = draw(&a, 100, 20);
        assert!(row(&t, 19).contains("hash ▸"));
    }

    #[test]
    fn appearance_overlay_renders_every_theme_and_layout() {
        use crate::app::Appearance;
        use crate::theme::{Backdrop, Borders, GlyphSet, OverlayPos, ThemeName};
        let mut a = app();
        a.appearance = Some(Appearance {
            cursor: 0,
            saved: a.settings.clone(),
        });
        for theme in ThemeName::ALL {
            for overlay in [OverlayPos::Docked, OverlayPos::Centered] {
                a.settings.theme = theme;
                a.settings.overlay = overlay;
                for (w, h) in [(140, 40), (80, 24), (40, 10), (10, 4)] {
                    draw(&a, w, h);
                }
            }
        }
        a.settings.theme = ThemeName::Dark;
        a.settings.overlay = OverlayPos::Centered;
        let t = draw(&a, 140, 40);
        let screen: String = (0..40).map(|y| row(&t, y)).collect::<Vec<_>>().join(
            "
",
        );
        for k in [
            "appearance",
            "preview",
            "Dark mode",
            "Borders",
            "Enter save",
        ] {
            assert!(screen.contains(k), "appearance overlay is missing {k:?}");
        }
        // ascii everything: no box-drawing or braille left in the dashboard
        a.appearance = None;
        a.settings.borders = Borders::Ascii;
        a.settings.glyphs = GlyphSet::Ascii;
        a.settings.backdrop = Backdrop::None;
        let t = draw(&a, 100, 20);
        let screen: String = (0..20).map(|y| row(&t, y)).collect::<Vec<_>>().join(
            "
",
        );
        for bad in ["╭", "─", "│", "●", "○", "▸"] {
            assert!(!screen.contains(bad), "ascii mode still draws {bad:?}");
        }
        assert!(screen.contains("+--"), "{screen}");
    }

    #[test]
    fn docked_overlay_keeps_the_footer_and_dims_the_rest() {
        let mut a = app();
        a.show_help = true;
        let t = draw(&a, 120, 40);
        let buf = t.backend().buffer();
        // the footer row is still the footer
        assert!(row(&t, 39).contains("quit"), "{}", row(&t, 39));
        // the header behind the overlay is faded
        assert!(buf[(1, 0)].modifier.contains(Modifier::DIM));
    }

    // ------------------------------------------------------------ snapshots

    /// A ratatui colour as CSS, for the snapshot pages (Windows Terminal's
    /// Campbell scheme for the 16 ANSI colours).
    fn css(c: Color, fallback: &str) -> String {
        let named = |i: usize| {
            [
                "#0c0c0c", "#c50f1f", "#13a10e", "#c19c00", "#0037da", "#881798", "#3a96dd",
                "#cccccc", "#767676", "#e74856", "#16c60c", "#f9f1a5", "#3b78ff", "#b4009e",
                "#61d6d6", "#f2f2f2",
            ][i]
                .to_string()
        };
        match c {
            Color::Reset => fallback.to_string(),
            Color::Rgb(r, g, b) => format!("#{r:02x}{g:02x}{b:02x}"),
            Color::Black => named(0),
            Color::Red => named(1),
            Color::Green => named(2),
            Color::Yellow => named(3),
            Color::Blue => named(4),
            Color::Magenta => named(5),
            Color::Cyan => named(6),
            Color::Gray => named(7),
            Color::DarkGray => named(8),
            Color::LightRed => named(9),
            Color::LightGreen => named(10),
            Color::LightYellow => named(11),
            Color::LightBlue => named(12),
            Color::LightMagenta => named(13),
            Color::LightCyan => named(14),
            Color::White => named(15),
            Color::Indexed(i) if i < 16 => named(i as usize),
            _ => fallback.to_string(),
        }
    }

    /// The test terminal as an HTML page: one span per run of same-styled
    /// cells, in Cascadia Mono like Windows Terminal.
    fn snapshot_html(t: &Terminal<TestBackend>, title: &str) -> String {
        const FG: &str = "#cccccc";
        const BG: &str = "#0c0c0c";
        let buf = t.backend().buffer();
        let mut out = format!(
            "<!doctype html><meta charset=utf-8><title>{title}</title><style>\
             body{{margin:0;background:{BG}}}\
             pre{{margin:0;padding:8px;font:15px/18px 'Cascadia Mono','Cascadia Code',Consolas,monospace;color:{FG}}}\
             span{{white-space:pre}}</style><pre>"
        );
        for y in 0..buf.area.height {
            let mut x = 0;
            while x < buf.area.width {
                let c = &buf[(x, y)];
                let (mut fg, mut bg) = (css(c.fg, FG), css(c.bg, BG));
                if c.modifier.contains(Modifier::REVERSED) {
                    std::mem::swap(&mut fg, &mut bg);
                }
                let mut style = format!("color:{fg};background:{bg}");
                if c.modifier.contains(Modifier::BOLD) {
                    style.push_str(";font-weight:700");
                }
                if c.modifier.contains(Modifier::DIM) {
                    style.push_str(";opacity:.55");
                }
                if c.modifier.contains(Modifier::ITALIC) {
                    style.push_str(";font-style:italic");
                }
                if c.modifier.contains(Modifier::UNDERLINED) {
                    style.push_str(";text-decoration:underline");
                }
                let mut text = String::new();
                while x < buf.area.width {
                    let d = &buf[(x, y)];
                    if d.fg != c.fg || d.bg != c.bg || d.modifier != c.modifier {
                        break;
                    }
                    if !d.skip {
                        text.push_str(d.symbol());
                    }
                    x += 1;
                }
                let text = text
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                out.push_str(&format!("<span style=\"{style}\">{text}</span>"));
            }
            out.push('\n');
        }
        out.push_str("</pre>");
        out
    }

    /// Renders the main screens to HTML pages for a UX review:
    ///   $env:WINSPLOIT_SNAP_DIR = "C:\tmp\snaps"; cargo test snapshots -- --ignored
    /// then open (or screenshot) the pages in a browser.
    #[test]
    #[ignore = "writes HTML files; run by hand for screenshots"]
    fn snapshots() {
        use crate::app::Appearance;
        use crate::theme::{Borders, GlyphSet, Settings, ThemeName};
        let Some(dir) = std::env::var_os("WINSPLOIT_SNAP_DIR").map(PathBuf::from) else {
            return;
        };
        std::fs::create_dir_all(&dir).unwrap();
        let save = |name: &str, t: &Terminal<TestBackend>| {
            std::fs::write(dir.join(format!("{name}.html")), snapshot_html(t, name)).unwrap();
        };
        let splash = |s: &Settings, w: u16, h: u16| {
            let mut t = Terminal::new(TestBackend::new(w, h)).unwrap();
            t.draw(|f| crate::banner::render_splash(f, 0, &s.theme(), Some(0)))
                .unwrap();
            t
        };
        let (w, h) = (130, 38);
        for theme in [ThemeName::Frost, ThemeName::Dark, ThemeName::DarkAnsi] {
            let s = Settings {
                theme,
                ..Default::default()
            };
            let name = format!("{theme:?}").to_lowercase();
            save(&format!("splash-{name}"), &splash(&s, w, h));
            let mut a = app();
            a.settings = s;
            save(&format!("dashboard-{name}"), &draw(&a, w, h));
        }
        let ascii = Settings {
            borders: Borders::Ascii,
            glyphs: GlyphSet::Ascii,
            ..Default::default()
        };
        save("splash-ascii", &splash(&ascii, w, h));
        save("splash-small", &splash(&Settings::default(), 80, 24));
        let mut a = app();
        a.settings = ascii;
        save("dashboard-ascii", &draw(&a, w, h));

        let mut a = app();
        save("dashboard-small", &draw(&a, 80, 24));
        a.appearance = Some(Appearance {
            cursor: 0,
            saved: a.settings.clone(),
        });
        save("appearance", &draw(&a, w, h));
        a.appearance = None;
        a.show_help = true;
        save("help", &draw(&a, w, h));
        a.show_help = false;
        a.tour = Some(0);
        save("tour", &draw(&a, w, h));
    }
}
