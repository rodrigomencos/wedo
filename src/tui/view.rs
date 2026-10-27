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
use crate::store::Item;
use crate::theme::ACCENT;

const HIGHLIGHT_SYMBOL: &str = "› ";

const HELP: &[(&str, &str)] = &[
    ("space", "toggle"),
    ("a", "add"),
    ("e", "edit"),
    ("d", "delete"),
    ("J/K", "move"),
    ("c", "clear done"),
    ("h", "hide done"),
    ("n/N", "next open"),
    ("q", "quit"),
];

impl App {
    pub(super) fn draw(&mut self, frame: &mut Frame) {
        let [main, footer] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());

        let (done, total) = self.list.progress();
        let all_done = done > 0 && done == total;
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
                Line::from(format!(
                    " {done}/{total}{} ",
                    if self.hide_done {
                        " · done hidden"
                    } else {
                        ""
                    }
                ))
                .fg(if all_done { Color::Green } else { Color::Reset })
                .not_dim()
                .right_aligned(),
            )
            .title_bottom(Line::from(format!(" {} ", tilde(self.store.path()))).right_aligned());

        if self.rows.is_empty() {
            let (before, key, after) = if self.list.items.is_empty() {
                ("Nothing to do. Press ", "a", " to add an item.")
            } else {
                ("Everything's done. Press ", "h", " to show finished items.")
            };
            let hint = Paragraph::new(Line::from(vec![
                before.dim(),
                key.bold().fg(ACCENT),
                after.dim(),
            ]))
            .centered()
            .block(block);
            frame.render_widget(hint, main);
        } else {
            let items = &self.list.items;
            let id_width = self
                .rows
                .iter()
                .map(|&i| items[i].id.to_string().len())
                .max()
                .unwrap_or(1);
            // "<id> [x] ", then the text; wrapped lines are indented to line up under it.
            let prefix_width = id_width + 5;
            let borders = 2;
            let text_width = (main.width as usize)
                .saturating_sub(borders + HIGHLIGHT_SYMBOL.width() + prefix_width)
                .max(1);
            let indent = " ".repeat(prefix_width);
            let sections = section_progress(items);
            // Headings start where the checkbox would, so they stand out from their items.
            let heading_width = text_width + prefix_width - (id_width + 1);
            let rows =
                self.rows
                    .iter()
                    .map(|&i| (&items[i], sections[i]))
                    .map(|(item, (done, total))| {
                        if item.is_heading() {
                            return ListItem::new(heading_lines(
                                item,
                                id_width,
                                heading_width,
                                done,
                                total,
                            ));
                        }
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
                        let rest = lines.map(|l| {
                            Line::from(vec![indent.as_str().into(), Span::styled(l, style)])
                        });
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
            Mode::Normal => match (&self.error, &self.notice) {
                (Some(error), _) => Line::from(error.as_str()).red(),
                (None, Some(notice)) => Line::from(notice.as_str()).fg(ACCENT),
                (None, None) => Line::from(
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

/// A heading as `<id> Title ────── done/total`, or wrapped without the rule when it's too long.
fn heading_lines(
    item: &Item,
    id_width: usize,
    width: usize,
    done: usize,
    total: usize,
) -> Vec<Line<'_>> {
    let number = format!("{:>id_width$} ", item.id).dim();
    let title = item.heading_title();
    let progress = format!(" {done}/{total}");
    let rule = width.saturating_sub(title.width() + 1 + progress.width());
    if rule >= 2 {
        return vec![Line::from(vec![
            number,
            title.bold(),
            " ".into(),
            "─".repeat(rule).dim(),
            progress.dim(),
        ])];
    }
    let indent = " ".repeat(id_width + 1);
    wrap(title, width)
        .into_iter()
        .enumerate()
        .map(|(i, part)| {
            let lead = if i == 0 {
                number.clone()
            } else {
                indent.clone().into()
            };
            Line::from(vec![lead, part.bold()])
        })
        .collect()
}

/// For each heading's index, the (done, total) of the items under it until the next heading.
fn section_progress(items: &[Item]) -> Vec<(usize, usize)> {
    let mut sections = vec![(0, 0); items.len()];
    let mut heading = None;
    for (i, item) in items.iter().enumerate() {
        if item.is_heading() {
            heading = Some(i);
        } else if let Some(h) = heading {
            sections[h].0 += usize::from(item.done);
            sections[h].1 += 1;
        }
    }
    sections
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
