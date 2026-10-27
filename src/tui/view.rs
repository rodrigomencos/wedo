//! Drawing the checklist, its title bar and the footer.

use std::path::{Path, PathBuf};

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{Block, HighlightSpacing, List as ListWidget, ListItem, Paragraph};
use unicode_width::UnicodeWidthStr;

use super::wrap::wrap;
use super::{Action, App, Mode, Target};
use crate::theme::ACCENT;

const HIGHLIGHT_SYMBOL: &str = "› ";

const HELP: &[(&str, &str)] = &[
    ("space", "toggle"),
    ("a", "add"),
    ("e", "edit"),
    ("d", "delete"),
    ("J/K", "move"),
    ("c", "clear done"),
    ("q", "quit"),
];

impl App {
    pub(super) fn draw(&mut self, frame: &mut Frame) {
        let [main, footer] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());

        let done = self.list.items.iter().filter(|i| i.done).count();
        let all_done = done > 0 && done == self.list.items.len();
        // Titles inherit the border's dim style unless they opt out.
        let block = Block::bordered()
            .border_style(Style::new().dim())
            .title(
                Line::from(format!(" {} ", self.store.name()))
                    .bold()
                    .not_dim()
                    .fg(ACCENT),
            )
            .title(
                Line::from(format!(" {done}/{} ", self.list.items.len()))
                    .fg(if all_done { Color::Green } else { Color::Reset })
                    .not_dim()
                    .right_aligned(),
            )
            .title_bottom(Line::from(format!(" {} ", tilde(self.store.path()))).right_aligned());

        if self.list.items.is_empty() {
            let hint = Paragraph::new(Line::from(vec![
                "Nothing to do. Press ".dim(),
                "a".bold().fg(ACCENT),
                " to add an item.".dim(),
            ]))
            .centered()
            .block(block);
            frame.render_widget(hint, main);
        } else {
            let id_width = self
                .list
                .items
                .iter()
                .map(|i| i.id.to_string().len())
                .max()
                .unwrap_or(1);
            // "<id> [x] ", then the text; wrapped lines are indented to line up under it.
            let prefix_width = id_width + 5;
            let borders = 2;
            let text_width = (main.width as usize)
                .saturating_sub(borders + HIGHLIGHT_SYMBOL.width() + prefix_width)
                .max(1);
            let indent = " ".repeat(prefix_width);
            let rows = self.list.items.iter().map(|item| {
                let (mark, style) = if item.done {
                    ("✓".green(), Style::new().dim().crossed_out())
                } else {
                    (" ".into(), Style::new())
                };
                let mut lines = wrap(&item.text, text_width).into_iter();
                let first = Line::from(vec![
                    format!("{:>id_width$} [", item.id).dim(),
                    mark,
                    "] ".dim(),
                    Span::styled(lines.next().unwrap_or_default(), style),
                ]);
                let rest =
                    lines.map(|l| Line::from(vec![indent.as_str().into(), Span::styled(l, style)]));
                ListItem::new(std::iter::once(first).chain(rest).collect::<Vec<_>>())
            });
            let list = ListWidget::new(rows)
                .block(block)
                .highlight_symbol(Line::from(HIGHLIGHT_SYMBOL).fg(ACCENT))
                .highlight_spacing(HighlightSpacing::Always)
                .highlight_style(self.theme.selected);
            frame.render_stateful_widget(list, main, &mut self.state);
        }

        let line = match &self.mode {
            Mode::Input { target, input } => {
                let prompt = match target {
                    Target::Add(_) => "add › ",
                    Target::Edit(_) => "edit › ",
                };
                let available = (footer.width as usize).saturating_sub(prompt.width() + 1);
                let (visible, cursor_x) = input.view(available);
                frame
                    .set_cursor_position((footer.x + (prompt.width() + cursor_x) as u16, footer.y));
                Line::from(vec![prompt.fg(ACCENT).bold(), visible.into()])
            }
            Mode::Confirm(Action::Delete(id)) => {
                let text = self
                    .list
                    .get(*id)
                    .map(|i| i.text.as_str())
                    .unwrap_or_default();
                Line::from(format!("delete \"{text}\"? (y/N)")).yellow()
            }
            Mode::Confirm(Action::ClearDone) => {
                Line::from(format!("delete {done} finished items? (y/N)")).yellow()
            }
            Mode::Normal => match &self.error {
                Some(error) => Line::from(error.as_str()).red(),
                None => Line::from(
                    HELP.iter()
                        .chain(self.lists.is_some().then_some(&("tab", "lists")))
                        .flat_map(|(key, what)| {
                            [
                                Span::from(*key).bold().fg(ACCENT),
                                Span::from(format!(" {what}  ")).dim(),
                            ]
                        })
                        .collect::<Vec<_>>(),
                ),
            },
        };
        frame.render_widget(line, footer);
        self.theme.finish(frame);
    }
}

fn tilde(path: &Path) -> String {
    let home = std::env::var_os("HOME")
        .map(PathBuf::from)
        .unwrap_or_default();
    match path.strip_prefix(&home) {
        Ok(rest) if !home.as_os_str().is_empty() => format!("~/{}", rest.display()),
        _ => path.display().to_string(),
    }
}
