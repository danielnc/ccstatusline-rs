//! Upstream `GlobalOverridesMenu.tsx`. Not ported yet: blank, ESC goes back.

use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::El;
use crate::tui::screens::{Action, Ctx, Placeholder, Screen};

pub struct GlobalOverridesMenu;

impl GlobalOverridesMenu {
    pub fn new(_ctx: &Ctx) -> Self {
        GlobalOverridesMenu
    }
}

impl Screen for GlobalOverridesMenu {
    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        Placeholder.handle(ev, ctx)
    }

    fn view(&mut self, ctx: &Ctx) -> El {
        Placeholder.view(ctx)
    }
}
