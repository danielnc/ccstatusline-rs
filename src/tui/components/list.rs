//! Upstream `List.tsx`: a vertical menu with separators, disabled items, sublabels and a description of the
//! selected item.

use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::*;

#[derive(Clone, Debug)]
pub struct Entry<V> {
    pub label: String,
    pub sublabel: Option<String>,
    pub disabled: bool,
    pub description: Option<String>,
    pub value: V,
}

impl<V> Entry<V> {
    pub fn new(label: impl Into<String>, value: V) -> Self {
        Entry { label: label.into(), sublabel: None, disabled: false, description: None, value }
    }
    pub fn description(mut self, d: impl Into<String>) -> Self {
        self.description = Some(d.into());
        self
    }
    pub fn sublabel(mut self, s: Option<String>) -> Self {
        self.sublabel = s;
        self
    }
    pub fn disabled(mut self, d: bool) -> Self {
        self.disabled = d;
        self
    }
}

#[derive(Clone, Debug)]
pub enum Item<V> {
    Entry(Entry<V>),
    Separator,
}

/// What a keypress did to the list.
pub enum Event<V> {
    Selected(V, usize),
    Back,
    None,
}

/// The list's state: `selectedIndex` and what its reset effect last saw.
#[derive(Clone, Debug, Default)]
pub struct List {
    pub selected: usize,
    synced: Option<(usize, usize)>,
}

pub struct Props<'a, V> {
    pub items: &'a [Item<V>],
    pub initial_selection: usize,
    pub show_back_button: bool,
    pub color: Option<&'a str>,
    pub wrap_navigation: bool,
    pub margin_top: f32,
}

impl<'a, V> Props<'a, V> {
    pub fn new(items: &'a [Item<V>]) -> Self {
        Props {
            items,
            initial_selection: 0,
            show_back_button: false,
            color: None,
            wrap_navigation: true,
            margin_top: 0.0,
        }
    }
}

enum Row<'a, V> {
    Entry(&'a Entry<V>),
    Back,
    Separator,
}

fn rows<'a, V>(p: &Props<'a, V>) -> Vec<Row<'a, V>> {
    let mut out: Vec<Row<V>> = p
        .items
        .iter()
        .map(|i| match i {
            Item::Entry(e) => Row::Entry(e),
            Item::Separator => Row::Separator,
        })
        .collect();
    if p.show_back_button {
        out.push(Row::Separator);
        out.push(Row::Back);
    }
    out
}

fn selectable<'a, V>(rows: &[Row<'a, V>]) -> Vec<usize> {
    rows.iter()
        .enumerate()
        .filter(|(_, r)| match r {
            Row::Entry(e) => !e.disabled,
            Row::Back => true,
            Row::Separator => false,
        })
        .map(|(i, _)| i)
        .collect()
}

impl List {
    pub fn new(initial: usize) -> Self {
        List { selected: initial, synced: None }
    }

    /// The effect on `[initialSelection, selectableItems.length]`: clamp to the initial selection whenever
    /// either changes (and on mount).
    pub fn sync<V>(&mut self, p: &Props<V>) {
        let n = selectable(&rows(p)).len();
        let key = (p.initial_selection, n);
        if self.synced != Some(key) {
            self.synced = Some(key);
            self.selected = p.initial_selection.min(n.saturating_sub(1));
        }
    }

    pub fn handle<V: Clone>(&mut self, p: &Props<V>, ev: &Input) -> Event<V> {
        let rows = rows(p);
        let sel = selectable(&rows);
        let n = sel.len() as i64;
        let k = &ev.key;
        if k.up_arrow {
            let prev = self.selected as i64 - 1;
            self.selected = if prev < 0 {
                if p.wrap_navigation {
                    (n - 1).max(0)
                } else {
                    0
                }
            } else {
                prev
            } as usize;
            return Event::None;
        }
        if k.down_arrow {
            let next = self.selected as i64 + 1;
            self.selected = if next > n - 1 {
                if p.wrap_navigation {
                    0
                } else {
                    (n - 1).max(0)
                }
            } else {
                next
            } as usize;
            return Event::None;
        }
        if k.return_ {
            if let Some(&ri) = sel.get(self.selected) {
                return match &rows[ri] {
                    Row::Entry(e) => Event::Selected(e.value.clone(), self.selected),
                    Row::Back => Event::Back,
                    Row::Separator => Event::None,
                };
            }
        }
        Event::None
    }

    pub fn selected_value<'v, V>(&self, p: &Props<'v, V>) -> Option<&'v V> {
        let rows = rows(p);
        let sel = selectable(&rows);
        match sel.get(self.selected).map(|&i| &rows[i]) {
            Some(Row::Entry(e)) => Some(&e.value),
            _ => None,
        }
    }

    pub fn view<V>(&self, p: &Props<V>) -> El {
        let rows = rows(p);
        let sel = selectable(&rows);
        let actual = sel.get(self.selected).copied();
        let mut children: Vec<El> = Vec::new();
        let mut description: Option<&str> = None;
        for (index, r) in rows.iter().enumerate() {
            let selected = Some(index) == actual;
            match r {
                Row::Separator => children.push(plain(" ")),
                Row::Back => children.push(list_item("← Back", None, selected, false, p.color)),
                Row::Entry(e) => {
                    if selected {
                        description = e.description.as_deref();
                    }
                    children.push(list_item(&e.label, e.sublabel.as_deref(), selected, e.disabled, p.color));
                }
            }
        }
        if let Some(d) = description.filter(|d| !d.is_empty()) {
            children.push(boxed(
                BoxStyle::default().margin_top(1.0).padding_left(2.0),
                vec![text(TextStyle::dim(), vec![s(d)])],
            ));
        }
        boxed(BoxStyle::column().margin_top(p.margin_top), children)
    }
}

/// `ListItem` around `<Text><Text>{label}</Text>{sublabel}</Text>`.
pub fn list_item(label: &str, sublabel: Option<&str>, selected: bool, disabled: bool, color: Option<&str>) -> El {
    let mut inner = vec![nested(TextStyle::default(), vec![s(label)])];
    if let Some(sub) = sublabel.filter(|s| !s.is_empty()) {
        inner.push(nested(TextStyle::default().with_dim(!selected), vec![s(" "), s(sub)]));
    }
    list_item_el(nested(TextStyle::default(), inner), selected, disabled, color)
}

/// `ListItem` with arbitrary children.
pub fn list_item_el(children: Inline, selected: bool, disabled: bool, color: Option<&str>) -> El {
    let color = color.unwrap_or("green");
    let style = TextStyle::default().with_color(if selected { Some(color) } else { None }).with_dim(disabled);
    row(vec![text(
        style,
        vec![
            nested(TextStyle::default(), vec![s(if selected { "▶  " } else { "   " })]),
            nested(TextStyle::default(), vec![children]),
        ],
    )])
}
