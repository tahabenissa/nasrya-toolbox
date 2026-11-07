//! ASCII-art banner and splash screen for winsploit.

use ratatui::layout::{Alignment, Constraint, Direction, Layout};
use ratatui::style::{Color, Modifier, Style};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, BorderType, Borders, Paragraph};
use ratatui::Frame;

/// "WINSPLOIT" in the ANSI-Shadow figlet style. Used by main.rs help text too.
pub const LOGO: &str = r#"
 ██╗    ██╗ ██╗ ███╗   ██╗ ███████╗ ██████╗  ██╗       ██████╗  ██╗ ████████╗
 ██║    ██║ ██║ ████╗  ██║ ██╔════╝ ██╔══██╗ ██║      ██╔═══██╗ ██║ ╚══██╔══╝
 ██║ █╗ ██║ ██║ ██╔██╗ ██║ ███████╗ ██████╔╝ ██║      ██║   ██║ ██║    ██║
 ██║███╗██║ ██║ ██║╚██╗██║ ╚════██║ ██╔═══╝  ██║      ██║   ██║ ██║    ██║
 ╚███╔███╔╝ ██║ ██║ ╚████║ ███████║ ██║      ███████╗ ╚██████╔╝ ██║    ██║
  ╚══╝╚══╝  ╚═╝ ╚═╝  ╚═══╝ ╚══════╝ ╚═╝      ╚══════╝  ╚═════╝  ╚═╝    ╚═╝
"#;

/// Linear interpolation between two bytes.
fn lerp(a: u8, b: u8, t: f32) -> u8 {
    let t = t.clamp(0.0, 1.0);
    (a as f32 + (b as f32 - a as f32) * t).round() as u8
}

/// A cyan -> electric-blue -> magenta gradient, sampled at `t` in [0, 1].
fn gradient(t: f32) -> (u8, u8, u8) {
    // Three stops; find which segment `t` falls in and lerp within it.
    const STOPS: [(u8, u8, u8); 3] = [
        (0, 245, 230),  // bright cyan
        (70, 120, 255), // electric blue
        (205, 90, 255), // magenta
    ];
    let t = t.clamp(0.0, 1.0);
    let scaled = t * (STOPS.len() as f32 - 1.0);
    let i = (scaled.floor() as usize).min(STOPS.len() - 2);
    let local = scaled - i as f32;
    let (ar, ag, ab) = STOPS[i];
    let (br, bg, bb) = STOPS[i + 1];
    (
        lerp(ar, br, local),
        lerp(ag, bg, local),
        lerp(ab, bb, local),
    )
}

/// Mix a colour toward white by `amount` (0..=1) for a glowing highlight.
fn glow(c: (u8, u8, u8), amount: f32) -> (u8, u8, u8) {
    (
        lerp(c.0, 255, amount),
        lerp(c.1, 255, amount),
        lerp(c.2, 255, amount),
    )
}

/// Render the full-screen splash shown at startup.
pub fn render_splash(f: &mut Frame, tick: u64) {
    let area = f.area();

    // Gently pulsing double border around the whole screen.
    let border_pulse = ((tick / 4) % 6) as f32 / 5.0;
    let bc = glow(
        (0, 200, 220),
        0.25 * (1.0 - (border_pulse - 0.5).abs() * 2.0).max(0.0),
    );
    let outer = Block::default()
        .borders(Borders::ALL)
        .border_type(BorderType::Double)
        .border_style(Style::default().fg(Color::Rgb(bc.0, bc.1, bc.2)));
    let inner = outer.inner(area);
    f.render_widget(outer, area);

    let logo_lines: Vec<&str> = LOGO.lines().filter(|l| !l.is_empty()).collect();
    let logo_h = logo_lines.len() as u16;

    // Block layout: pad / logo / gap / subtitle / gap / prompt / pad.
    let total = logo_h + 1 + 1 + 1 + 1;
    let top_pad = inner.height.saturating_sub(total) / 2;

    let rows = Layout::default()
        .direction(Direction::Vertical)
        .constraints([
            Constraint::Length(top_pad),
            Constraint::Length(logo_h),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Length(1),
            Constraint::Min(0),
        ])
        .split(inner);

    // A scanning glow sweeps down the logo, line by line.
    let n = logo_lines.len().max(1);
    let glow_idx = (tick / 2) % n as u64;
    let logo: Vec<Line> = logo_lines
        .iter()
        .enumerate()
        .map(|(i, l)| {
            let t = if n > 1 {
                i as f32 / (n as f32 - 1.0)
            } else {
                0.0
            };
            let mut c = gradient(t);
            if i as u64 == glow_idx {
                c = glow(c, 0.5);
            }
            Line::from(Span::styled(
                l.to_string(),
                Style::default()
                    .fg(Color::Rgb(c.0, c.1, c.2))
                    .add_modifier(Modifier::BOLD),
            ))
            .alignment(Alignment::Center)
        })
        .collect();
    f.render_widget(Paragraph::new(logo), rows[1]);

    // Subtitle.
    let subtitle = Line::from(vec![
        Span::styled(
            "native-windows security toolbox",
            Style::default().fg(Color::Rgb(150, 170, 190)),
        ),
        Span::styled("  ·  ", Style::default().fg(Color::Rgb(90, 100, 120))),
        Span::styled(
            "command runner",
            Style::default().fg(Color::Rgb(150, 170, 190)),
        ),
    ])
    .alignment(Alignment::Center);
    f.render_widget(Paragraph::new(subtitle), rows[3]);

    // Blinking "press any key" prompt.
    let on = (tick / 5).is_multiple_of(2);
    let prompt = if on {
        Line::from(vec![
            Span::styled("▸ ", Style::default().fg(Color::Rgb(120, 220, 255))),
            Span::styled(
                "press any key to enter",
                Style::default()
                    .fg(Color::Rgb(230, 240, 150))
                    .add_modifier(Modifier::BOLD),
            ),
            Span::styled(" ◂", Style::default().fg(Color::Rgb(120, 220, 255))),
        ])
    } else {
        Line::from(Span::styled(
            "  press any key to enter  ",
            Style::default().fg(Color::Rgb(70, 80, 95)),
        ))
    }
    .alignment(Alignment::Center);
    f.render_widget(Paragraph::new(prompt), rows[5]);
}

/// A compact one-line wordmark for the main dashboard header.
pub fn wordmark() -> Line<'static> {
    Line::from(vec![
        Span::styled(
            " winsploit ",
            Style::default()
                .fg(Color::Black)
                .bg(Color::Cyan)
                .add_modifier(Modifier::BOLD),
        ),
        Span::styled("  ", Style::default()),
        Span::styled(
            "native-windows security toolbox",
            Style::default().fg(Color::Rgb(140, 160, 185)),
        ),
        Span::styled("  ·  ", Style::default().fg(Color::Rgb(80, 90, 110))),
        Span::styled(
            "command runner",
            Style::default().fg(Color::Rgb(100, 120, 145)),
        ),
    ])
}
