//! Upstream `TerminalWidthMenu.tsx`. Not ported yet: blank, ESC goes back.

use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::El;
use crate::tui::screens::{Action, Ctx, Placeholder, Screen};

pub struct TerminalWidthMenu;

impl TerminalWidthMenu {
    pub fn new(_ctx: &Ctx) -> Self {
        TerminalWidthMenu
    }
}

impl Screen for TerminalWidthMenu {
    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        Placeholder.handle(ev, ctx)
    }

    fn view(&mut self, ctx: &Ctx) -> El {
        Placeholder.view(ctx)
    }
}
