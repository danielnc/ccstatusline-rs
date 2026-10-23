//! The install screens, adapted from upstream `InstallMenu.tsx` and `ManageInstallationMenu.tsx`.
//!
//! Upstream installs its npm package (an update style, then a package manager). ccstatusline-rs installs the
//! command that runs this binary, so the install menu offers that one command, and Manage Installation keeps only
//! Uninstall (no npm/bun package actions, no update check). Layout and wording follow upstream's.

use super::list::{Entry, Event, Item, List, Props};
use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;

pub enum Outcome {
    None,
    Select,
    Cancel,
}

pub struct InstallMenu {
    list: List,
}

fn items(command: &str) -> Vec<Item<&'static str>> {
    vec![Item::Entry(Entry::new(command, "install").description(
        "Runs this ccstatusline-rs binary directly: no package runner and no Node process on each render.",
    ))]
}

fn props<'a>(items: &'a [Item<&'static str>]) -> Props<'a, &'static str> {
    let mut p = Props::new(items);
    p.color = Some("blue");
    p.margin_top = 1.0;
    p.show_back_button = true;
    p
}

impl InstallMenu {
    pub fn new() -> Self {
        InstallMenu { list: List::new(0) }
    }

    pub fn handle(&mut self, ev: &Input, command: &str) -> Outcome {
        let items = items(command);
        let p = props(&items);
        self.list.sync(&p);
        let picked = self.list.handle(&p, ev);
        if ev.key.escape {
            return Outcome::Cancel;
        }
        match picked {
            Event::Selected(..) => Outcome::Select,
            Event::Back => Outcome::Cancel,
            Event::None => Outcome::None,
        }
    }

    pub fn view(&mut self, command: &str, existing: Option<&str>, settings_path: &str) -> El {
        let items = items(command);
        let p = props(&items);
        self.list.sync(&p);
        let mut children = vec![text(TextStyle::bold(), vec![s("Install ccstatusline to Claude Code")])];
        if let Some(existing) = existing.filter(|e| !e.is_empty()) {
            children.push(boxed(
                BoxStyle::default().margin_bottom(1.0),
                vec![text(TextStyle::color("yellow"), vec![s("⚠ Current status line: \""), s(existing), s("\"")])],
            ));
        }
        children.push(row(vec![text(TextStyle::dim(), vec![s("Select the command to install:")])]));
        children.push(self.list.view(&p));
        children.push(boxed(
            BoxStyle::default().margin_top(2.0),
            vec![text(TextStyle::dim(), vec![s("The selected command will be written to"), s(" "), s(settings_path)])],
        ));
        children.push(boxed(
            BoxStyle::default().margin_top(1.0),
            vec![text(TextStyle::dim(), vec![s("Press Enter to select, ESC to go back")])],
        ));
        col(children)
    }
}

/// Manage Installation, for a status line upstream installed as a global or pinned package.
pub struct ManageInstallation {
    list: List,
}

fn manage_items() -> Vec<Item<&'static str>> {
    vec![Item::Entry(
        Entry::new("🔌 Uninstall", "uninstall").description("Remove ccstatusline from Claude Code settings"),
    )]
}

impl ManageInstallation {
    pub fn new() -> Self {
        ManageInstallation { list: List::new(0) }
    }

    pub fn handle(&mut self, ev: &Input) -> Outcome {
        let items = manage_items();
        let mut p = Props::new(&items);
        p.margin_top = 1.0;
        p.show_back_button = true;
        self.list.sync(&p);
        let picked = self.list.handle(&p, ev);
        if ev.key.escape {
            return Outcome::Cancel;
        }
        match picked {
            Event::Selected(..) => Outcome::Select,
            Event::Back => Outcome::Cancel,
            Event::None => Outcome::None,
        }
    }

    pub fn view(&mut self, current: &str) -> El {
        let items = manage_items();
        let mut p = Props::new(&items);
        p.margin_top = 1.0;
        p.show_back_button = true;
        self.list.sync(&p);
        col(vec![
            text(TextStyle::bold(), vec![s("Manage Installation")]),
            boxed(
                BoxStyle::default().margin_top(1.0),
                vec![text(TextStyle::default(), vec![s("Current:"), s(" "), s(current)])],
            ),
            self.list.view(&p),
        ])
    }
}

/// App.tsx's `FlowNotice`: a titled message and a Continue item.
pub struct FlowNotice {
    pub title: String,
    pub message: String,
    pub color: &'static str,
    list: List,
}

impl FlowNotice {
    pub fn new(title: impl Into<String>, message: impl Into<String>, color: &'static str) -> Self {
        FlowNotice { title: title.into(), message: message.into(), color, list: List::new(0) }
    }

    /// True when the notice is dismissed (ESC or Continue).
    pub fn handle(&mut self, ev: &Input) -> bool {
        let items = vec![Item::Entry(Entry::new("Continue", "continue"))];
        let mut p = Props::new(&items);
        p.margin_top = 1.0;
        p.color = Some("cyan");
        self.list.sync(&p);
        let picked = matches!(self.list.handle(&p, ev), Event::Selected(..));
        ev.key.escape || picked
    }

    pub fn view(&mut self) -> El {
        let items = vec![Item::Entry(Entry::new("Continue", "continue"))];
        let mut p = Props::new(&items);
        p.margin_top = 1.0;
        p.color = Some("cyan");
        self.list.sync(&p);
        col(vec![
            text(TextStyle::bold(), vec![s(self.title.clone())]),
            boxed(
                BoxStyle::default().margin_top(1.0),
                vec![text(TextStyle::color(self.color), vec![s(self.message.clone())])],
            ),
            self.list.view(&p),
        ])
    }
}
