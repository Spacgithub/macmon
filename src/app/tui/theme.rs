//! Colors from the terminal's own palette (the default foreground and the 16 ANSI colors), so
//! macmon follows the terminal theme, and the bar glyphs the terminal draws without gaps.

use std::borrow::Cow;

use ratatui::style::{Color, Modifier, Style};
use ratatui::text::Span;

/// Borders: bright black (ANSI 8).
pub(super) const BORDER: Color = Color::DarkGray;
/// Secondary text (separators, units, zeros): bright black (ANSI 8).
pub(super) const DIM: Color = Color::DarkGray;
/// Titles and text: the terminal's default foreground.
pub(super) const TEXT: Color = Color::Reset;
/// Keys in hints and buttons: cyan (ANSI 6).
pub(super) const KEY: Color = Color::Cyan;
/// Brand and panel names share one restrained accent.
pub(super) const SECTION: Color = Color::Cyan;
/// Quiet, consistent accents; load and temperature values still use the warning gradient.
pub(super) const CPU: Color = Color::Cyan;
pub(super) const GPU: Color = Color::Magenta;
pub(super) const MEMORY: Color = Color::Blue;
pub(super) const POWER: Color = Color::Green;
pub(super) const DOWNLOAD: Color = CPU;
pub(super) const UPLOAD: Color = GPU;
/// Selected process row: reverse video over the default colors, so the row reads as one bar
/// instead of reversing each load color in it.
pub(super) const SELECTED: Style = Style::new().fg(TEXT).add_modifier(Modifier::REVERSED);

/// Steps of the load gradient: green up to `GREEN_MAX`, yellow up to `YELLOW_MAX`, red above.
const GREEN_MAX: f64 = 1.0 / 3.0;
const YELLOW_MAX: f64 = 2.0 / 3.0;

/// Box or metric name: bold, in the default color.
pub(super) fn heading<'a>(text: impl Into<Cow<'a, str>>) -> Span<'a> {
  Span::styled(text, Style::new().fg(TEXT).add_modifier(Modifier::BOLD))
}

pub(super) fn value<'a>(text: impl Into<Cow<'a, str>>, color: Color) -> Span<'a> {
  Span::styled(text, Style::new().fg(color).add_modifier(Modifier::BOLD))
}

/// Text in the default color. The color is set, not left out, so text drawn over a border doesn't
/// take the border's color.
pub(super) fn text<'a>(text: impl Into<Cow<'a, str>>) -> Span<'a> {
  Span::styled(text, TEXT)
}

pub(super) fn dim<'a>(text: impl Into<Cow<'a, str>>) -> Span<'a> {
  Span::styled(text, DIM)
}

/// The key to press in a hint or a button: `KEY`, no modifier.
pub(super) fn key<'a>(text: impl Into<Cow<'a, str>>) -> Span<'a> {
  Span::styled(text, KEY)
}

/// The label of a hint or a button: the default color, faint (SGR 2). Terminals that support it
/// blend the text toward the background, so it reads the same in light and dark themes, unlike
/// ANSI 8; the others show plain text.
pub(super) fn faint<'a>(text: impl Into<Cow<'a, str>>) -> Span<'a> {
  Span::styled(text, Style::new().fg(TEXT).add_modifier(Modifier::DIM))
}

/// Link text: underlined blue (ANSI 4).
pub(super) fn link<'a>(text: impl Into<Cow<'a, str>>) -> Span<'a> {
  Span::styled(text, Style::new().fg(Color::Blue).add_modifier(Modifier::UNDERLINED))
}

/// Load color for `t` in `0.0..=1.0`: the terminal's green, yellow or red, in steps of a third.
/// Values below the range (and NaN) are green, above it red.
pub(super) fn gradient(t: f64) -> Color {
  if t.is_nan() || t <= GREEN_MAX {
    Color::Green
  } else if t <= YELLOW_MAX {
    Color::Yellow
  } else {
    Color::Red
  }
}

/// Calm chart accents retain the hot-load warning of the original history graphs.
pub(super) fn graph_color(accent: Color, load: f64) -> Color {
  if load > YELLOW_MAX { Color::Red } else { accent }
}

pub(super) fn history_emphasis(newest: bool, color: Color) -> Modifier {
  if newest || color == Color::Red { Modifier::BOLD } else { Modifier::DIM }
}

/// True in Apple Terminal (`TERM_PROGRAM`), which draws gaps between the eighth blocks, so graph
/// bars come in three levels there (blank, `▄`, `█`).
pub(super) fn detect_three_level_bars() -> bool {
  is_apple_terminal(std::env::var("TERM_PROGRAM").ok().as_deref())
}

fn is_apple_terminal(term_program: Option<&str>) -> bool {
  term_program == Some("Apple_Terminal")
}

/// Native RGBA shades come from queried terminal colors, alongside the ANSI text palette.
pub(super) fn fan_palette(caps: super::fan_protocol::Capabilities) -> super::fan_raster::Palette {
  super::fan_raster::Palette {
    background: caps.background,
    foreground: caps.foreground,
    accent: caps.accent.filter(|color| *color != caps.background).unwrap_or(caps.foreground),
  }
}

#[cfg(test)]
mod tests {
  use ratatui::style::Color;

  use super::gradient;

  #[test]
  fn gradient_steps_through_terminal_colors() {
    // values below the range (and NaN) are green, above it red
    let steps = [
      (f64::NAN, Color::Green),
      (-1.0, Color::Green),
      (1.0 / 3.0, Color::Green),
      (0.34, Color::Yellow),
      (2.0 / 3.0, Color::Yellow),
      (0.67, Color::Red),
      (2.0, Color::Red),
    ];
    for (t, color) in steps {
      assert_eq!(gradient(t), color, "{t}");
    }
  }
}
