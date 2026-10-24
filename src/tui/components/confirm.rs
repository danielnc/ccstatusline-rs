//! Upstream `ConfirmDialog.tsx`: a message and a Yes/No list.

use super::list::{Entry, Event, Item, List, Props};
use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;

pub enum Answer {
    Yes,
    No,
    None,
}

fn items() -> Vec<Item<bool>> {
    vec![Item::Entry(Entry::new("Yes", true)), Item::Entry(Entry::new("No", false))]
}

fn props(items: &[Item<bool>]) -> Props<'_, bool> {
    let mut p = Props::new(items);
    p.color = Some("cyan");
    p
}

#[derive(Default)]
pub struct Confirm {
    list: List,
}

impl Confirm {
    /// Both the dialog's ESC handler and its list see every key, in that order.
    pub fn handle(&mut self, ev: &Input, busy: bool) -> Answer {
        let cancel = ev.key.escape && !busy;
        let items = items();
        let p = props(&items);
        self.list.sync(&p);
        let picked = if busy { Event::None } else { self.list.handle(&p, ev) };
        if cancel {
            return Answer::No;
        }
        match picked {
            Event::Selected(true, _) => Answer::Yes,
            Event::Selected(false, _) => Answer::No,
            _ => Answer::None,
        }
    }

    pub fn view(&mut self, message: Option<&str>, inline: bool, busy: bool) -> El {
        let items = items();
        let p = props(&items);
        self.list.sync(&p);
        if inline {
            return self.list.view(&p);
        }
        col(vec![
            plain(message.unwrap_or("")),
            boxed(
                BoxStyle::default().margin_top(1.0),
                vec![if busy {
                    text(TextStyle::color("yellow"), vec![s("Working... This may take a moment.")])
                } else {
                    self.list.view(&p)
                }],
            ),
        ])
    }
}
