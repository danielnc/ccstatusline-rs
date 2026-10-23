//! Upstream `TerminalOptionsMenu.tsx`. Not ported yet: blank, ESC goes back.

use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::El;
use crate::tui::screens::{Action, Ctx, Placeholder, Screen};

pub struct TerminalOptionsMenu;

impl TerminalOptionsMenu {
    pub fn new(_ctx: &Ctx) -> Self {
        TerminalOptionsMenu
    }
}

impl Screen for TerminalOptionsMenu {
    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        Placeholder.handle(ev, ctx)
    }

    fn view(&mut self, ctx: &Ctx) -> El {
        Placeholder.view(ctx)
    }
}
