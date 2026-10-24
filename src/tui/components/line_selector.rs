//! Upstream `LineSelector.tsx`: pick a status line (for widgets or colors), and append, delete or move lines.

use super::confirm::{Answer, Confirm};
use super::list::{Entry, Event, Item, List, Props};
use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;
use serde_json::Value;

/// `pluralize('widget', n, true)`.
fn widgets_label(n: usize) -> String {
    if n == 1 {
        "1 widget".into()
    } else {
        format!("{n} widgets")
    }
}

fn suffix(line: &Value) -> String {
    let n = line.as_array().map_or(0, Vec::len);
    if n > 0 {
        widgets_label(n)
    } else {
        "empty".into()
    }
}

/// `moveItem`: take the item out and insert it at `to`.
pub fn move_item<T: Clone>(items: &[T], from: usize, to: usize) -> Vec<T> {
    let mut moved = items.to_vec();
    if from < moved.len() {
        let item = moved.remove(from);
        moved.insert(to.min(moved.len()), item);
    }
    moved
}

pub struct Config<'a> {
    pub title: Option<&'a str>,
    pub allow_editing: bool,
    /// `blockIfPowerlineActive` with a theme other than custom: the theme's name.
    pub managed_theme: Option<&'a str>,
}

/// What the line selector asks the App to do.
pub enum Outcome {
    None,
    Select(usize),
    Back,
    LinesUpdated(Vec<Value>),
}

pub struct LineSelector {
    pub selected: usize,
    show_delete: bool,
    move_mode: bool,
    local_lines: Vec<Value>,
    seen_lines: Option<String>,
    seen_initial: Option<usize>,
    list: List,
    confirm: Confirm,
}

impl LineSelector {
    pub fn new() -> Self {
        LineSelector {
            selected: 0,
            show_delete: false,
            move_mode: false,
            local_lines: Vec::new(),
            seen_lines: None,
            seen_initial: None,
            list: List::new(0),
            confirm: Confirm::default(),
        }
    }

    /// The effects on `[lines]` and `[initialSelection]`.
    pub fn sync(&mut self, lines: &[Value], initial: usize) {
        let key = crate::jsobj::stringify(&Value::Array(lines.to_vec()));
        if self.seen_lines.as_deref() != Some(key.as_str()) {
            self.seen_lines = Some(key);
            self.local_lines = lines.to_vec();
        }
        if self.seen_initial != Some(initial) {
            self.seen_initial = Some(initial);
            self.selected = initial;
        }
    }

    fn items(&self) -> Vec<Item<usize>> {
        self.local_lines
            .iter()
            .enumerate()
            .map(|(i, line)| {
                Item::Entry(Entry::new(format!("☰ Line {}", i + 1), i).sublabel(Some(format!("({})", suffix(line)))))
            })
            .collect()
    }

    fn props<'a>(&self, items: &'a [Item<usize>]) -> Props<'a, usize> {
        let mut p = Props::new(items);
        p.margin_top = 1.0;
        p.show_back_button = true;
        p.initial_selection = self.selected;
        p
    }

    pub fn handle(&mut self, ev: &Input, cfg: &Config) -> Outcome {
        if self.show_delete {
            return match self.confirm.handle(ev, false) {
                Answer::Yes => {
                    let updated = self.delete_line(self.selected);
                    self.selected = self.selected.saturating_sub(1);
                    self.show_delete = false;
                    updated.map_or(Outcome::None, Outcome::LinesUpdated)
                }
                Answer::No => {
                    self.show_delete = false;
                    Outcome::None
                }
                Answer::None => Outcome::None,
            };
        }
        let shortcut = if ev.key.ctrl || ev.key.meta { "" } else { ev.input.as_str() };
        // The List's handler runs first (a child's effect registers before its parent's).
        let mut outcome = Outcome::None;
        if cfg.managed_theme.is_none() && !self.move_mode {
            let items = self.items();
            let p = self.props(&items);
            self.list.sync(&p);
            match self.list.handle(&p, ev) {
                Event::Selected(line, _) => outcome = Outcome::Select(line),
                Event::Back => outcome = Outcome::Back,
                Event::None => {}
            }
            // onSelectionChange
            self.selected = self.list.selected;
        }
        if cfg.managed_theme.is_some() {
            return Outcome::Back;
        }
        let n = self.local_lines.len();
        if self.move_mode {
            if ev.key.up_arrow && n > 1 {
                let target = if self.selected == 0 { n - 1 } else { self.selected - 1 };
                return self.move_to(target);
            } else if ev.key.down_arrow && n > 1 {
                let target = if self.selected + 1 > n - 1 { 0 } else { self.selected + 1 };
                return self.move_to(target);
            } else if ev.key.escape || ev.key.return_ {
                self.move_mode = false;
            }
            return outcome;
        }
        match shortcut {
            "a" => {
                if cfg.allow_editing {
                    self.local_lines.push(Value::Array(Vec::new()));
                    self.selected = self.local_lines.len() - 1;
                    return Outcome::LinesUpdated(self.local_lines.clone());
                }
                return outcome;
            }
            "d" => {
                if cfg.allow_editing && n > 1 && self.selected < n {
                    self.show_delete = true;
                    self.confirm = Confirm::default();
                }
                return outcome;
            }
            "m" => {
                if cfg.allow_editing && n > 1 && self.selected < n {
                    self.move_mode = true;
                }
                return outcome;
            }
            _ => {}
        }
        if ev.key.escape {
            return Outcome::Back;
        }
        outcome
    }

    fn move_to(&mut self, target: usize) -> Outcome {
        self.local_lines = move_item(&self.local_lines, self.selected, target);
        self.selected = target;
        Outcome::LinesUpdated(self.local_lines.clone())
    }

    fn delete_line(&mut self, index: usize) -> Option<Vec<Value>> {
        if self.local_lines.len() <= 1 {
            return None;
        }
        self.local_lines.remove(index);
        Some(self.local_lines.clone())
    }

    pub fn view(&mut self, cfg: &Config) -> El {
        if let Some(theme) = cfg.managed_theme {
            let mut name = theme.to_string();
            if let Some(first) = name.get(..1) {
                name = format!("{}{}", first.to_uppercase(), &theme[1..]);
            }
            return col(vec![
                text(TextStyle::bold(), vec![s(cfg.title.unwrap_or("Select Line"))]),
                boxed(
                    BoxStyle::default().margin_top(1.0),
                    vec![text(
                        TextStyle::color("yellow"),
                        vec![s("⚠ Colors are currently managed by the Powerline theme:"), s(format!(" {name}"))],
                    )],
                ),
                boxed(
                    BoxStyle::default().margin_top(1.0),
                    vec![text(TextStyle::dim(), vec![s("To customize colors, either:")])],
                ),
                boxed(
                    BoxStyle::default().margin_left(2.0),
                    vec![text(
                        TextStyle::dim(),
                        vec![s("• Change to 'Custom' theme in Powerline Configuration → Themes")],
                    )],
                ),
                boxed(
                    BoxStyle::default().margin_left(2.0),
                    vec![text(TextStyle::dim(), vec![s("• Disable Powerline mode in Powerline Configuration")])],
                ),
                boxed(BoxStyle::default().margin_top(2.0), vec![plain("Press any key to go back...")]),
            ]);
        }
        if self.show_delete {
            if let Some(line) = self.local_lines.get(self.selected) {
                let header = text(
                    TextStyle::bold(),
                    vec![nested(
                        TextStyle::default(),
                        vec![
                            // JSX trims "☰ Line" before the line break, so no space before the number.
                            nested(TextStyle::default(), vec![s("☰ Line"), s((self.selected + 1).to_string())]),
                            s(" "),
                            nested(TextStyle::dim(), vec![s("("), s(suffix(line)), s(")")]),
                        ],
                    )],
                );
                let mut column = BoxStyle::column();
                column.gap = 1.0;
                return col(vec![
                    boxed(
                        column,
                        vec![header, text(TextStyle::bold(), vec![s("Are you sure you want to delete line?")])],
                    ),
                    boxed(BoxStyle::default().margin_top(1.0), vec![self.confirm.view(None, true, false)]),
                ]);
            }
        }
        let mut header = vec![text(TextStyle::bold(), vec![s(cfg.title.unwrap_or("Select Line to Edit")), s(" ")])];
        if self.move_mode {
            header.push(text(TextStyle::color("blue"), vec![s("[MOVE MODE]")]));
        }
        let help = if self.move_mode {
            "↑↓ to move line, ESC or Enter to exit move mode"
        } else if cfg.allow_editing {
            if self.local_lines.len() > 1 {
                "(a) to append new line, (d) to delete line, (m) to move line, ESC to go back"
            } else {
                "(a) to append new line, ESC to go back"
            }
        } else {
            "ESC to go back"
        };
        let mut children = vec![
            row(header),
            text(TextStyle::dim(), vec![s("Choose which status line to configure")]),
            text(TextStyle::dim(), vec![s(help)]),
        ];
        if self.move_mode {
            let rows: Vec<El> = self
                .local_lines
                .iter()
                .enumerate()
                .map(|(i, line)| {
                    let sel = i == self.selected;
                    row(vec![text(
                        TextStyle::default().with_color(sel.then_some("blue")),
                        vec![
                            nested(TextStyle::default(), vec![s(if sel { "◆  " } else { "   " })]),
                            nested(
                                TextStyle::default(),
                                vec![
                                    nested(TextStyle::default(), vec![s("☰ Line"), s(" "), s((i + 1).to_string())]),
                                    s(" "),
                                    nested(TextStyle::default().with_dim(!sel), vec![s("("), s(suffix(line)), s(")")]),
                                ],
                            ),
                        ],
                    )])
                })
                .collect();
            children.push(boxed(BoxStyle::column().margin_top(1.0), rows));
        } else {
            let items = self.items();
            let p = self.props(&items);
            self.list.sync(&p);
            self.selected = self.list.selected;
            children.push(self.list.view(&p));
        }
        col(children)
    }
}
