//! App.tsx's `FlowNotice`: a titled message and a lone Continue item; Enter or ESC continues.

use super::list::{Entry, Event, Item, List, Props};
use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;

pub struct FlowNotice {
    pub title: String,
    pub message: String,
    pub color: &'static str,
    list: List,
}

fn items() -> Vec<Item<()>> {
    vec![Item::Entry(Entry::new("Continue", ()))]
}

fn props(items: &[Item<()>]) -> Props<'_, ()> {
    let mut p = Props::new(items);
    p.margin_top = 1.0;
    p.color = Some("cyan");
    p
}

impl FlowNotice {
    pub fn new(title: String, message: String, color: &'static str) -> Self {
        FlowNotice { title, message, color, list: List::new(0) }
    }

    /// Whether the key continues (`onContinue`).
    pub fn handle(&mut self, ev: &Input) -> bool {
        let items = items();
        let p = props(&items);
        self.list.sync(&p);
        let selected = matches!(self.list.handle(&p, ev), Event::Selected(..));
        selected || ev.key.escape
    }

    pub fn view(&mut self) -> El {
        let items = items();
        let p = props(&items);
        self.list.sync(&p);
        boxed(
            BoxStyle::column(),
            vec![
                text(TextStyle::bold(), vec![s(&self.title)]),
                boxed(
                    BoxStyle::default().margin_top(1.0),
                    vec![text(TextStyle::color(self.color), vec![s(&self.message)])],
                ),
                self.list.view(&p),
            ],
        )
    }
}
