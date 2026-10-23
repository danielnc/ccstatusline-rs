//! Upstream `PowerlineSetup.tsx`, `PowerlineThemeSelector.tsx` and `PowerlineSeparatorEditor.tsx`. Not ported yet: blank, ESC goes back.

use crate::tui::ink::keys::Input;
use crate::tui::ink::layout::El;
use crate::tui::screens::{Action, Ctx, Placeholder, Screen};

pub struct PowerlineSetup;

impl PowerlineSetup {
    pub fn new(_ctx: &Ctx) -> Self {
        PowerlineSetup
    }
}

impl Screen for PowerlineSetup {
    fn handle(&mut self, ev: &Input, ctx: &Ctx) -> Vec<Action> {
        Placeholder.handle(ev, ctx)
    }

    fn view(&mut self, ctx: &Ctx) -> El {
        Placeholder.view(ctx)
    }
}
