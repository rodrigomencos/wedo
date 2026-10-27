//! Drawing the checklist, its title bar and the footer.

use std::path::{Path, PathBuf};

use ratatui::Frame;
use ratatui::layout::{Constraint, Layout, Rect};
use ratatui::style::{Color, Style, Stylize};
use ratatui::text::{Line, Span};
use ratatui::widgets::{
    Block, Clear, HighlightSpacing, List as ListWidget, ListItem, ListState, Padding, Paragraph,
};
use unicode_width::UnicodeWidthStr;

use super::wrap::wrap;
use super::{Action, App, HIGHLIGHT_FOR, Mode, Target};
use crate::store::Item;
use crate::store::Summary;
use crate::theme::ACCENT;

const HIGHLIGHT_SYMBOL: &str = "› ";

/// The footer's reminder of the most common keys.
const FOOTER: &[(&str, &str)] = &[
    ("space", "check"),
    ("a", "add"),
    ("e", "edit"),
    ("d", "delete"),
    ("/", "search"),
    ("u", "undo"),
    ("?", "all keys"),
    ("q", "quit"),
];

/// Every key, grouped, for the `?` screen.
const KEYS: &[(&str, &[(&str, &str)])] = &[
    (
        "Move",
        &[
            ("j k ↓ ↑", "up and down"),
            ("g G", "first and last item"),
            ("n N", "next and previous unfinished item"),
            ("tab shift-tab", "next and previous list"),
            ("L", "go to the lists sidebar: open, create, delete"),
        ],
    ),
    (
        "Change",
        &[
            ("space enter", "check or uncheck"),
            ("a", "add at the end"),
            ("o O", "add below or above the selection"),
            ("e", "edit"),
            ("d", "delete"),
            ("J K", "move the item down or up"),
            ("c", "delete all finished items"),
            ("u", "undo"),
        ],
    ),
    (
        "View",
        &[
            ("h", "hide or show finished items"),
            ("/", "search; esc clears it"),
            ("y", "copy the item's text"),
            ("q esc", "quit"),
        ],
    ),
];

impl App {
    pub(super) fn draw(&mut self, frame: &mut Frame) {
        self.recent.retain(|_, at| at.elapsed() < HIGHLIGHT_FOR);
        let [body, footer] =
            Layout::vertical([Constraint::Fill(1), Constraint::Length(1)]).areas(frame.area());
        let main = match self.sidebar_width(body.width) {
            Some(width) => {
                let [side, main] =
                    Layout::horizontal([Constraint::Length(width), Constraint::Fill(1)])
                        .areas(body);
                self.draw_sidebar(frame, side);
                main
            }
            None => body,
        };

        let (done, total) = self.list.progress();
        let all_done = done > 0 && done == total;
        let mut status = format!(" {done}/{total} ");
        if self.hide_done {
            status.push_str("· done hidden ");
        }
        if !self.filter.is_empty() {
            status.push_str(&format!("· /{} ", self.filter));
        }
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
                Line::from(status)
                    .fg(if all_done { Color::Green } else { Color::Reset })
                    .not_dim()
                    .right_aligned(),
            )
            .title_bottom(Line::from(format!(" {} ", tilde(self.store.path()))).right_aligned());

        if self.rows.is_empty() {
            let (before, key, after) = if self.list.items.is_empty() {
                ("Nothing to do. Press ", "a", " to add an item.")
            } else if !self.filter.is_empty() {
                ("Nothing matches. Press ", "esc", " to clear the search.")
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
                        let (mark, mut style) = if item.done {
                            ("✓".green(), Style::new().dim().crossed_out())
                        } else {
                            (" ".into(), Style::new())
                        };
                        // Just changed by someone else: color it until the highlight fades.
                        let fresh = self.recent.contains_key(&item.text);
                        let mut number = Style::new().dim();
                        if fresh {
                            style = style.fg(ACCENT).bold();
                            number = Style::new().fg(ACCENT).bold();
                        }
                        let mut lines = wrap(&item.text, text_width).into_iter();
                        let first = Line::from(vec![
                            Span::styled(format!("{:>id_width$} [", item.id), number),
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
                    Target::Search => "search › ",
                    Target::NewList => "new list › ",
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
            Mode::Help => Line::from("press any key to close").dim(),
            Mode::Confirm(Action::DeleteList(name)) => Line::from(format!(
                "delete the list \"{name}\" and all its items? (y/N)"
            ))
            .yellow(),
            Mode::Lists { .. } => key_hints(&[
                ("enter", "open"),
                ("n", "new"),
                ("d", "delete"),
                ("esc", "close"),
            ]),
            Mode::Normal => match (&self.error, &self.notice) {
                (Some(error), _) => Line::from(error.as_str()).red(),
                (None, Some((notice, at))) if at.elapsed() < HIGHLIGHT_FOR => {
                    Line::from(notice.as_str()).fg(ACCENT)
                }
                _ => Line::from(
                    FOOTER
                        .iter()
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
        if let Mode::Help = self.mode {
            draw_help(frame);
        }
        self.theme.finish(frame);
    }
}

impl App {
    /// The sidebar's width, or None to hide it: no other lists to show, or too narrow.
    fn sidebar_width(&self, total: u16) -> Option<u16> {
        let focused = matches!(self.mode, Mode::Lists { .. });
        if self.lists.is_none() || (self.sidebar.len() < 2 && !focused) || total < 60 {
            return None;
        }
        let widest = self
            .sidebar
            .iter()
            .map(|s| s.name.width() + 1 + progress(s).width())
            .max();
        let chrome = 2 + HIGHLIGHT_SYMBOL.width();
        Some(((widest.unwrap_or(0) + chrome) as u16).clamp(16, total / 3))
    }

    /// Every list with its progress. Focused (`L`), it has a cursor; otherwise it
    /// just marks the list on screen.
    fn draw_sidebar(&mut self, frame: &mut Frame, area: Rect) {
        let current = self.store.name();
        let room = (area.width as usize).saturating_sub(2 + HIGHLIGHT_SYMBOL.width());
        let rows: Vec<ListItem> = self
            .sidebar
            .iter()
            .map(|s| {
                let progress = progress(s);
                let name = truncate(&s.name, room.saturating_sub(progress.width() + 1));
                let pad = " ".repeat(room.saturating_sub(name.width() + progress.width()));
                let name_style = if s.name == current {
                    Style::new().fg(ACCENT).bold()
                } else {
                    Style::new()
                };
                let progress_style = if s.total > 0 && s.done == s.total {
                    Style::new().green()
                } else {
                    Style::new().dim()
                };
                ListItem::new(Line::from(vec![
                    Span::styled(name, name_style),
                    pad.into(),
                    Span::styled(progress, progress_style),
                ]))
            })
            .collect();
        let focused = matches!(self.mode, Mode::Lists { .. });
        let block = Block::bordered()
            .border_style(if focused {
                Style::new().fg(ACCENT)
            } else {
                Style::new().dim()
            })
            .title(Line::from(" lists ").bold().not_dim().fg(ACCENT))
            .title_bottom(Line::from(if focused { "" } else { " L " }).right_aligned());
        let list = ListWidget::new(rows)
            .block(block)
            .highlight_symbol(Line::from(HIGHLIGHT_SYMBOL).fg(ACCENT))
            .highlight_spacing(HighlightSpacing::Always);
        let mut marker = ListState::default().with_selected(self.current_in_sidebar());
        match &mut self.mode {
            Mode::Lists { state } => {
                frame.render_stateful_widget(list.highlight_style(self.theme.selected), area, state)
            }
            _ => frame.render_stateful_widget(list, area, &mut marker),
        }
    }
}

fn progress(summary: &Summary) -> String {
    format!("{}/{}", summary.done, summary.total)
}

/// Cuts `text` to `width` columns, ending in … when it had to cut.
fn truncate(text: &str, width: usize) -> String {
    if text.width() <= width {
        return text.to_string();
    }
    let mut out = String::new();
    for c in text.chars() {
        if out.width() + c.to_string().width() + 1 > width {
            break;
        }
        out.push(c);
    }
    out + "…"
}

fn key_hints(keys: &[(&str, &str)]) -> Line<'static> {
    let spans = keys.iter().flat_map(|(key, what)| {
        [
            Span::from(key.to_string()).bold().fg(ACCENT),
            Span::from(format!(" {what}  ")).dim(),
        ]
    });
    Line::from(spans.collect::<Vec<_>>())
}

fn draw_help(frame: &mut Frame) {
    let key_width = KEYS
        .iter()
        .flat_map(|(_, keys)| keys.iter())
        .map(|(k, _)| k.width())
        .max()
        .unwrap_or(0);
    let mut lines = Vec::new();
    for (group, keys) in KEYS {
        if !lines.is_empty() {
            lines.push(Line::default());
        }
        lines.push(Line::from(*group).bold());
        for (key, what) in *keys {
            let pad = " ".repeat(key_width - key.width() + 2);
            lines.push(Line::from(vec![
                format!("  {key}").fg(ACCENT).bold(),
                pad.into(),
                (*what).into(),
            ]));
        }
    }
    lines.push(Line::default());
    lines.push(Line::from(vec![
        "Items written as ".dim(),
        "# Title".bold(),
        " are section headings.".dim(),
    ]));

    // Fit on screen; on a very small one the bottom of the list is cut off.
    let screen = frame.area();
    let width = (lines.iter().map(Line::width).max().unwrap_or(0) as u16 + 4).min(screen.width);
    let height = (lines.len() as u16 + 2).min(screen.height);
    let area = frame
        .area()
        .centered(Constraint::Length(width), Constraint::Length(height));
    frame.render_widget(Clear, area);
    let block = Block::bordered()
        .title(Line::from(" keys ").bold().fg(ACCENT))
        .padding(Padding::horizontal(1));
    frame.render_widget(Paragraph::new(lines).block(block), area);
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
