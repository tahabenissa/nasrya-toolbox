//! Appearance: colour themes, "font" (glyph set, borders, weight) and how
//! overlays are drawn. Picked live in the `t` overlay and saved to
//! `%APPDATA%\winsploit\settings.yaml`.
//!
//! The default, Frost, is winsploit's own look: the web view's palette and
//! the Iso cube logo's colours. The others follow Claude Code's `/theme`
//! choices (dark, light, the colour-blind-friendly variants and the ANSI-only
//! ones) so the two feel at home side by side in one terminal. The terminal still owns the actual
//! typeface; what we control is which glyphs we draw, the border line style
//! and whether emphasis is bold.

use std::path::PathBuf;

use ratatui::style::{Color, Modifier, Style};
use ratatui::symbols::border;
use serde::{Deserialize, Serialize};

// ---------------------------------------------------------------- settings

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum ThemeName {
    /// winsploit's own look: slate and icy blue, like the web view's default
    #[default]
    Frost,
    Dark,
    Light,
    DarkDaltonized,
    LightDaltonized,
    DarkAnsi,
    LightAnsi,
    /// winsploit's original cyan look
    Cyan,
}

impl ThemeName {
    pub const ALL: [ThemeName; 8] = [
        ThemeName::Frost,
        ThemeName::Dark,
        ThemeName::Light,
        ThemeName::DarkDaltonized,
        ThemeName::LightDaltonized,
        ThemeName::DarkAnsi,
        ThemeName::LightAnsi,
        ThemeName::Cyan,
    ];

    pub fn label(self) -> &'static str {
        match self {
            ThemeName::Frost => "Winsploit Frost",
            ThemeName::Dark => "Dark mode",
            ThemeName::Light => "Light mode",
            ThemeName::DarkDaltonized => "Dark mode (colour-blind friendly)",
            ThemeName::LightDaltonized => "Light mode (colour-blind friendly)",
            ThemeName::DarkAnsi => "Dark mode (ANSI colours only)",
            ThemeName::LightAnsi => "Light mode (ANSI colours only)",
            ThemeName::Cyan => "Winsploit classic (cyan)",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Borders {
    #[default]
    Rounded,
    Plain,
    Double,
    Thick,
    /// `+ - |`: for raster fonts without box-drawing characters
    Ascii,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum GlyphSet {
    #[default]
    Unicode,
    /// plain ASCII icons and spinner, for fonts missing ● ◐ ⠋ ▸
    Ascii,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OverlayPos {
    /// a full-width panel above the footer, like Claude Code's pickers
    #[default]
    Docked,
    Centered,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Backdrop {
    /// fade the dashboard behind an open overlay
    #[default]
    Dim,
    None,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default, rename_all = "kebab-case")]
pub struct Settings {
    pub theme: ThemeName,
    pub borders: Borders,
    pub glyphs: GlyphSet,
    pub bold: bool,
    pub overlay: OverlayPos,
    pub backdrop: Backdrop,
    /// the first-run welcome tour was finished or skipped
    pub tour_done: bool,
    /// Which AI the `a` agent talks to.
    pub agent: crate::agent::Provider,
    /// Per-provider model overrides, keyed by `Provider::short()`. An entry
    /// here replaces that provider's default model; absent = use the default.
    pub agent_models: std::collections::BTreeMap<String, String>,
    /// Check GitHub once a day for a newer release (toast only, never downloads).
    pub update_check: bool,
    /// Ring the terminal bell when a long run finishes while unfocused (#113).
    pub notify_sound: bool,
    /// How much of a run is kept out of the pane's `$` line and the run
    /// log: `off`, `commands` (default) or `all` (#70, see redact.rs).
    pub redact: crate::redact::Level,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: ThemeName::default(),
            borders: Borders::default(),
            glyphs: GlyphSet::default(),
            bold: true,
            overlay: OverlayPos::default(),
            backdrop: Backdrop::default(),
            tour_done: false,
            agent: crate::agent::Provider::default(),
            agent_models: std::collections::BTreeMap::new(),
            update_check: true,
            notify_sound: false,
            redact: crate::redact::Level::default(),
        }
    }
}

/// One editable row of the appearance overlay.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Field {
    Theme,
    Borders,
    Glyphs,
    Bold,
    Overlay,
    Backdrop,
}

impl Field {
    pub const ALL: [Field; 6] = [
        Field::Theme,
        Field::Borders,
        Field::Glyphs,
        Field::Bold,
        Field::Overlay,
        Field::Backdrop,
    ];

    pub fn label(self) -> &'static str {
        match self {
            Field::Theme => "Theme",
            Field::Borders => "Borders",
            Field::Glyphs => "Glyphs",
            Field::Bold => "Bold text",
            Field::Overlay => "Overlays",
            Field::Backdrop => "Backdrop",
        }
    }
}

/// Step through `all` from `cur` by `dir` (wrapping).
fn cycle<T: Copy + PartialEq>(all: &[T], cur: T, dir: i32) -> T {
    let i = all.iter().position(|x| *x == cur).unwrap_or(0) as i32;
    let n = all.len() as i32;
    all[(i + dir).rem_euclid(n) as usize]
}

impl Settings {
    /// The value shown for `field` in the overlay.
    pub fn value(&self, field: Field) -> &'static str {
        match field {
            Field::Theme => self.theme.label(),
            Field::Borders => match self.borders {
                Borders::Rounded => "rounded  ╭─╮",
                Borders::Plain => "plain  ┌─┐",
                Borders::Double => "double  ╔═╗",
                Borders::Thick => "thick  ┏━┓",
                Borders::Ascii => "ascii  +-+",
            },
            Field::Glyphs => match self.glyphs {
                GlyphSet::Unicode => "unicode  ● ○ ◐ ⠋ ▸",
                GlyphSet::Ascii => "ascii  * o ~ | >",
            },
            Field::Bold => {
                if self.bold {
                    "on"
                } else {
                    "off"
                }
            }
            Field::Overlay => match self.overlay {
                OverlayPos::Docked => "docked above the footer",
                OverlayPos::Centered => "centered",
            },
            Field::Backdrop => match self.backdrop {
                Backdrop::Dim => "dim the dashboard",
                Backdrop::None => "leave it as is",
            },
        }
    }

    /// Change `field` one step left (-1) or right (+1).
    pub fn step(&mut self, field: Field, dir: i32) {
        match field {
            Field::Theme => self.theme = cycle(&ThemeName::ALL, self.theme, dir),
            Field::Borders => {
                use Borders::*;
                self.borders = cycle(&[Rounded, Plain, Double, Thick, Ascii], self.borders, dir)
            }
            Field::Glyphs => {
                self.glyphs = cycle(&[GlyphSet::Unicode, GlyphSet::Ascii], self.glyphs, dir)
            }
            Field::Bold => self.bold = !self.bold,
            Field::Overlay => {
                self.overlay = cycle(
                    &[OverlayPos::Docked, OverlayPos::Centered],
                    self.overlay,
                    dir,
                )
            }
            Field::Backdrop => {
                self.backdrop = cycle(&[Backdrop::Dim, Backdrop::None], self.backdrop, dir)
            }
        }
    }

    pub fn path() -> Option<PathBuf> {
        std::env::var_os("APPDATA")
            .map(|a| PathBuf::from(a).join("winsploit").join("settings.yaml"))
    }

    /// Saved settings, or the defaults if there are none / they don't parse.
    pub fn load() -> Self {
        Self::path()
            .and_then(|p| std::fs::read_to_string(p).ok())
            .and_then(|t| serde_yaml::from_str(&t).ok())
            .unwrap_or_default()
    }

    pub fn save(&self) -> anyhow::Result<PathBuf> {
        let path = Self::path().ok_or_else(|| anyhow::anyhow!("%APPDATA% is not set"))?;
        if let Some(dir) = path.parent() {
            std::fs::create_dir_all(dir)?;
        }
        let body = format!(
            "# winsploit appearance — edit here or press `t` in the dashboard\n{}",
            serde_yaml::to_string(self)?
        );
        std::fs::write(&path, body)?;
        Ok(path)
    }

    pub fn theme(&self) -> Theme {
        Theme {
            pal: Palette::of(self.theme),
            glyph: if self.glyphs == GlyphSet::Ascii {
                &ASCII
            } else {
                &UNICODE
            },
            border: match self.borders {
                Borders::Rounded => border::ROUNDED,
                Borders::Plain => border::PLAIN,
                Borders::Double => border::DOUBLE,
                Borders::Thick => border::THICK,
                Borders::Ascii => ASCII_BORDER,
            },
            bold: self.bold,
            overlay: self.overlay,
            backdrop: self.backdrop,
            mono: no_color(),
        }
    }
}

/// `NO_COLOR` set to anything non-empty: crossterm then drops every colour,
/// backgrounds included, so state must also show through modifiers.
pub fn no_color() -> bool {
    std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty())
}

// ---------------------------------------------------------------- palettes

/// Semantic colours. Every colour in the UI comes from one of these.
#[derive(Clone, Copy, Debug)]
pub struct Palette {
    /// brand colour: titles, key hints, the selection bar
    pub accent: Color,
    /// idle pane borders
    pub border: Color,
    pub ok: Color,
    pub err: Color,
    pub warn: Color,
    /// secondary highlight: stdin echo, scroll hint (Claude's lavender)
    pub info: Color,
    pub dim: Color,
    pub fg: Color,
    /// text drawn on an accent / warn background
    pub ink: Color,
    /// background of the current find-in-output match
    pub hit_bg: Color,
    /// three stops for the splash logo; None = solid accent (ANSI themes)
    pub gradient: Option<[(u8, u8, u8); 3]>,
}

const fn rgb(r: u8, g: u8, b: u8) -> Color {
    Color::Rgb(r, g, b)
}

/// Claude's terracotta.
const CLAUDE: Color = rgb(215, 119, 87);

impl Palette {
    pub fn of(name: ThemeName) -> Palette {
        let dark = Palette {
            accent: CLAUDE,
            border: rgb(118, 82, 70),
            ok: rgb(78, 186, 101),
            err: rgb(255, 107, 128),
            warn: rgb(255, 193, 7),
            info: rgb(177, 185, 249),
            dim: rgb(140, 140, 140),
            fg: rgb(235, 235, 235),
            ink: rgb(20, 20, 20),
            hit_bg: rgb(90, 55, 40),
            gradient: Some([(244, 176, 140), (215, 119, 87), (177, 120, 190)]),
        };
        let light = Palette {
            accent: rgb(196, 98, 66),
            border: rgb(214, 176, 162),
            ok: rgb(44, 122, 57),
            err: rgb(171, 43, 63),
            warn: rgb(150, 108, 30),
            info: rgb(87, 105, 247),
            dim: rgb(110, 110, 110),
            fg: rgb(25, 25, 25),
            ink: rgb(255, 255, 255),
            hit_bg: rgb(252, 226, 200),
            gradient: Some([(226, 140, 100), (196, 98, 66), (120, 80, 170)]),
        };
        match name {
            // the web view's Frost palette (src/web/app.css, dark mode) and
            // the Iso cube logo's faces for the splash gradient
            ThemeName::Frost => Palette {
                accent: rgb(137, 194, 210),
                border: rgb(73, 98, 110),
                ok: rgb(92, 199, 119),
                err: rgb(255, 122, 140),
                warn: rgb(240, 185, 58),
                info: rgb(177, 185, 249),
                dim: rgb(132, 138, 148),
                fg: rgb(233, 234, 237),
                ink: rgb(12, 25, 29),
                hit_bg: rgb(34, 69, 79),
                gradient: Some([(188, 221, 230), (137, 194, 210), (70, 160, 185)]),
            },
            ThemeName::Dark => dark,
            ThemeName::Light => light,
            // blue / red instead of green / red: distinguishable with
            // deuteranopia and protanopia
            ThemeName::DarkDaltonized => Palette {
                ok: rgb(51, 153, 255),
                err: rgb(255, 102, 102),
                ..dark
            },
            ThemeName::LightDaltonized => Palette {
                ok: rgb(0, 102, 204),
                err: rgb(204, 0, 0),
                ..light
            },
            ThemeName::DarkAnsi => Palette {
                accent: Color::LightRed,
                border: Color::DarkGray,
                ok: Color::Green,
                err: Color::Red,
                warn: Color::Yellow,
                info: Color::LightBlue,
                dim: Color::DarkGray,
                fg: Color::Reset,
                ink: Color::Black,
                hit_bg: Color::DarkGray,
                gradient: None,
            },
            ThemeName::LightAnsi => Palette {
                accent: Color::Red,
                border: Color::Gray,
                ok: Color::Green,
                err: Color::LightRed,
                warn: Color::Yellow,
                info: Color::Blue,
                dim: Color::DarkGray,
                fg: Color::Reset,
                ink: Color::White,
                hit_bg: Color::Gray,
                gradient: None,
            },
            ThemeName::Cyan => Palette {
                accent: rgb(80, 210, 230),
                border: rgb(45, 120, 135),
                ok: rgb(120, 230, 150),
                err: rgb(255, 110, 120),
                warn: rgb(240, 215, 120),
                info: rgb(80, 210, 230),
                dim: rgb(95, 110, 130),
                fg: rgb(215, 225, 235),
                ink: rgb(10, 14, 20),
                hit_bg: rgb(70, 60, 10),
                gradient: Some([(0, 245, 230), (70, 120, 255), (205, 90, 255)]),
            },
        }
    }
}

// ------------------------------------------------------------------ glyphs

pub struct Glyphs {
    pub installed: &'static str,
    pub missing: &'static str,
    pub unknown: &'static str,
    pub conflict: &'static str,
    /// before the selected list row
    pub pointer: &'static str,
    /// in prompt labels and the overlay cursor
    pub arrow: &'static str,
    pub left: &'static str,
    pub right: &'static str,
    pub spinner: &'static [&'static str],
    /// terminal tab: the tool finished cleanly / failed or was killed
    pub done: &'static str,
    pub failed: &'static str,
    /// terminal tab: new output you haven't looked at
    pub unread: &'static str,
    /// the fade on either side of the selected row's bar
    pub glow_l: &'static str,
    pub glow_r: &'static str,
    /// around the terminal tab in front
    pub tab_l: &'static str,
    pub tab_r: &'static str,
    /// more chips hidden off the edge of the category bar
    pub more_l: &'static str,
    pub more_r: &'static str,
    /// symbol icons on category chips and the header mark
    pub icons: bool,
}

pub static UNICODE: Glyphs = Glyphs {
    installed: "●",
    missing: "○",
    unknown: "·",
    conflict: "◐",
    pointer: "▸ ",
    arrow: "▸",
    left: "◂",
    right: "▸",
    spinner: &["⠋", "⠙", "⠹", "⠸", "⠼", "⠴", "⠦", "⠧", "⠇", "⠏"],
    done: "✓",
    failed: "✗",
    unread: "•",
    glow_l: "░▒▓",
    glow_r: "▓▒░",
    tab_l: "┌",
    tab_r: "┐",
    more_l: "‹",
    more_r: "›",
    icons: true,
};

pub static ASCII: Glyphs = Glyphs {
    installed: "*",
    missing: "o",
    unknown: ".",
    conflict: "~",
    pointer: "> ",
    arrow: ">",
    left: "<",
    right: ">",
    spinner: &["|", "/", "-", "\\"],
    done: "+",
    failed: "x",
    unread: "!",
    glow_l: "  >",
    glow_r: "<  ",
    tab_l: "[",
    tab_r: "]",
    more_l: "<",
    more_r: ">",
    icons: false,
};

const ASCII_BORDER: border::Set = border::Set {
    top_left: "+",
    top_right: "+",
    bottom_left: "+",
    bottom_right: "+",
    vertical_left: "|",
    vertical_right: "|",
    horizontal_top: "-",
    horizontal_bottom: "-",
};

// ------------------------------------------------------------------- theme

/// Everything the renderer needs, resolved from `Settings`.
pub struct Theme {
    pub pal: Palette,
    pub glyph: &'static Glyphs,
    pub border: border::Set,
    pub bold: bool,
    pub overlay: OverlayPos,
    pub backdrop: Backdrop,
    /// colours are stripped (NO_COLOR): selection falls back to reverse video
    pub mono: bool,
}

impl Default for Theme {
    fn default() -> Self {
        Settings::default().theme()
    }
}

impl Theme {
    /// `fg`, made bold when bold text is on.
    pub fn strong(&self, fg: Color) -> Style {
        self.emph(Style::default().fg(fg))
    }

    /// Add bold to `s` when bold text is on.
    pub fn emph(&self, s: Style) -> Style {
        if self.bold {
            s.add_modifier(Modifier::BOLD)
        } else {
            s
        }
    }

    /// The one solid "you are here" bar. Reverse video when colours are
    /// stripped, since an accent background would simply vanish.
    pub fn selected(&self) -> Style {
        if self.mono {
            Style::default().add_modifier(Modifier::REVERSED | Modifier::BOLD)
        } else {
            self.emph(Style::default().fg(self.pal.ink).bg(self.pal.accent))
        }
    }

    pub fn fg(&self, c: Color) -> Style {
        Style::default().fg(c)
    }

    /// A bordered block in the chosen border style.
    pub fn block<'a>(&self, border: Color) -> ratatui::widgets::Block<'a> {
        ratatui::widgets::Block::default()
            .borders(ratatui::widgets::Borders::ALL)
            .border_set(self.border)
            .border_style(Style::default().fg(border))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_tolerate_partial_files() {
        let s = Settings {
            theme: ThemeName::LightDaltonized,
            borders: Borders::Ascii,
            bold: false,
            ..Default::default()
        };
        let y = serde_yaml::to_string(&s).unwrap();
        assert!(y.contains("light-daltonized"), "{y}");
        assert_eq!(serde_yaml::from_str::<Settings>(&y).unwrap(), s);
        // a hand-written file with one key keeps the other defaults
        let partial: Settings = serde_yaml::from_str("theme: cyan\n").unwrap();
        assert_eq!(partial.theme, ThemeName::Cyan);
        assert!(partial.bold);
    }

    #[test]
    fn stepping_wraps_both_ways() {
        let mut s = Settings::default();
        s.step(Field::Theme, -1);
        assert_eq!(s.theme, ThemeName::Cyan);
        s.step(Field::Theme, 1);
        assert_eq!(s.theme, ThemeName::Frost);
        for _ in 0..ThemeName::ALL.len() {
            s.step(Field::Theme, 1);
        }
        assert_eq!(s.theme, ThemeName::Frost);
        s.step(Field::Bold, 1);
        assert!(!s.bold);
    }

    #[test]
    fn frost_is_the_default_and_first() {
        assert_eq!(Settings::default().theme, ThemeName::Frost);
        assert_eq!(ThemeName::ALL[0], ThemeName::Frost);
        let y = serde_yaml::to_string(&Settings::default()).unwrap();
        assert!(y.contains("theme: frost"), "{y}");
        // the splash gets the cube's three faces, light to dark
        assert_eq!(
            Palette::of(ThemeName::Frost).gradient,
            Some([(188, 221, 230), (137, 194, 210), (70, 160, 185)])
        );
    }

    #[test]
    fn every_theme_resolves() {
        for t in ThemeName::ALL {
            let _ = Palette::of(t);
            assert!(!t.label().is_empty());
        }
    }
}
