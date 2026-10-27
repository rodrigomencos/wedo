//! Colors come from the user's terminal theme, never from this program.
//!
//! Text only uses ANSI palette slots and the DIM modifier, which every terminal maps
//! to its own theme. The selection bar needs a background a little lighter (or
//! darker) than the terminal's, which no palette slot reliably is: Solarized's
//! bright black *is* its background. So the bar is tinted from the terminal's real
//! colors when it will tell us (OSC 10/11), and falls back to reverse video.

use ratatui::Frame;
use ratatui::crossterm::style::force_color_output;
use ratatui::style::{Color, Modifier, Style};
use terminal_colorsaurus::{QueryOptions, color_palette};

pub const ACCENT: Color = Color::Blue;

/// How far the selection bar moves from the background toward the foreground.
const TINT: f32 = 0.15;

pub struct Theme {
    pub selected: Style,
    no_color: bool,
}

impl Theme {
    /// Asks the terminal for its colors, so call it before the TUI takes over the terminal.
    pub fn detect() -> Self {
        // RGB needs truecolor, and NO_COLOR suppresses colors but not modifiers.
        let truecolor = matches!(
            std::env::var("COLORTERM").as_deref(),
            Ok("truecolor" | "24bit")
        );
        let no_color = std::env::var_os("NO_COLOR").is_some_and(|v| !v.is_empty());
        if no_color {
            // crossterm's own NO_COLOR support writes an empty SGR (a full reset) in place of
            // each color, which also wipes bold and reverse video. `finish` strips colors instead.
            force_color_output(true);
        }
        let palette = (truecolor && !no_color).then(|| color_palette(QueryOptions::default()).ok());
        let selected = match palette.flatten() {
            Some(p) => Style::new().bg(mix(
                p.background.scale_to_8bit(),
                p.foreground.scale_to_8bit(),
                TINT,
            )),
            None => Style::new().add_modifier(Modifier::REVERSED),
        };
        Self {
            selected: selected.add_modifier(Modifier::BOLD),
            no_color,
        }
    }

    /// Call at the end of each draw: honors NO_COLOR by dropping every color, keeping modifiers.
    pub fn finish(&self, frame: &mut Frame) {
        if self.no_color {
            for cell in &mut frame.buffer_mut().content {
                cell.fg = Color::Reset;
                cell.bg = Color::Reset;
            }
        }
    }
}

fn mix(from: (u8, u8, u8), to: (u8, u8, u8), amount: f32) -> Color {
    let channel =
        |a: u8, b: u8| (f32::from(a) + (f32::from(b) - f32::from(a)) * amount).round() as u8;
    Color::Rgb(
        channel(from.0, to.0),
        channel(from.1, to.1),
        channel(from.2, to.2),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn mix_moves_toward_the_target() {
        assert_eq!(mix((0, 0, 0), (255, 255, 255), 0.0), Color::Rgb(0, 0, 0));
        assert_eq!(
            mix((0, 0, 0), (255, 255, 255), 1.0),
            Color::Rgb(255, 255, 255)
        );
        // A light theme gets a darker bar.
        assert_eq!(
            mix((250, 250, 250), (50, 50, 50), 0.15),
            Color::Rgb(220, 220, 220)
        );
    }
}
